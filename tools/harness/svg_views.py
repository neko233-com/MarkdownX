"""Exercise relative SVG views in the real Windows editor and table layout."""
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
    parser.add_argument('-Target', action='store_true')
    args = parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-svg-views-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        (root / 'shots').mkdir()
        svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50">'
               '<view id="right" viewBox="50 0 50 50"/><rect width="50" height="50" fill="red"/>'
               '<rect x="50" width="50" height="50" fill="blue"/></svg>')
        (root / 'shots' / 'frame#take.svg').write_text(svg, encoding='utf-8')
        source = ('# Story\n\n![view](./shots/frame%23take.svg?v=1#right)\n\n'
                  '| Shot | Frame |\n| --- | --- |\n| 1 | Before ![view](<shots/frame%23take.svg#svgView(viewBox(50,0,50,50))>) after |\n')
        if args.Target:
            svg = ('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50">'
                   '<style>rect {fill:red} :target {fill:blue}</style>'
                   '<rect id="frame" width="100" height="50"/></svg>')
            (root / 'shots' / 'frame#take.svg').write_text(svg, encoding='utf-8')
            source = ('# Story\n\n![view](./shots/frame%23take.svg?v=1#frame)\n\n'
                      '| Shot | Frame |\n| --- | --- |\n| 1 | Before ![view](shots/frame%23take.svg#frame) after |\n')
        document = root / 'story.md'
        document.write_text(source, encoding='utf-8')
        script = root / 'steps.txt'
        script.write_text(f'resize 1100 760\nwait 1000\npick {document}\nopen-file\nwait 6000\ncheck-svg-views\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=25, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'svg-views: desktop PASS' not in logs or 'ERROR gpui::asset_cache: Failed to load asset' in logs:
            raise SystemExit(f'SVG view desktop test failed: {result.returncode}\n{result.stderr[:2500]}\n{logs[-2500:]}')
        if document.read_text(encoding='utf-8') != source or (root / 'shots' / 'frame#take.svg').read_text(encoding='utf-8') != svg:
            raise SystemExit('Opening SVG views changed the source files')
        print('Desktop SVG views, table images and source preservation: PASS')


if __name__ == '__main__':
    main()
