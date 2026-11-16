"""Exercise sandboxed HTML and video in an isolated Windows desktop window."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true', help='Keep the test window offscreen')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / 'markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    hits = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            hits.append(self.path)
            self.send_response(200)
            self.end_headers()
        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='markdownx-inline-html-') as tmp:
            root = Path(tmp)
            shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
            (root / 'documents').mkdir()
            video = root / 'clip.mp4'
            shutil.copyfile(ROOT / 'MarkdownX-Demo/示例视频.mp4', video)
            shutil.copyfile(ROOT / 'MarkdownX-Demo/storyboard-shots/02.svg', root / 'frame.svg')
            game = root / 'game.md'
            url = f'http://127.0.0.1:{server.server_port}/unexpected'
            game_source = f'''# Game\n\n<canvas id="game" width="300" height="140"></canvas>\n<script>\nconst c = document.getElementById('game'); let n = 0;\nconst p = document.createElement('p'); c.insertAdjacentElement('afterend', p);\nfunction draw() {{ p.textContent = 'score: ' + n; c.getContext('2d').fillRect(10,10,80,80); }}\nc.onclick = () => {{ n++; draw(); }}; draw();\nfetch('{url}').catch(() => {{}});\nwindow.navTarget = '{url}';\n</script>\n'''
            game_source += ''.join(f'\nParagraph {i}.\n' for i in range(100))
            game.write_text(game_source, encoding='utf-8')
            movie = root / 'video.md'
            movie_source = '# Video\n\n<video controls src="./clip.mp4" poster="./frame.svg"></video>\n'
            movie.write_text(movie_source, encoding='utf-8')
            movie_bare = root / 'video-bare.md'
            movie_bare_source = '# Video\n\n<video controls src="clip.mp4" poster="frame.svg"></video>\n'
            movie_bare.write_text(movie_bare_source, encoding='utf-8')
            demo_text = (ROOT / 'MarkdownX-Demo/分镜与互动演示.md').read_text(encoding='utf-8')
            demo_start = demo_text.index('<canvas id="game"')
            demo_end = demo_text.index('</script>', demo_start) + len('</script>')
            demo = root / 'demo.md'
            demo_source = '# Demo\n\n' + demo_text[demo_start:demo_end] + '\n'
            demo.write_text(demo_source, encoding='utf-8')
            external_script = "const c=document.querySelector('canvas'); let n=0; const p=document.createElement('p'); c.after(p); function draw(){p.textContent='score: '+n;} c.onclick=()=>{n++;draw();}; draw();"
            (root / 'game.js').write_text(external_script, encoding='utf-8')
            (root / 'game.css').write_text('canvas { background: rgb(18, 52, 86); }', encoding='utf-8')
            external = root / 'external.md'
            external.write_text('# Game\n\n<canvas id="game" width="300" height="140"></canvas>\n<link rel="stylesheet" href="./game.css">\n<script src="game.js"></script>\n', encoding='utf-8')
            external_bare = root / 'external-bare.md'
            external_bare.write_text('# Game\n\n<canvas id="game" width="300" height="140"></canvas>\n<link rel="stylesheet" href="game.css">\n<script src="./game.js"></script>\n', encoding='utf-8')
            (root / 'storyboard-shots').mkdir()
            shutil.copyfile(ROOT / 'MarkdownX-Demo/storyboard-shots/02.svg', root / 'storyboard-shots/02.svg')
            for name in ('storyboard-game.js', 'storyboard-game.css', 'storyboard-palette.css'):
                shutil.copyfile(ROOT / 'MarkdownX-Demo' / name, root / name)
            demo_external = root / 'demo-external.md'
            start = demo_text.index('<canvas id="slate"')
            end = demo_text.index('</script>', start) + len('</script>')
            demo_external.write_text('# Demo\n\n' + demo_text[start:end] + '\n', encoding='utf-8')
            originals = {path: path.read_bytes() for path in (game, movie, movie_bare, demo, external, external_bare, demo_external,
                         root / 'game.js', root / 'game.css', root / 'storyboard-game.js', root / 'storyboard-game.css', root / 'storyboard-palette.css', root / 'storyboard-shots/02.svg', video, root / 'frame.svg')}
            script = root / 'steps.txt'
            script.write_text(f'wait 1000\npick {game}\nopen-file\nwait 8000\ncheck-inline-html-state shown\nresize 1100 760\nwait 700\ncheck-inline-html-state shown\nwheel 700 400 120\nwait 500\ncheck-inline-html-state shown\nwheel 700 400 -120\nwait 500\ninline-html-menu open\nwait 500\ncheck-inline-html-state hidden\ninline-html-menu close\nwait 500\ncheck-inline-html-state shown\npick {root / "frame.svg"}\nopen-file\nwait 1000\ncheck-media-preview image\ncheck-inline-html-state hidden\nkey escape\nwait 1000\ncheck-inline-html-state shown\ncheck-inline-html game\ncheck-inline-html navigate\nwait 1500\ncheck-inline-html game-after-navigation\nwheel 700 400 5000\nwait 1000\ncheck-inline-html-state offscreen\nwheel 700 400 -5000\nwait 6000\ncheck-inline-html-state shown\npick {movie}\nopen-file\nwait 8000\ncheck-inline-html video\npick {movie_bare}\nopen-file\nwait 8000\ncheck-inline-html video\npick {demo}\nopen-file\nwait 8000\ncheck-inline-html demo\npick {external}\nopen-file\nwait 8000\ncheck-inline-html external\npick {external_bare}\nopen-file\nwait 8000\ncheck-inline-html external\npick {demo_external}\nopen-file\nwait 8000\ncheck-inline-html demo-external\nquit\n', encoding='utf-8')
            env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                       MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
            env.pop('FDL_NOPARK', None)
            result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                    timeout=110, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
            logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
            if result.returncode or any(f'inline-html: {phase} PASS' not in logs for phase in ('game', 'navigate', 'game-after-navigation', 'video', 'demo', 'external', 'demo-external')):
                raise SystemExit(f'Inline HTML desktop test failed: {result.returncode}\n{result.stderr[:8000]}\n{logs[-7000:]}')
            if any(f'inline-html-state: {phase} PASS' not in logs for phase in ('shown', 'hidden', 'offscreen')):
                raise SystemExit(f'Inline HTML visibility test failed:\n{logs[-7000:]}')
            if logs.count('inline-html: video PASS') != 2:
                raise SystemExit('Relative video paths did not both pass')
            if logs.count('inline-html: external PASS') != 2:
                raise SystemExit('Relative script and stylesheet paths did not both pass')
            if hits:
                raise SystemExit(f'Untrusted widget made an external request: {hits}')
            if any(path.read_bytes() != data for path, data in originals.items()):
                raise SystemExit('Interactive preview changed a source document or asset')
            print('Inline Canvas game, sandbox and video playback: PASS')
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == '__main__':
    main()
