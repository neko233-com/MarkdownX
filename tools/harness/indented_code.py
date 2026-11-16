"""Verify indented code and real relative images in an isolated desktop session."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-indented-code-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        source = root / 'paper.md'
        original = '# Paper\n\n    ![literal](missing.png)\n    second\n\n![real](./real.svg)\n\n[^n]: note\n\n    ![footnote](./real.svg)\n'
        source.write_text(original, encoding='utf-8')
        image = root / 'real.svg'
        image.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>', encoding='utf-8')
        steps = root / 'steps.txt'
        steps.write_text(f'resize 1100 760\nwait 1000\npick {source}\nopen-file\nwait 2000\ncheck-indented-code\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'indented-code: PASS' not in logs:
            raise SystemExit(f'Indented code UI failed: {result.returncode}\n{(result.stderr + logs)[-5000:]}')
        if source.read_text(encoding='utf-8') != original or image.read_text(encoding='utf-8') != '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>':
            raise SystemExit('Desktop preview changed a source file')
        print('Desktop indented code and real relative images: PASS')


if __name__ == '__main__':
    main()
