"""Exercise real asynchronous desktop update checks against a loopback release fixture."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
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
        raise SystemExit('Build the Windows app with --features devtools first.')
    requests = []
    version = json.loads(subprocess.check_output(['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=ROOT, creationflags=subprocess.CREATE_NO_WINDOW))['packages']
    current = next(p['version'] for p in version if p['name'] == 'markdownx')
    future = {'tag_name': 'v99.0.0', 'draft': False, 'prerelease': False, 'html_url': 'https://untrusted.test/setup.exe'}
    replies = [(200, future), (500, {}), (200, dict(future, tag_name='v' + current)), (404, {}), (200, {}), (200, future)]

    class Fixture(BaseHTTPRequestHandler):
        def do_GET(self):
            index = len(requests)
            requests.append(self.path)
            if index == 0:
                time.sleep(.8)
            status, body = replies[min(index, len(replies) - 1)]
            data = json.dumps(body).encode()
            self.send_response(status)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        def log_message(self, *_):
            pass

    server = ThreadingHTTPServer(('127.0.0.1', 0), Fixture)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix='markdownx-updates-') as tmp:
            root = Path(tmp)
            shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
            (root / 'library/preferences.json').write_text(json.dumps({'auto_update': False, 'remote_images': False, 'language': 'en', 'appearance': 'light'}), encoding='utf-8')
            (root / 'documents').mkdir()
            steps = root / 'steps.txt'
            steps.write_text('wait 1000\nupdate-check manual\nupdate-check manual\nupdate-state checking\nwait 1800\nupdate-state available\n'
                             'appearance dark\nwait 400\nupdate-check manual\nwait 1200\nupdate-state failed\n'
                             'update-check manual\nwait 1200\nupdate-state current\nupdate-check auto\nwait 400\nupdate-state current\n'
                             'update-check manual\nwait 1200\nupdate-state none\nupdate-check manual\nwait 1200\nupdate-state failed\n'
                             'appearance light\nupdate-check manual\nwait 1200\nupdate-state available\nquit\n', encoding='utf-8')
            env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                       MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root),
                       MARKDOWNX_TEST_RELEASE_URL=f'http://127.0.0.1:{server.server_port}/latest')
            env.pop('FDL_NOPARK', None)
            result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                    timeout=40, creationflags=subprocess.CREATE_NO_WINDOW)
            logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
            for state in ['checking', 'available', 'failed', 'current', 'none']:
                if f'update-state: {state} PASS' not in logs:
                    raise SystemExit(f'Missing {state} verification: {result.returncode}\n{(result.stderr + logs)[-7000:]}')
            if result.returncode or requests != ['/latest'] * 6:
                raise SystemExit(f'Unexpected update requests or exit: {result.returncode}, {requests}\n{(result.stderr + logs)[-7000:]}')
            print('Desktop manual checks, duplicate suppression, disabled-auto override, retry, HTTP errors, no-release, malformed response and appearance changes: PASS')
    finally:
        server.shutdown()
        thread.join(timeout=5)
        server.server_close()


if __name__ == '__main__':
    main()
