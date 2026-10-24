#!/usr/bin/env python3
"""Public anime snapshots. Python 3 standard library; secrets stay on the server."""
import base64
import datetime as dt
import hashlib
import json
import logging
import os
from pathlib import Path
import re
import sqlite3
import threading
import time
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

CST = dt.timezone(dt.timedelta(hours=8))
LOG = logging.getLogger("anime-cache")


def day(timestamp=None):
    return dt.datetime.fromtimestamp(time.time() if timestamp is None else timestamp, CST).date().isoformat()


def latest_comments(rows):
    """The latest 20 available as of the daily refresh, including older dates."""
    unique = {}
    for row in rows:
        key = str(row.get("id") or (row.get("externalUserId"), row.get("text")))
        if key not in unique or str(row.get("updatedTime", "")) > str(unique[key].get("updatedTime", "")):
            unique[key] = row
    return sorted(unique.values(), key=lambda r: str(r.get("updatedTime", "")), reverse=True)[:20]


class Cache:
    def __init__(self, root, fetch=None):
        self.root = Path(root)
        self.root.mkdir(parents=True, exist_ok=True)
        (self.root / "images").mkdir(exist_ok=True)
        self.db = self.root / "cache.sqlite3"
        with self.connect() as db:
            db.execute("PRAGMA journal_mode=WAL")
            db.execute("CREATE TABLE IF NOT EXISTS snapshots (key TEXT PRIMARY KEY, fetched INTEGER, body TEXT)")
        self.fetch = fetch or self.upstream
        self.slots = threading.BoundedSemaphore(2)
        self.locks = [threading.Lock() for _ in range(128)]
        self.failures = {}
        self.live_lock = threading.Lock()
        self.live_windows = {}
        self.hq_retry_after = {}
        proxy = os.environ.get("ANIME_IMAGE_PROXY")
        self.bangumi_images = None
        if proxy and urlsplit(proxy).hostname in {"127.0.0.1", "localhost"}:
            from images import BangumiImages
            self.bangumi_images = BangumiImages(proxy)
        self.image_opener = urllib.request.build_opener(
            urllib.request.ProxyHandler({"https": proxy}) if proxy else urllib.request.ProxyHandler()
        )

    @contextmanager
    def connect(self):
        # sqlite3's own context manager commits but does not close. A long-lived
        # cache server must release descriptors after every request/refresh.
        db = sqlite3.connect(self.db, timeout=15)
        try:
            with db:
                yield db
        finally:
            db.close()

    def read(self, key):
        with self.connect() as db:
            row = db.execute("SELECT fetched, body FROM snapshots WHERE key=?", (key,)).fetchone()
        return (row[0], json.loads(row[1])) if row else None

    def write(self, key, body):
        fetched = int(time.time())
        body = dict(body, cachedAt=fetched, cacheDate=day(fetched))
        with self.connect() as db:
            db.execute("INSERT OR REPLACE INTO snapshots VALUES (?, ?, ?)", (key, fetched, json.dumps(body, ensure_ascii=False)))
        return body

    def upstream(self, path, query=""):
        app = os.environ.get("DANDANPLAY_APP_ID", "nipaplayv1")
        timestamp = str(int(time.time()))
        secret = os.environ["DANDANPLAY_APP_SECRET"]
        signature = base64.b64encode(hashlib.sha256((app + timestamp + path + secret).encode()).digest()).decode()
        base = os.environ.get("DANDANPLAY_UPSTREAM_BASE", "https://api.dandanplay.net").rstrip("/")
        request = urllib.request.Request(base + path + query, headers={
            "X-AppId": app, "X-Timestamp": timestamp, "X-Signature": signature,
            "Accept": "application/json", "User-Agent": "NipaPlay/1.0",
        })
        with self.slots, urllib.request.urlopen(request, timeout=12) as response:
            body = json.loads(response.read(8 * 1024 * 1024))
        if not isinstance(body, dict) or body.get("success") is False:
            raise ValueError("upstream rejected request")
        return body

    def comments(self, anime_id):
        body = self.fetch(f"/api/v2/bangumi/{anime_id}/comments", "?page=0")
        rows = body.get("comments")
        if not isinstance(rows, list):
            raise ValueError("missing comments array")
        rows = latest_comments(rows)
        return {"success": True, "comments": rows, "count": len(rows), "source": "dandanplay"}

    def save_image(self, url, name, timeout=12):
        dest = self.root / "images" / name
        if not dest.exists():
            request = urllib.request.Request(url, headers={"User-Agent": "NipaPlay/1.0"})
            with self.slots, self.image_opener.open(request, timeout=timeout) as response:
                if not response.headers.get("Content-Type", "").startswith("image/"):
                    raise ValueError("poster response is not an image")
                data = response.read(12 * 1024 * 1024 + 1)
            if len(data) > 12 * 1024 * 1024 or not data:
                raise ValueError("invalid poster size")
            temp = dest.with_suffix(f".{threading.get_ident()}.tmp")
            temp.write_bytes(data)
            temp.replace(dest)
        return f"/dandanplay/public/images/{name}"

    def poster(self, url, subject_url=""):
        if not url:
            return url, "unavailable"
        subject = re.fullmatch(r"https?://(?:bangumi.tv|bgm.tv|chii.in)/subject/([1-9][0-9]*)/?", subject_url)
        if subject:
            high = f"https://api.bgm.tv/v0/subjects/{subject[1]}/image?type=large"
            name = hashlib.sha256((high + url).encode()).hexdigest() + ".jpg"
            if (self.root / "images" / name).exists() or time.time() >= self.hq_retry_after.get(name, 0):
                try:
                    dest = self.root / "images" / name
                    if not dest.exists() and self.bangumi_images:
                        with self.slots:
                            data = self.bangumi_images.poster(subject[1])
                        temporary = dest.with_suffix(f".{threading.get_ident()}.tmp")
                        temporary.write_bytes(data)
                        temporary.replace(dest)
                    image = self.save_image(high, name, 4)
                    self.hq_retry_after.pop(name, None)
                    return image, "high"
                except Exception:
                    # A missing poster must not block unrelated shows' upgrades.
                    self.hq_retry_after[name] = time.time() + 300
                    LOG.warning("Bangumi poster unavailable; retaining high-resolution cache where present")
        # Only cache upstream poster hosts; never fetch a user-supplied URL.
        parts = urlsplit(url)
        if parts.scheme != "https" or parts.hostname not in {"assets.anixplayer.net", "cdn.dandanplay.net"}:
            return url, "upstream"
        name = hashlib.sha256(url.encode()).hexdigest() + ".jpg"
        return self.save_image(url, name), "standard"

    def build(self, key):
        if key == "season":
            body = self.fetch("/api/v2/bangumi/shin", "?filterAdultContent=true")
            rows = body.get("bangumiList")
            if not isinstance(rows, list) or not rows:
                raise ValueError("empty season response")
            for row in rows:
                old = self.read(f"details/{row['animeId']}")
                if old:
                    details = old[1]["bangumi"]
                    row["summary"] = details.get("summary", "")
                    row["imageUrl"] = details.get("imageUrl") or row.get("imageUrl", "")
                row["isFavorited"] = False
            return dict(body, bangumiList=rows)
        kind, raw_id = key.split("/")
        anime_id = int(raw_id)
        if kind == "comments":
            return self.comments(anime_id)
        body = self.fetch(f"/api/v2/bangumi/{anime_id}")
        details = body.get("bangumi")
        if not isinstance(details, dict) or details.get("animeId") != anime_id:
            raise ValueError("invalid details response")
        # No account context is sent to upstream, and user-specific fields are stripped.
        for field in ("userRating", "favoriteStatus", "comment"):
            details.pop(field, None)
        details["isFavorited"] = False
        details["originalImageUrl"] = details.get("imageUrl", "")
        try:
            details["imageUrl"], details["imageQuality"] = self.poster(details["originalImageUrl"], details.get("bangumiUrl", ""))
        except Exception:
            old = self.read(key)
            if old:
                details["imageUrl"] = old[1]["bangumi"].get("imageUrl", details["originalImageUrl"])
                details["imageQuality"] = old[1]["bangumi"].get("imageQuality", "upstream")
            else:
                details["imageQuality"] = "upstream"
        return {"success": True, "bangumi": details}

    def refresh(self, key, force=False):
        with self.locks[hash(key) % len(self.locks)]:
            old = self.read(key)
            if not force and old and day(old[0]) == day():
                return old[1]
            if time.time() < self.failures.get(key, 0):
                if old:
                    return dict(old[1], stale=True)
                raise ValueError("cache refresh pending retry")
            try:
                body = self.write(key, self.build(key))
                self.failures.pop(key, None)
                return body
            except Exception:
                # Avoid logging upstream URLs/headers or credentials.
                LOG.warning("refresh failed: %s; keeping previous snapshot", key)
                self.failures[key] = time.time() + 300
                if old:
                    return dict(old[1], stale=True)
                raise

    def get(self, key):
        old = self.read(key)
        if old:
            # The worker refreshes daily; cached reads never wait on upstream.
            return dict(old[1], stale=day(old[0]) != day())
        # Bound the catalog so arbitrary anonymous IDs cannot grow storage forever.
        with self.connect() as db:
            count = db.execute("SELECT COUNT(*) FROM snapshots").fetchone()[0]
        if count >= 6000:
            raise ValueError("catalog full")
        return self.refresh(key)

    def allow_live(self, ip):
        now = int(time.time() // 60)
        with self.live_lock:
            self.live_windows = {k: v for k, v in self.live_windows.items() if v[0] == now}
            keys = [("global", 60), ("ip:" + ip, 10)]
            if any(self.live_windows.get(k, (now, 0))[1] >= limit for k, limit in keys):
                return False
            for k, _ in keys:
                self.live_windows[k] = (now, self.live_windows.get(k, (now, 0))[1] + 1)
        return True

    def daily(self):
        season = self.refresh("season")
        ids = {r["animeId"] for r in season["bangumiList"]}
        with self.connect() as db:
            # Refresh previously viewed non-season anime as well.
            keys = {row[0] for row in db.execute("SELECT key FROM snapshots WHERE key != 'season'")}
        keys.update(f"{kind}/{i}" for i in ids for kind in ("details", "comments"))
        with ThreadPoolExecutor(max_workers=4) as pool:
            for key, result in zip(sorted(keys), pool.map(self.safe_refresh, sorted(keys))):
                if not result:
                    LOG.warning("snapshot unavailable: %s", key)
            # Retry only missing HD images after connectivity recovers, without
            # spending another dandanplay metadata/comment request that day.
            list(pool.map(self.upgrade_poster, sorted(k for k in keys if k.startswith("details/"))))
        # Publish enriched intros atomically, without repeating the season request.
        for row in season["bangumiList"]:
            detail = self.read(f"details/{row['animeId']}")
            if detail:
                row.update({k: detail[1]["bangumi"].get(k, "") for k in ("imageUrl", "summary")})
        # Preserve the original season timestamp if its upstream refresh failed.
        if not season.get("stale"):
            self.write("season", season)
        LOG.info("daily cache ready: %s, %d season shows", day(), len(ids))

    def upgrade_poster(self, key):
        try:
            with self.locks[hash(key) % len(self.locks)]:
                snapshot = self.read(key)
                if not snapshot:
                    return
                body = snapshot[1]
                detail = body["bangumi"]
                if detail.get("imageQuality") == "high":
                    return
                image, quality = self.poster(detail.get("originalImageUrl", ""), detail.get("bangumiUrl", ""))
                if quality == "high":
                    detail.update(imageUrl=image, imageQuality=quality)
                    with self.connect() as db:
                        db.execute("UPDATE snapshots SET body=? WHERE key=?", (json.dumps(body, ensure_ascii=False), key))
        except Exception:
            LOG.warning("poster upgrade pending: %s", key)

    def safe_refresh(self, key):
        try:
            return self.refresh(key)
        except Exception:
            return None

    def worker(self):
        while True:
            try:
                self.daily()
            except Exception:
                LOG.exception("daily refresh failed")
            time.sleep(300)


def handler(cache):
    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_args):
            pass

        def respond(self, status, body, content_type="application/json", cache_control="public, max-age=300"):
            if not isinstance(body, bytes):
                body = json.dumps(body, ensure_ascii=False).encode()
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", cache_control if status == 200 else "no-store")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.end_headers()
            self.wfile.write(body)

        def do_GET(self):
            path = urlsplit(self.path).path
            if path == "/healthz":
                snapshot = cache.read("season")
                return self.respond(200, {"ok": True, "cacheDate": snapshot[1].get("cacheDate") if snapshot else None})
            if path.startswith("/images/"):
                name = path.removeprefix("/images/")
                if re.fullmatch(r"[a-f0-9]{64}\.jpg", name):
                    try:
                        data = (cache.root / "images" / name).read_bytes()
                        mime = "image/png" if data.startswith(b"\x89PNG") else "image/webp" if data.startswith(b"RIFF") else "image/jpeg"
                        return self.respond(200, data, mime, "public, max-age=31536000, immutable")
                    except FileNotFoundError:
                        pass
                return self.respond(404, {"success": False})
            match = re.fullmatch(r"/(details|comments|live/comments)/([1-9][0-9]{0,8})", path)
            if path != "/season" and not match:
                return self.respond(404, {"success": False})
            key = path.lstrip("/")
            # Nginx overwrites this header; the listener is loopback only.
            ip = self.headers.get("X-Real-IP", self.client_address[0])
            live = key.startswith("live/")
            try:
                if (live or cache.read(key) is None) and not cache.allow_live(ip):
                    return self.respond(429, {"success": False, "errorMessage": "请求过于频繁，请稍后重试"})
                body = cache.comments(int(match[2])) if live else cache.get(key)
                return self.respond(200, body, cache_control="private, no-store" if live else "public, max-age=300")
            except Exception:
                return self.respond(503, {"success": False, "errorMessage": "缓存暂时不可用，请稍后重试"})
    return Handler


if __name__ == "__main__":
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    cache = Cache(os.environ.get("ANIME_CACHE_DIR", "/var/lib/erika-anime-cache"))
    threading.Thread(target=cache.worker, daemon=True).start()
    ThreadingHTTPServer(("127.0.0.1", int(os.environ.get("ANIME_CACHE_PORT", "18082"))), handler(cache)).serve_forever()
