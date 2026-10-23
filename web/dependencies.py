"""Locate liberika from Cargo's resolved Git dependency or local patch."""
from pathlib import Path
import json
import os
import subprocess


def liberika_root():
    override = os.environ.get('LIBERIKA_DIR')
    if override:
        root = Path(override).expanduser().resolve()
    else:
        project = Path(__file__).resolve().parents[1]
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--format-version', '1'], cwd=project, text=True,
        ))
        packages = [p for p in metadata['packages'] if p['name'] == 'erika']
        if len(packages) != 1:
            raise RuntimeError('Expected one resolved liberika (erika) Cargo dependency')
        root = Path(packages[0]['manifest_path']).resolve().parents[2]
    if not (root / 'crates/erika/Cargo.toml').is_file():
        raise RuntimeError(f'Not a liberika source checkout: {root}')
    return root


if __name__ == '__main__':
    print(liberika_root())
