"""Verify an interactive game's local CSS imports and image in the desktop sandbox."""
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
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-inline-css-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        (root / 'storyboard-shots').mkdir()
        for name in ('storyboard-game.js', 'storyboard-game.css', 'storyboard-palette.css', 'storyboard-shots/01.svg', 'storyboard-shots/02.svg'):
            shutil.copyfile(ROOT / 'MarkdownX-Demo' / name, root / name)
        source = (ROOT / 'MarkdownX-Demo/分镜与互动演示.md').read_text(encoding='utf-8')
        start = source.index('<canvas id="slate"')
        end = source.index('</script>', start) + len('</script>')
        document = root / 'demo.md'
        document.write_text('# Demo\n\n' + source[start:end] + '\n', encoding='utf-8')
        start = source.index('<canvas id="inline-scene"')
        end = source.index('</script>', start) + len('</script>')
        inline_document = root / 'inline.md'
        inline_document.write_text('# Inline\n\n' + source[start:end] + '\n', encoding='utf-8')
        files = [document, inline_document, root / 'storyboard-game.js', root / 'storyboard-game.css', root / 'storyboard-palette.css', root / 'storyboard-shots/01.svg', root / 'storyboard-shots/02.svg']
        originals = {path: path.read_bytes() for path in files}
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {document}\nopen-file\nwait 8000\ncheck-inline-html demo-external\npick {inline_document}\nopen-file\nwait 8000\ncheck-inline-html inline-style\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=50, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'inline-html: {phase} PASS' not in logs for phase in ('demo-external', 'inline-style')):
            raise SystemExit(f'CSS desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-4000:]}')
        if any(path.read_bytes() != content for path, content in originals.items()):
            raise SystemExit('Preview changed a source document or asset')
        print('Desktop inline and external CSS images, imported stylesheet and Canvas interaction: PASS')


if __name__ == '__main__':
    main()
