import concurrent.futures
import json
import tempfile
import threading
import time
import sqlite3
import unittest
from unittest.mock import patch
from http.server import ThreadingHTTPServer
from urllib.request import urlopen
from urllib.error import HTTPError
from cache import Cache, day, handler, latest_comments


class CacheTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.calls = []
        def fetch(path, query=""):
            self.calls.append((path, query))
            return {"comments": [{"id": i, "updatedTime": f"2026-10-{i:02}", "text": "评论"} for i in range(1, 21)], "hasMore": True}
        self.cache = Cache(self.temp.name, fetch)

    def test_one_page_per_day_and_persistent_across_restart(self):
        body = self.cache.refresh("comments/1")
        self.assertEqual(len(body["comments"]), 20)
        self.assertEqual(body["comments"][0]["id"], 20)
        self.assertEqual(self.calls, [("/api/v2/bangumi/1/comments", "?page=0")])
        again = Cache(self.temp.name, self.cache.fetch)
        self.assertEqual(again.refresh("comments/1"), body)
        self.assertEqual(len(self.calls), 1)

    def test_connections_close_after_requests(self):
        with self.cache.connect() as db:
            db.execute("SELECT 1")
        with self.assertRaises(sqlite3.ProgrammingError):
            db.execute("SELECT 1")

    def test_hd_recovery_does_not_repeat_dandanplay_requests(self):
        self.cache.write("details/1", {"bangumi": {"animeId": 1, "originalImageUrl": "original", "bangumiUrl": "https://bgm.tv/subject/2", "imageQuality": "standard"}})
        fetched = self.cache.read("details/1")[0]
        self.cache.poster = lambda *_: ("/cached-hd.jpg", "high")
        self.cache.upgrade_poster("details/1")
        stamp, body = self.cache.read("details/1")
        self.assertEqual(body["bangumi"]["imageQuality"], "high")
        self.assertEqual(stamp, fetched)
        self.assertEqual(self.calls, [])

    def test_missing_hd_poster_does_not_block_other_shows(self):
        original = "https://assets.anixplayer.net/image/poster/medium/1.jpg"
        def save(url, name, timeout=12):
            if "/subjects/1/" in url:
                raise OSError("unavailable")
            return "/images/" + name
        with patch.object(self.cache, "save_image", side_effect=save) as save_image:
            self.assertEqual(self.cache.poster(original, "https://bgm.tv/subject/1")[1], "standard")
            self.assertEqual(self.cache.poster(original, "https://bgm.tv/subject/2")[1], "high")
            self.cache.poster(original, "https://bgm.tv/subject/1")
            failed_requests = [c for c in save_image.call_args_list if "/subjects/1/" in c.args[0]]
            self.assertEqual(len(failed_requests), 1)

    def test_concurrent_cache_miss_only_fetches_once(self):
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            list(pool.map(lambda _: self.cache.refresh("comments/1"), range(12)))
        self.assertEqual(len(self.calls), 1)

    def test_failed_refresh_preserves_snapshot_and_retries_later(self):
        self.cache.refresh("comments/1")
        with self.cache.connect() as db:
            db.execute("UPDATE snapshots SET fetched=0")
        def broken(*_):
            raise OSError("offline")
        self.cache.fetch = broken
        result = self.cache.refresh("comments/1")
        self.assertTrue(result["stale"])
        self.assertEqual(len(result["comments"]), 20)
        self.assertEqual(self.cache.read("comments/1")[0], 0)

    def test_beijing_day_boundary_and_deduplication(self):
        self.assertEqual(day(1792771199), "2026-10-23")
        self.assertEqual(day(1792771200), "2026-10-24")
        rows = [{"id": 1, "updatedTime": "2026-10-20"}, {"id": 1, "updatedTime": "2026-10-24"}]
        self.assertEqual(latest_comments(rows), [rows[1]])

    def test_anonymous_routes_serve_cached_data_and_live_is_separate(self):
        server = ThreadingHTTPServer(("127.0.0.1", 0), handler(self.cache))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        self.addCleanup(server.server_close)
        self.addCleanup(server.shutdown)
        base = f"http://127.0.0.1:{server.server_port}"
        for _ in range(2):
            with urlopen(base + "/comments/1") as r:
                self.assertEqual(len(json.load(r)["comments"]), 20)
                self.assertEqual(r.headers["Access-Control-Allow-Origin"], "*")
        self.assertEqual(len(self.calls), 1)
        with urlopen(base + "/live/comments/1") as r:
            self.assertEqual(r.headers["Cache-Control"], "private, no-store")
        self.assertEqual(len(self.calls), 2)
        for path in ["/api/v2/playhistory", "/details/-1", "/images/../cache.sqlite3"]:
            with self.assertRaises(HTTPError) as err:
                urlopen(base + path)
            self.assertEqual(err.exception.code, 404)
            err.exception.close()


if __name__ == "__main__":
    unittest.main()
