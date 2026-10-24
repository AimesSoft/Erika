#!/usr/bin/env python3
"""Bootstrap HD posters from a machine that can reach Bangumi.

Only public metadata is read. Copy the output's images/ to the service's images/
and run `--import-dir` on the server to update snapshots atomically. The normal
daily worker reuses these posters, even if the image source is temporarily down.
"""
import argparse
from concurrent.futures import ThreadPoolExecutor
import hashlib
import json
from pathlib import Path
import re
import shutil
import urllib.request


def download(base, output):
    output.mkdir(parents=True, exist_ok=True)
    (output / "images").mkdir(exist_ok=True)
    def get(path):
        with urllib.request.urlopen(base + path, timeout=20) as r:
            return json.load(r)
    season = get("/season")["bangumiList"]
    def one(row):
        anime_id = row["animeId"]
        try:
            detail = get(f"/details/{anime_id}")["bangumi"]
            subject = re.fullmatch(r"https?://(?:bangumi.tv|bgm.tv|chii.in)/subject/([1-9][0-9]*)/?", detail.get("bangumiUrl", ""))
            if not subject:
                return None
            url = f"https://api.bgm.tv/v0/subjects/{subject[1]}/image?type=large"
            name = hashlib.sha256((url + detail["originalImageUrl"]).encode()).hexdigest() + ".jpg"
            if (output / "images" / name).exists():
                return {"id": anime_id, "name": name, "original": detail["originalImageUrl"]}
            request = urllib.request.Request(url, headers={"User-Agent": "NipaPlay/1.0"})
            with urllib.request.urlopen(request, timeout=30) as r:
                if not r.headers.get("Content-Type", "").startswith("image/"):
                    raise ValueError("not an image")
                data = r.read(12 * 1024 * 1024 + 1)
            if not data or len(data) > 12 * 1024 * 1024:
                raise ValueError("invalid image size")
            (output / "images" / name).write_bytes(data)
            print(f"{anime_id}: high-resolution poster ({len(data)} bytes)", flush=True)
            return {"id": anime_id, "name": name, "original": detail["originalImageUrl"]}
        except Exception as error:
            print(f"{anime_id}: {type(error).__name__} {getattr(error, 'code', '')}", flush=True)
            return None
    with ThreadPoolExecutor(max_workers=2) as pool:
        rows = [r for r in pool.map(one, season) if r]
    (output / "posters.json").write_text(json.dumps(rows))
    print(f"Ready: {len(rows)}/{len(season)}", flush=True)


def import_posters(directory, root):
    from cache import Cache
    cache = Cache(root)
    for image in (directory / "images").glob("*.jpg"):
        if re.fullmatch(r"[a-f0-9]{64}\.jpg", image.name):
            shutil.copyfile(image, cache.root / "images" / image.name)
    with cache.connect() as db:
        for key, raw in db.execute("SELECT key, body FROM snapshots WHERE key LIKE 'details/%'").fetchall():
            body = json.loads(raw)
            detail = body["bangumi"]
            subject = re.fullmatch(r"https?://(?:bangumi.tv|bgm.tv|chii.in)/subject/([1-9][0-9]*)/?", detail.get("bangumiUrl", ""))
            if not subject:
                continue
            url = f"https://api.bgm.tv/v0/subjects/{subject[1]}/image?type=large"
            name = hashlib.sha256((url + detail["originalImageUrl"]).encode()).hexdigest() + ".jpg"
            if not (cache.root / "images" / name).exists():
                continue
            body["bangumi"]["imageUrl"] = "/dandanplay/public/images/" + name
            body["bangumi"]["imageQuality"] = "high"
            db.execute("UPDATE snapshots SET body=? WHERE key=?", (json.dumps(body, ensure_ascii=False), key))
    cache.daily()


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--base", default="http://43.142.85.190/dandanplay/public")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--import-dir", type=Path)
    parser.add_argument("--cache-dir", default="/var/lib/erika-anime-cache")
    args = parser.parse_args()
    if args.import_dir:
        import_posters(args.import_dir, args.cache_dir)
    elif args.output:
        download(args.base.rstrip("/"), args.output)
    else:
        parser.error("provide --output or --import-dir")
