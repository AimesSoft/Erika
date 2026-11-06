#!/usr/bin/env python3
"""Bern browser extension build: build liberika and stage its complete runtime."""
from pathlib import Path
import os
import shutil
import subprocess
import sys
from dependencies import liberika_root

HERE = Path(__file__).resolve().parent
out = Path(sys.argv[1]).resolve()
# Already built, on another machine: the extension is the same on every target.
built = os.environ.get('ERIKA_WEB_EXTENSION')
if built:
    shutil.copytree(built, out, dirs_exist_ok=True)
    print(f'Erika browser playback: {out}', flush=True)
    sys.exit(0)
LIBERIKA = liberika_root()
# The native dependency build is incremental too; always run it so a source or
# toolchain update cannot silently package an old WASM module.
args = ['cargo', 'run', '-p', 'xtask', '--', 'web', 'build']
if os.environ.get('LIBERIKA_SKIP_NATIVE') == '1':
    args.append('--skip-native')
subprocess.run(args, cwd=LIBERIKA, check=True)
out.mkdir(parents=True, exist_ok=True)
for name in ['index.js', 'player.js', 'player.css', 'remote.js']:
    shutil.copy2(HERE / name, out / name)
package = out / 'liberika'
package.mkdir(exist_ok=True)
source = LIBERIKA / 'target/web'
# Only the package and its notices; no fixtures, logs, test videos or demo page.
for entry in source.iterdir():
    if entry.is_file() and entry.name != 'index.html':
        shutil.copy2(entry, package / entry.name)
# Older packages carried a second copy of the font now shared from Rust WASM.
(package / 'subfont.ttf').unlink(missing_ok=True)
shutil.copytree(source / 'licenses', package / 'licenses', dirs_exist_ok=True)
print(f'Erika browser playback: {out}', flush=True)
