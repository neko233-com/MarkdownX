"""Verify HTML images in a Markdown document and table in the desktop editor."""
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
    parser.add_argument('-Reference', action='store_true')
    args = parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-html-images-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        (root / 'shots').mkdir()
        for name in ('01.svg', '02.svg'):
            shutil.copyfile(ROOT / 'MarkdownX-Demo/storyboard-shots' / name, root / 'shots' / name)
        source = "# Story\n\n<img\n src='./shots/01.svg?v=1' alt='cover' width='120'>\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Before <img src=shots/02.svg?cache=2 alt='detail'> after |\n"
        if args.Reference:
            source = "# Story\n\n<img\n src='./shots/01.svg?v=1' alt='cover' width='120'>\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Before ![detail][shot] after |\n\n[shot]: shots/02.svg?cache=2\n"
        document = root / 'story.md'
        document.write_text(source, encoding='utf-8')
        script = root / 'steps.txt'
        script.write_text(f'resize 1100 760\nwait 1000\npick {document}\nopen-file\nwait 6000\ncheck-html-images\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=25, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'html-images: desktop PASS' not in logs or 'ERROR gpui::asset_cache: Failed to load asset' in logs:
            raise SystemExit(f'HTML image desktop test failed: {result.returncode}\n{result.stderr[:2500]}\n{logs[-2500:]}')
        if document.read_text(encoding='utf-8') != source:
            raise SystemExit('Opening HTML images changed the source Markdown')
        print('Desktop standalone and table ' + ('reference' if args.Reference else 'HTML') + ' images, relative paths and source preservation: PASS')


if __name__ == '__main__':
    main()
