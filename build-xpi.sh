#!/bin/sh
set -eu

python3 - <<'PY'
import json
import pathlib
import zipfile

manifest = pathlib.Path('manifest.json')
version = json.loads(manifest.read_text(encoding='utf-8'))['version']
out_dir = pathlib.Path('dist')
out_dir.mkdir(exist_ok=True)
out_file = out_dir / f'yeet-and-yoink-firefox-bridge-{version}.xpi'
with zipfile.ZipFile(out_file, 'w', compression=zipfile.ZIP_DEFLATED) as archive:
    archive.write('manifest.json')
    archive.write('background.js')
print(f'Wrote {out_file}')
PY
