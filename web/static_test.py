#!/usr/bin/env python3
"""Verify the packaged pure Web app from a fresh static-site subdirectory.

The server only serves files and byte ranges. The browser blocks other origins
and WebSockets, so a running native Erika cannot supply the playback backend.
"""
from functools import partial
from http.server import ThreadingHTTPServer
from pathlib import Path
import argparse
import importlib.util
import os
import subprocess
import tempfile
import threading
import zipfile
from dependencies import liberika_root


def main():
    root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--archive', type=Path,
                        default=root / 'target/bern/dist/erika-0.1.0-web.zip')
    args = parser.parse_args()
    liberika = liberika_root()
    fixtures = liberika / 'target/web/fixtures'
    chapters = root / 'target/web-review/fixtures/chapters.mkv'
    if not all(p.is_file() for p in [fixtures / 'test.mkv', fixtures / 'test.webm', chapters]):
        subprocess.run(['python3', root / 'web/test_fixtures.py'], cwd=root, check=True)

    # This handler implements only static GET/HEAD and HTTP byte ranges.
    spec = importlib.util.spec_from_file_location('liberika_static_server', liberika / 'scripts/serve_web.py')
    server_module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(server_module)
    output = root / 'target/web-static-review'
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix='site-', dir=output) as directory:
        site = Path(directory)
        with zipfile.ZipFile(args.archive) as archive:
            for entry in archive.infolist():
                path = (site / entry.filename).resolve()
                if not path.is_relative_to(site):
                    raise ValueError(f'Archive path escapes the site: {entry.filename}')
            archive.extractall(site)
        apps = list(site.glob('*/app.js'))
        if len(apps) != 1:
            raise ValueError('Expected one packaged Web application')
        app = apps[0].parent
        if (app / 'bin').exists() or (app / 'extension/liberika/subfont.ttf').exists():
            raise ValueError('Unexpected native host or duplicate font in Web package')
        # Synthetic test input only; application files all come from the ZIP.
        (app / 'test-fixtures').mkdir()
        (app / 'test-fixtures/chapters.mkv').write_bytes(chapters.read_bytes())
        server = ThreadingHTTPServer(('127.0.0.1', 0), partial(server_module.Handler, directory=site))
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            env = os.environ.copy()
            env.update({
                'ERIKA_APP_URL': f'http://127.0.0.1:{server.server_port}/{app.name}/',
                'ERIKA_APP_ROOT': str(app),
                'ERIKA_APP_REPORT_DIR': str(output),
                'LIBERIKA_FIXTURES': str(fixtures),
                'ERIKA_STATIC_ONLY': '1',
            })
            env.setdefault('PLAYWRIGHT_MODULE', str(liberika / 'target/web-tests/node_modules/playwright/index.mjs'))
            subprocess.run(['node', root / 'web/browser.test.mjs'], cwd=root, env=env, check=True)
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == '__main__':
    main()
