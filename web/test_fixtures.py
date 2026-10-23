#!/usr/bin/env python3
"""Create synthetic media for browser.test.mjs; no user videos are used."""
from pathlib import Path
import subprocess
from dependencies import liberika_root

root = Path(__file__).resolve().parents[1]
liberika = liberika_root()
subprocess.run(['python3', liberika / 'scripts/web_test_fixtures.py'], check=True)
fixtures = root / 'target/web-review/fixtures'
fixtures.mkdir(parents=True, exist_ok=True)
metadata = ';FFMETADATA1\n'
for start, end, title in [(0, 3000, 'OP'), (3000, 9000, '正文'), (9000, 12000, 'ED')]:
    metadata += f'[CHAPTER]\nTIMEBASE=1/1000\nSTART={start}\nEND={end}\ntitle={title}\n'
(fixtures / 'chapters.txt').write_text(metadata)
subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', '-i', liberika / 'target/web/fixtures/test.mkv',
                '-i', fixtures / 'chapters.txt', '-map', '0', '-map_chapters', '1', '-c', 'copy', fixtures / 'chapters.mkv'], check=True)
link = root / 'target/bern/web_app/test-fixtures'
if not link.exists():
    link.symlink_to(fixtures, target_is_directory=True)
