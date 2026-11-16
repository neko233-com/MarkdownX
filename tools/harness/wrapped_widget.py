"""Run a wrapped Canvas with chapter-relative assets in an isolated desktop window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-wrapped-widget-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        document = root / 'game.md'
        source = ("# Game\r\n\r\n<div class='game'>\r\n<!-- </div> -->\r\n"
                  "<div><canvas id='game' width='300' height='140'></canvas></div>\r\n"
                  "</div>\r\n<link rel='stylesheet' href='./game.css'>\r\n<script src='game.js'></script>\r\n")
        document.write_bytes(source.encode('utf-8'))
        script = root / 'game.js'
        script.write_text("const c=document.querySelector('canvas'); let n=0; const p=document.createElement('p'); c.after(p); function draw(){p.textContent='score: '+n;} c.onclick=()=>{n++;draw();}; draw();", encoding='utf-8')
        css = root / 'game.css'
        css.write_text('canvas { background: rgb(18, 52, 86); }', encoding='utf-8')
        (root / 'storyboard-shots').mkdir()
        assets = [root / 'storyboard-game.js', root / 'storyboard-game.css', root / 'storyboard-palette.css', root / 'storyboard-shots/01.svg', root / 'storyboard-shots/02.svg']
        for path in assets:
            shutil.copyfile(ROOT / 'MarkdownX-Demo' / path.relative_to(root), path)
        demo_source = (ROOT / 'MarkdownX-Demo/分镜与互动演示.md').read_text(encoding='utf-8')
        begin = demo_source.index('<div class="storyboard-wrapper">')
        end = demo_source.index('</script>', begin) + len('</script>')
        demo = root / 'demo.md'
        demo.write_text('# Demo\n\n' + demo_source[begin:end] + '\n', encoding='utf-8')
        originals = {path: path.read_bytes() for path in (document, script, css, demo, *assets)}
        steps = root / 'steps.txt'
        steps.write_text(f'wait 1000\npick {document}\nopen-file\nwait 8000\ncheck-inline-html external\npick {demo}\nopen-file\nwait 8000\ncheck-inline-html demo-external\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=60, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'inline-html: {phase} PASS' not in logs for phase in ('external', 'demo-external')):
            raise SystemExit(f'Wrapped widget desktop test failed: {result.returncode}\n{(result.stderr + logs)[-5000:]}')
        if any(path.read_bytes() != content for path, content in originals.items()):
            raise SystemExit('Preview modified a source document or asset')
        print('Wrapped Canvas, chapter-relative JS/CSS, responsive image and source preservation: PASS')


if __name__ == '__main__':
    main()
