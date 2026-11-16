"""Verify an embedded model and mouse rotation in an isolated desktop window."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import struct

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true', help='Keep the test window offscreen')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-inline-model-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例模型.obj', root / 'model.obj')
        original = '# Model\n\n<model-viewer src="./model.obj"></model-viewer>\n'
        document = root / 'model.md'
        document.write_text(original, encoding='utf-8')
        (root / 'mesh data.bin').write_bytes(struct.pack('<9f', 0, 0, 0, 1, 0, 0, 0, 1, 0))
        gltf = {'asset': {'version': '2.0'}, 'buffers': [{'uri': './mesh%20data.bin', 'byteLength': 36}],
                'bufferViews': [{'buffer': 0, 'byteOffset': 0, 'byteLength': 36}],
                'accessors': [{'bufferView': 0, 'componentType': 5126, 'count': 3, 'type': 'VEC3'}],
                'meshes': [{'primitives': [{'attributes': {'POSITION': 0}}]}]}
        (root / 'model.gltf').write_text(json.dumps(gltf), encoding='utf-8')
        second = root / 'gltf.md'
        second.write_text('# glTF\n\n<model-viewer src="./model.gltf"></model-viewer>\n', encoding='utf-8')
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {document}\nopen-file\nwait 2500\ncheck-inline-model loaded\ndrag-inline-model\nwait 2500\ncheck-inline-model rotated\nzoom-inline-model\nwait 2500\ncheck-inline-model zoomed\nreset-inline-model\nwait 2500\ncheck-inline-model reset\npick {second}\nopen-file\nwait 2500\ncheck-inline-model loaded\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=60, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('inline-model: loaded PASS') < 2 or any(f'inline-model: {phase} PASS' not in logs for phase in ('rotated', 'zoomed', 'reset')):
            raise SystemExit(f'Inline model desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-5000:]}')
        if document.read_text(encoding='utf-8') != original:
            raise SystemExit('Preview or rotation unexpectedly changed the source Markdown')
        if json.loads((root / 'model.gltf').read_text(encoding='utf-8')) != gltf:
            raise SystemExit('Preview unexpectedly changed the source glTF')
        print('Inline model desktop load, drag, zoom and reset: PASS')


if __name__ == '__main__':
    main()
