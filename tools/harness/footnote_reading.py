"""Exercise Markdown reading mode, footnote navigation and safe autosave."""
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
        raise SystemExit('Build the Windows app with --features devtools first.')
    with tempfile.TemporaryDirectory(prefix='markdownx-footnote-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        source = root / 'paper.md'
        original = ('# Paper\r\n\r\nIntro[^a].\r\n\r\n| Shot | Frame |\r\n| --- | --- |\r\n'
                    '| 01[^a] | ![one](./shot.svg) ![two](shot2.svg) |\r\n\r\n'
                    '## Section[^a]\r\n\r\n[Jump](#sectiona)\r\n\r\n'
                    '[^a]: **Important** note. ![detail](./shot.svg)\r\n')
        source.write_bytes(original.encode('utf-8'))
        image = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"/>'
        (root / 'shot.svg').write_text(image, encoding='utf-8')
        (root / 'shot2.svg').write_text(image, encoding='utf-8')
        steps = root / 'steps.txt'
        steps.write_text(f'resize 1100 760\nwait 1000\npick {source}\nopen-file\nwait 2000\n'
                         'footnote-read-start\nwait 3500\nfootnote-read-check\nwait 1000\nfootnote-read-guard\nwait 1000\n'
                         'footnote-read-back\nwait 1000\nfootnote-heading-check\nwait 1000\nfootnote-heading-back\n'
                         'footnote-read-end\nwait 3500\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        expected_logs = ('footnote-reading: preview PASS', 'relative-link: heading PASS', 'footnote-reading: back PASS',
                         'footnote-reading: heading alias PASS', 'footnote-reading: restore PASS')
        if result.returncode or any(item not in logs for item in expected_logs):
            details = result.stderr + logs
            raise SystemExit(f'Footnote reading UI failed: {result.returncode}\n{details[:5000]}\n{details[-1000:]}')
        expected = original.replace('Intro[^a]', 'After Before Intro[^a]').encode('utf-8')
        if source.read_bytes() != expected:
            raise SystemExit(f'Reading mode changed the original Markdown: {source.read_bytes()!r}')
        conflict = root / 'conflict.md'
        conflict.write_bytes(original.encode('utf-8'))
        steps.write_text(f'resize 1100 760\nwait 1000\npick {conflict}\nopen-file\nwait 2000\n'
                         'footnote-conflict-start\nwait 3500\nfootnote-conflict-check\nwait 1000\nquit\n', encoding='utf-8')
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'footnote-reading: conflict PASS' not in logs:
            details = result.stderr + logs
            raise SystemExit(f'Footnote reading conflict test failed: {result.returncode}\n{details[:5000]}\n{details[-1000:]}')
        if conflict.read_text(encoding='utf-8') != '# External\n\nChanged by another app.\n':
            raise SystemExit('Reading mode overwrote an external change')
    print('Markdown footnote reading, navigation, autosave and conflict protection: PASS')


if __name__ == '__main__':
    main()
