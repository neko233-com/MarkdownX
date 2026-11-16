"""Configure a local image host, restart, and upload from an isolated workspace."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import threading
from image_host import PNG

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools first.')
    requests = []
    release = threading.Event()

    class MockPicGo(BaseHTTPRequestHandler):
        protocol_version = 'HTTP/1.1'
        def do_POST(self):
            request = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
            requests.append((self.path, self.headers.get('Authorization'), Path(request['list'][0]).read_bytes()))
            data = json.dumps({'success': True, 'result': ['https://example.test/uploaded.png']}).encode()
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
            self.wfile.flush()
            release.wait(4)
            self.close_connection = True
        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), MockPicGo)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='markdownx-picgo-settings-') as tmp:
            root = Path(tmp)
            shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
            (root / 'library/preferences.json').write_text(json.dumps({'auto_update': False, 'remote_images': False}), encoding='utf-8')
            (root / 'documents').mkdir()
            image = root / 'frame.png'
            image.write_bytes(PNG)
            document = root / 'story.md'
            original = '# Story\n\n![frame](./frame.png)\n'
            document.write_text(original, encoding='utf-8')
            endpoint = f'http://127.0.0.1:{server.server_port}/upload?profile=demo'
            steps = root / 'steps.txt'
            env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                       MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root),
                       PICGO_SERVER_SECRET='test-secret')
            env.pop('MARKDOWNX_PICGO_ENDPOINT', None)
            env.pop('FDL_NOPARK', None)
            default = 'http://127.0.0.1:36677/upload'
            steps.write_text(f'wait 1000\npicgo-edit http://example.test/upload\nkey enter\nwait 500\npicgo-check {default}\n'
                             f'picgo-edit {endpoint}\nkey enter\nwait 500\npicgo-check {endpoint}\nquit\n', encoding='utf-8')
            first = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                   timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
            if first.returncode:
                raise SystemExit(f'PicGo settings failed: {first.returncode}\n{first.stderr[-4000:]}')
            prefs = json.loads((root / 'library/preferences.json').read_text(encoding='utf-8'))
            if prefs.get('picgo_endpoint') != endpoint:
                raise SystemExit('PicGo endpoint did not persist')
            steps.write_text(f'wait 1000\npicgo-check {endpoint}\npick {document}\nopen-file\nwait 2000\n'
                             'image-host local\nimage-host upload\nwait 3000\nimage-host remote\nquit\n', encoding='utf-8')
            second = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                    timeout=40, creationflags=subprocess.CREATE_NO_WINDOW)
            release.set()
            logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
            if second.returncode or 'image-host: remote PASS' not in logs:
                raise SystemExit(f'Persisted PicGo upload failed: {second.returncode}\n{(second.stderr + logs)[:6000]}')
            if requests != [('/upload?profile=demo', 'Bearer test-secret', PNG)]:
                raise SystemExit(f'Unexpected local upload request: {requests!r}')
            if 'https://example.test/uploaded.png' not in document.read_text(encoding='utf-8') or image.read_bytes() != PNG:
                raise SystemExit('Upload did not update the link safely')
            print('PicGo settings validation, restart, keep-alive upload and source asset preservation: PASS')
    finally:
        release.set()
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()


if __name__ == '__main__':
    main()
