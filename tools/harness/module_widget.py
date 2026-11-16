"""Run external and inline ES modules in an isolated desktop widget."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-module-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        (root / 'storyboard-shots').mkdir()
        names = ('module-game.mjs', 'module-helper.mjs', 'module-label.mjs', 'storyboard-game.css', 'storyboard-palette.css', 'storyboard-shots/01.svg', 'storyboard-shots/02.svg', 'storyboard-data.json')
        for name in names:
            shutil.copyfile(ROOT / 'MarkdownX-Demo' / name, root / name)
        demo_source = (ROOT / 'MarkdownX-Demo/分镜与互动演示.md').read_text(encoding='utf-8')
        begin = demo_source.index('<canvas id="slate"', demo_source.index('### 模块化场记板'))
        end = demo_source.index('</script>', begin) + len('</script>')
        demo = root / 'demo.md'
        demo.write_text('# Demo\n\n' + demo_source[begin:end] + '\n', encoding='utf-8')
        inline = root / 'inline.md'
        inline.write_text("# Inline\n\n<canvas id='slate' width='300' height='140'></canvas>\n"
                          "<link rel='stylesheet' href='./storyboard-game.css'>\n"
                          "<script type='module'>import {mountSlate} from './module-helper.mjs'; "
                          "const {initial} = await import('./module-label.mjs'); mountSlate(document.getElementById('slate'), initial);</script>\n", encoding='utf-8')
        fetch_begin = demo_source.index('<canvas id="data-slate"')
        fetch_end = demo_source.index('</script>', fetch_begin) + len('</script>')
        local_data = root / 'local-data.md'
        local_data.write_text('# Local data\n\n' + demo_source[fetch_begin:fetch_end] + '\n', encoding='utf-8')
        image_begin = demo_source.index('<canvas id="image-slate"')
        image_end = demo_source.index('</script>', image_begin) + len('</script>')
        image_demo = root / 'image-demo.md'
        image_demo.write_text('# Canvas image\n\n' + demo_source[image_begin:image_end] + '\n', encoding='utf-8')
        (root / 'scripts').mkdir()
        external_script = root / 'scripts/game.js'
        external_script.write_text("const sprite=new Image();const ready=new Promise(resolve=>{sprite.onload=resolve});sprite.src='storyboard-shots/01.svg';Promise.all([ready,fetch('storyboard-data.json').then(r=>r.json()),import('../module-label.mjs')]).then(([_,data,labels])=>{let n=data.initial+labels.initial;const c=document.querySelector('canvas'),p=document.createElement('p');c.getContext('2d').drawImage(sprite,0,0,100,100);c.after(p);const draw=()=>p.textContent=data.label+n;c.onclick=()=>{n++;draw()};draw()});", encoding='utf-8')
        external_data = root / 'external-data.md'
        external_data.write_text("# External data\n\n<canvas width='300' height='140'></canvas>\n<script src='./scripts/game.js?cache=2'></script>\n", encoding='utf-8')
        identity_script = root / 'identity.mjs'
        identity_script.write_text('globalThis.moduleRuns=(globalThis.moduleRuns||0)+1;', encoding='utf-8')
        identity = root / 'identity.md'
        identity.write_text("# Identity\n\n<canvas></canvas>\n"
                            "<script type='module' src='./identity.mjs?v=1'></script>\n"
                            "<script type='module' src='./identity.mjs?v=1'></script>\n"
                            "<script type='module' src='./identity.mjs?v=2'></script>\n"
                            "<script type='module' src='./identity.mjs?v=中'></script>\n"
                            "<script type='module' src='./identity.mjs?v=%E4%B8%AD'></script>\n"
                            "<script type='module'>import './identity.mjs?v=中';</script>\n", encoding='utf-8')
        originals = {path: path.read_bytes() for path in (demo, inline, local_data, image_demo, external_data, external_script, identity, identity_script, *(root / name for name in names))}
        steps = root / 'steps.txt'
        steps.write_text(f'wait 1000\npick {demo}\nopen-file\nwait 8000\ncheck-inline-html module\n'
                         f'pick {inline}\nopen-file\nwait 8000\ncheck-inline-html module\n'
                         f'pick {local_data}\nopen-file\nwait 8000\ncheck-inline-html fetch\n'
                         f'pick {image_demo}\nopen-file\nwait 8000\ncheck-inline-html fetch\n'
                         f'pick {external_data}\nopen-file\nwait 8000\ncheck-inline-html fetch\n'
                         f'pick {identity}\nopen-file\nwait 8000\ncheck-inline-html module-identity\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('inline-html: module PASS') != 2 or logs.count('inline-html: fetch PASS') != 3 or 'inline-html: module-identity PASS' not in logs:
            raise SystemExit(f'Module widget desktop test failed: {result.returncode}\n{(result.stderr + logs)[:6000]}')
        if any(path.read_bytes() != content for path, content in originals.items()):
            raise SystemExit('Preview modified a source document or asset')
        print('Module imports, local fetch and canvas images, network isolation and source preservation: PASS')


if __name__ == '__main__':
    main()
