"""Edit text alongside styled content and reference images in a nested list."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-nested-list-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        source = root / 'case.md'
        original = '# Case\r\n\r\n3。 Parent\r\n   - Before **closeup** ![frame][shot] and <img src="other.svg" alt="Detail" width="40"> after\r\n\r\nAfter\r\n\r\n[shot]: <./shot%20one.svg> "title"\r\n'
        source.write_bytes(original.encode('utf-8'))
        image = root / 'shot one.svg'
        image.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>', encoding='utf-8')
        (root / 'other.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>', encoding='utf-8')
        steps = root / 'steps.txt'
        steps.write_text(f'resize 1100 760\nwait 1000\npick {source}\nopen-file\nwait 2000\nedit-nested-inline-image\nwait 3500\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'nested-list-edit: PASS' not in logs:
            raise SystemExit(f'Nested list UI failed: {result.returncode}\n{(result.stderr + logs)[-5000:]}')
        expected = original.replace('- Before', '- New Before').encode('utf-8')
        if source.read_bytes() != expected:
            raise SystemExit(f'Nested list save did not preserve Markdown: {source.read_bytes()!r}')
        if image.read_text(encoding='utf-8') != '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>':
            raise SystemExit('Nested list editing changed the image')
        print('Nested rich text and source-preserving autosave: PASS')


if __name__ == '__main__':
    main()
