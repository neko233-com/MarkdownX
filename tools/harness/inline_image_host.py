"""Upload mixed text images through the real desktop toolbar using a local mock."""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
import time

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    requests = []

    class PicGo(BaseHTTPRequestHandler):
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append(body)
            if len(requests) == 5:
                time.sleep(2)
            status = 500 if len(requests) == 1 else 200
            data = json.dumps({'success': status == 200, 'result': ['https://example.invalid/uploaded.svg']}).encode()
            self.send_response(status)
            self.send_header('Content-Type', 'application/json')
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        def log_message(self, *_):
            pass

    server = HTTPServer(('127.0.0.1', 0), PicGo)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='markdownx-inline-image-host-ui-') as tmp:
            root = Path(tmp)
            shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
            (root / 'library/preferences.json').write_text(json.dumps({'remote_images': False, 'auto_update': False}), encoding='utf-8')
            (root / 'documents').mkdir()
            workspace = root / 'workspace'
            (workspace / 'shots').mkdir(parents=True)
            image = workspace / 'shots/frame.svg'
            image.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="100" height="60" fill="red"/></svg>', encoding='utf-8')
            original_image = image.read_bytes()
            plain = workspace / 'plain.md'
            plain.write_text('# Plain\n\nBefore ![frame](./shots/frame.svg "Keep title") after\n', encoding='utf-8')
            table = workspace / 'table.md'
            table.write_text("# Table\n\n| Shot | Frame |\n| --- | --- |\n| 1 | Before <img src='shots/frame.svg' alt='frame' data-note='keep'> after |\n", encoding='utf-8')
            reference = workspace / 'reference.md'
            reference.write_text('# Reference\n\nBefore ![frame][shot] after\n\n[shot]: ./shots/frame.svg "Reference title"\n', encoding='utf-8')
            stale = workspace / 'stale.md'
            stale.write_text('# Stale\n\nBefore ![frame](./shots/frame.svg) after\n', encoding='utf-8')
            script = root / 'steps.txt'
            script.write_text(f'''resize 1100 760
wait 1000
pick {plain}
open-file
wait 2200
click-inline-image
wait 500
click-inline-upload
wait 2000
click-inline-upload
wait 2500
pick {table}
open-file
wait 2200
click-inline-image
wait 500
click-inline-upload
wait 2500
pick {reference}
open-file
wait 2200
click-inline-image
wait 500
click-inline-upload
wait 2500
pick {stale}
open-file
wait 2200
click-inline-image
wait 500
click-inline-upload
wait 300
type EDIT
wait 3500
quit
''', encoding='utf-8')
            env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                       MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root),
                       MARKDOWNX_PICGO_ENDPOINT=f'http://127.0.0.1:{server.server_port}/upload')
            env.pop('FDL_NOPARK', None)
            result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                    timeout=65, creationflags=subprocess.CREATE_NO_WINDOW)
            logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
            if result.returncode:
                relevant = '\n'.join(line for line in (result.stderr + '\n' + logs).splitlines() if 'PANIC' in line or 'panicked at' in line or 'inline image' in line or 'inline toolbar' in line)
                raise SystemExit(f'Inline image toolbar test failed: {result.returncode}; requests={len(requests)}\n{relevant}\n{result.stderr[:2500]}')
            if len(requests) != 5 or any(len(body.get('list', [])) != 1 or Path(body['list'][0]) != image for body in requests):
                raise SystemExit(f'Unexpected upload requests: {requests!r}')
            if 'Before ![frame](https://example.invalid/uploaded.svg "Keep title") after' not in plain.read_text(encoding='utf-8'):
                raise SystemExit('Plain image upload did not preserve its title and surrounding text')
            if "Before <img src='https://example.invalid/uploaded.svg' alt='frame' data-note='keep'> after" not in table.read_text(encoding='utf-8'):
                raise SystemExit('Table image upload did not preserve HTML attributes')
            if 'Before ![frame](https://example.invalid/uploaded.svg "Reference title") after' not in reference.read_text(encoding='utf-8'):
                raise SystemExit('Reference image upload did not preserve its title')
            if 'EDIT' not in stale.read_text(encoding='utf-8') or 'https://example.invalid/' in stale.read_text(encoding='utf-8'):
                raise SystemExit('An upload overwrote a concurrent edit')
            if image.read_bytes() != original_image:
                raise SystemExit('Uploading changed the local image')
            print('Inline image toolbar, retry, table/reference source and concurrent edits: PASS')
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


if __name__ == '__main__':
    main()
