"""Verify a Markdown title's image and equation in an isolated desktop library."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-rich-title-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        (root / 'temp').mkdir()
        image = root / 'frame.svg'
        image.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="160" height="100"><rect width="160" height="100" fill="red"/></svg>', encoding='utf-8')
        source = root / 'rich.md'
        original = '# Story ![frame](./frame.svg) $\\alpha$\r\n\r\nBody\r\n'.encode('utf-8')
        source.write_bytes(original)
        steps = root / 'steps.txt'
        steps.write_text(f'resize 1100 760\nwait 1000\npick {source}\nopen-file\nwait 4000\ncheck-rich-title\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root),
                   TEMP=str(root / 'temp'), TMP=str(root / 'temp'))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=50, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'rich-title: desktop PASS' not in logs:
            raise SystemExit(f'Rich title desktop test failed: {result.returncode}\n{(result.stderr + logs)[-5000:]}')
        if source.read_bytes() != original:
            raise SystemExit('Opening the rich title modified the Markdown source')
        print('Rich title image, equation and source preservation: PASS')


if __name__ == '__main__':
    main()
