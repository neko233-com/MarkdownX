"""Verify inaccessible HTML resources show errors in an isolated desktop."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-widget-boundary-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        workspace = root / 'workspace'
        workspace.mkdir()
        outside = root / 'outside.js'
        outside.write_text('document.body.textContent="outside";', encoding='utf-8')
        denied = workspace / 'denied.md'
        denied.write_text('# Outside\n\n<canvas></canvas><script src="../outside.js"></script>\n', encoding='utf-8')
        missing = workspace / 'missing.md'
        missing.write_text('# Missing\n\n<video src="./missing.mp4"></video>\n', encoding='utf-8')
        original = {path: path.read_bytes() for path in (denied, missing, outside)}
        steps = root / 'steps.txt'
        steps.write_text(f'wait 1000\npick {denied}\nopen-file\nwait 5000\ncheck-inline-html-state denied\npick {missing}\nopen-file\nwait 5000\ncheck-inline-html-state missing\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=40, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'inline-html-state: {phase} PASS' not in logs for phase in ('denied', 'missing')):
            raise SystemExit(f'HTML resource boundary test failed: {result.returncode}\n{result.stderr[:3000]}\n{logs[-4000:]}')
        if any(path.read_bytes() != data for path, data in original.items()):
            raise SystemExit('HTML resource failure changed a source file')
        print('Desktop HTML resource boundary and missing-file errors: PASS')


if __name__ == '__main__':
    main()
