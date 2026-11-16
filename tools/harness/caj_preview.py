"""Check a separately supplied CAJ in an isolated, read-only desktop preview."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('sample', type=Path)
    parser.add_argument('-Quiet', action='store_true')
    args = parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    before = hashlib.sha256(args.sample.read_bytes()).digest()
    with tempfile.TemporaryDirectory(prefix='markdownx-caj-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        sample = root / 'sample.caj'
        shutil.copyfile(args.sample, sample)
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {sample}\nopen-file\nwait 5000\ncheck-media-preview caj\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=30, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'media-preview: caj PASS' not in logs:
            raise SystemExit(f'CAJ preview failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-3000:]}')
        if hashlib.sha256(sample.read_bytes()).digest() != before or hashlib.sha256(args.sample.read_bytes()).digest() != before:
            raise SystemExit('CAJ preview unexpectedly changed the source')
        print('Read-only CAJ desktop preview: PASS')


if __name__ == '__main__':
    main()
