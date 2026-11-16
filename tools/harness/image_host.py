"""Exercise workspace image storage and an explicit upload to a local PicGo mock."""
import argparse
from http.server import BaseHTTPRequestHandler, HTTPServer
import json
import os
from pathlib import Path
import shutil
import subprocess
import struct
import tempfile
import threading
import zlib

ROOT = Path(__file__).resolve().parents[2]
def png_chunk(kind, data):
    return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))


PNG = (b'\x89PNG\r\n\x1a\n' + png_chunk(b'IHDR', struct.pack('>IIBBBBB', 1, 1, 8, 6, 0, 0, 0))
       + png_chunk(b'IDAT', zlib.compress(b'\0\xff\0\0\xff')) + png_chunk(b'IEND', b''))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-image-host-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'library' / 'preferences.json').write_text(json.dumps({'remote_images': False, 'auto_update': False}), encoding='utf-8')
        (root / 'documents').mkdir()
        workspace = root / 'workspace'
        workspace.mkdir()
        document = workspace / 'story.md'
        document.write_text('# Story\n\nNotes\n', encoding='utf-8')
        image = root / 'original.png'
        image.write_bytes(PNG)
        svg = root / 'storyboard.svg'
        svg.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>', encoding='utf-8')
        (workspace / 'shots').mkdir()
        html_image = workspace / 'shots' / 'frame.png'
        html_image.write_bytes(PNG)
        html_document = workspace / 'html-story.md'
        html_source = "# HTML\n\n<img alt='cover'\n width='120' src='./shots/frame.png' data-note='keep'>\n"
        html_document.write_text(html_source, encoding='utf-8')
        reference_document = workspace / 'reference-story.md'
        reference_source = '# References\n\n![cover][shot]\n\nAnother ![detail][shot] image\n\n[shot]: ./shots/frame.png\n'
        reference_document.write_text(reference_source, encoding='utf-8')
        requests = []

        class MockPicGo(BaseHTTPRequestHandler):
            def do_POST(self):
                body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                image_path = Path(body['list'][0])
                source = document.read_text(encoding='utf-8')
                requests.append((self.path, self.headers.get('Authorization'), image_path, source, image_path.read_bytes()))
                result = json.dumps({'success': True, 'result': ['https://example.test/uploaded.png']}).encode()
                self.send_response(500 if len(requests) == 1 else 200)
                self.send_header('Content-Length', str(len(result)))
                self.end_headers()
                self.wfile.write(result)

            def log_message(self, *_args):
                pass

        server = HTTPServer(('127.0.0.1', 0), MockPicGo)
        worker = threading.Thread(target=server.serve_forever, daemon=True)
        worker.start()
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {document}\nopen-file\nwait 1600\nimage-host-insert {image}\nwait 2500\nimage-host local\nimage-host upload\nwait 2000\nimage-host local\nimage-host upload\nwait 3500\nimage-host remote\nimage-host-insert {svg}\nwait 3000\nimage-host svg\nwait 1800\npick {html_document}\nopen-file\nwait 2500\nimage-host local\nimage-host upload\nwait 3500\nimage-host remote\npick {reference_document}\nopen-file\nwait 2500\nimage-host local\nimage-host upload\nwait 3500\nimage-host remote\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root),
                   MARKDOWNX_PICGO_ENDPOINT=f'http://127.0.0.1:{server.server_port}/upload', PICGO_SERVER_SECRET='test-secret')
        env.pop('FDL_NOPARK', None)
        try:
            result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                    timeout=70, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        finally:
            server.shutdown()
            worker.join(timeout=5)
            server.server_close()
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'image-host: {phase} PASS' not in logs for phase in ('local', 'remote', 'svg')):
            raise SystemExit(f'Image host desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-4000:]}')
        if 'ERROR gpui::asset_cache: Failed to load asset' in logs:
            raise SystemExit(f'An inserted image could not be rendered:\n{logs[-4000:]}')
        if len(requests) != 4:
            raise SystemExit(f'Expected one failed upload, one retry, one HTML upload and one reference upload, got {len(requests)}')
        target, auth, stored, before, content = requests[1]
        if target != '/upload' or auth != 'Bearer test-secret' or stored.parent != workspace / 'assets' or content != PNG:
            raise SystemExit('PicGo request did not use the local workspace asset')
        final = document.read_text(encoding='utf-8')
        if '![](./assets/' not in before or 'https://example.test/uploaded.png' not in final:
            raise SystemExit(f'Markdown did not transition from a relative local image to the uploaded URL: before={before!r}; final={final!r}; logs={logs[-3000:]}')
        if '.svg)' not in final:
            raise SystemExit('Inserted SVG was not saved in Markdown')
        if stored.read_bytes() != PNG or image.read_bytes() != PNG:
            raise SystemExit('Image upload unexpectedly modified a local file')
        target, auth, uploaded_html, _, content = requests[2]
        html_saved = html_document.read_text(encoding='utf-8')
        if target != '/upload' or auth != 'Bearer test-secret' or uploaded_html != html_image or content != PNG:
            raise SystemExit('HTML image upload did not use the original relative asset')
        if "<img alt='cover'\n width='120' src='https://example.test/uploaded.png' data-note='keep'>" not in html_saved or html_image.read_bytes() != PNG:
            raise SystemExit('HTML image upload did not preserve the tag and local image')
        target, auth, uploaded_ref, _, content = requests[3]
        saved = reference_document.read_text(encoding='utf-8')
        if target != '/upload' or auth != 'Bearer test-secret' or uploaded_ref != html_image or content != PNG:
            raise SystemExit('Reference image upload did not use the original relative asset')
        if '![cover](https://example.test/uploaded.png)' not in saved or '![detail][shot]' not in saved or '[shot]: ./shots/frame.png' not in saved:
            raise SystemExit(f'Reference upload changed its shared definition or another use: {saved!r}')
        print('Workspace image storage, failed upload, PicGo retry, HTML and reference image uploads: PASS')


if __name__ == '__main__':
    main()
