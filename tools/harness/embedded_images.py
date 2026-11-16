"""Open embedded PNG/SVG and a table image in an isolated desktop library."""
import argparse
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from image_host import PNG

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    png = 'data:image/png;base64,' + base64.b64encode(PNG).decode()
    svg = 'data:image/svg+xml;base64,' + base64.b64encode(b'<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="100" height="60" fill="red"/></svg>').decode()
    with tempfile.TemporaryDirectory(prefix='markdownx-embedded-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'library/preferences.json').write_text(json.dumps({'remote_images': False, 'auto_update': False}), encoding='utf-8')
        (root / 'documents').mkdir()
        (root / 'temp').mkdir()
        source = root / 'embedded.md'
        original = f'# Embedded\n\n![PNG]({png})\n\n![SVG]({svg})\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Before ![frame]({png}) after |\n'
        source.write_text(original, encoding='utf-8')
        script = root / 'steps.txt'
        script.write_text(f'resize 1100 760\nwait 1000\npick {source}\nopen-file\nwait 3500\ncheck-embedded-images\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root),
                   TEMP=str(root / 'temp'), TMP=str(root / 'temp'))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'embedded-images: desktop PASS' not in logs:
            raise SystemExit(f'Embedded image desktop test failed: {result.returncode}\n{result.stderr[:4000]}\n{logs[-4000:]}')
        if source.read_text(encoding='utf-8') != original:
            raise SystemExit('Opening embedded images changed the Markdown source')
        if list((root / 'temp').glob('markdownx-images-*')):
            raise SystemExit('Embedded image cache was not cleaned up')
        print('Embedded PNG/SVG, table image and source preservation: PASS')


if __name__ == '__main__':
    main()
