"""Play video, audio and a Canvas game inside isolated Markdown lists."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-list-media-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        for src, dst in [('示例视频.mp4', 'clip.mp4'), ('示例音频.wav', 'sound.wav'), ('storyboard-shots/02.svg', 'frame.svg')]:
            shutil.copyfile(ROOT / 'MarkdownX-Demo' / src, root / dst)
        cases = []
        for prefix, name in [('./', 'dot'), ('', 'bare')]:
            video = root / f'video-{name}.md'
            video.write_text(f'# Video\n\n- <video controls poster="{prefix}frame.svg">\n  <source src="{prefix}clip.mp4" type="video/mp4">\n  </video>\n', encoding='utf-8')
            audio = root / f'audio-{name}.md'
            audio.write_text(f'# Audio\n\n- <audio controls>\n  <source src="{prefix}sound.wav" type="audio/wav">\n  </audio>\n', encoding='utf-8')
            cases.extend(((video, 'video'), (audio, 'audio')))
        game = root / 'game.md'
        game.write_text("# Game\n\n- <canvas id=\"game\" width=\"300\" height=\"140\"></canvas>\n  <script>\n  const c = document.getElementById('game'); let n = 0;\n\n  const p = document.createElement('p'); c.insertAdjacentElement('afterend', p);\n  function draw() { p.textContent = 'score: ' + n; c.getContext('2d').fillRect(10,10,80,80); }\n  c.onclick = () => { n++; draw(); }; draw();\n  </script>\n", encoding='utf-8')
        cases.append((game, 'game'))
        originals = {p: p.read_bytes() for p in [*(p for p, _ in cases), root / 'clip.mp4', root / 'sound.wav', root / 'frame.svg']}
        steps = root / 'steps.txt'
        steps.write_text('resize 1100 760\nwait 1000\n' + ''.join(f'pick {p}\nopen-file\nwait 8000\ncheck-inline-html {kind}\n' for p, kind in cases) + 'quit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=100, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('inline-html: video PASS') != 2 or logs.count('inline-html: audio PASS') != 2 or logs.count('inline-html: game PASS') != 1:
            relevant = '\n'.join(line for line in (result.stderr + '\n' + logs).splitlines() if any(mark in line.lower() for mark in ('panic', 'inline-html:', 'failed to')))
            raise SystemExit(f'List media UI failed: {result.returncode}\n{relevant[-5000:]}')
        if any(path.read_bytes() != data for path, data in originals.items()):
            raise SystemExit('List media preview changed a source file')
        print('List video, audio and Canvas game, dot and bare paths: PASS')


if __name__ == '__main__':
    main()
