"""Exercise video and Canvas inside Markdown tables in an isolated desktop."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-table-media-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例视频.mp4', root / 'clip#take.mp4')
        shutil.copyfile(ROOT / 'MarkdownX-Demo/storyboard-shots/02.svg', root / 'frame.svg')
        movie = root / 'movie.md'
        movie.write_text('# Video\n\n| Shot | Video |\n| --- | --- |\n| 1 | <video controls src="./clip%23take.mp4?v=1#t=0,5" poster="frame.svg?cache=2"></video> |\n', encoding='utf-8')
        game = root / 'game.md'
        game.write_text('# Game\n\n| Shot | Game |\n| --- | --- |\n| 1 | <canvas id="game" width="300" height="140"></canvas><script>const c=document.getElementById("game"); let n=0; const p=document.createElement("p"); c.after(p); function draw(){p.textContent="score: "+n;} c.onclick=()=>{n++;draw();}; draw();</script> |\n', encoding='utf-8')
        originals = {path: path.read_bytes() for path in (movie, game, root / 'clip#take.mp4', root / 'frame.svg')}
        steps = root / 'steps.txt'
        steps.write_text(f'resize 1100 760\nwait 1000\npick {movie}\nopen-file\nwait 8000\ncheck-inline-html video\npick {game}\nopen-file\nwait 8000\ncheck-inline-html game\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=45, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'inline-html: {phase} PASS' not in logs for phase in ('video', 'game')):
            raise SystemExit(f'Table media desktop test failed: {result.returncode}\n{result.stderr[:3000]}\n{logs[:5000]}')
        if any(path.read_bytes() != data for path, data in originals.items()):
            raise SystemExit('Table media preview changed a source document or asset')
        print('Markdown table video and Canvas: PASS')


if __name__ == '__main__':
    main()
