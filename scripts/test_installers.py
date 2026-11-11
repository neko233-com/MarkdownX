"""Exercise installer decisions using isolated packages and mocked macOS tools."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parent.parent


def shell_path():
    if os.name == 'nt':
        candidate = Path(os.environ.get('ProgramFiles', r'C:\Program Files')) / 'Git/bin/sh.exe'
        return str(candidate) if candidate.exists() else None
    return shutil.which('sh')


def shell_name(path):
    text = str(path)
    if os.name == 'nt':
        return '/' + text[0].lower() + text[2:].replace('\\', '/')
    return text


@unittest.skipUnless(shell_path(), 'a POSIX shell is required')
class MacInstallerTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix='markdownx-install-test-')
        self.addCleanup(self.tmp.cleanup)
        self.base = Path(self.tmp.name)
        tools = self.base / 'tools'
        tools.mkdir()
        fixture = self.base / 'fixture' / 'MarkdownX.app' / 'Contents'
        (fixture / 'MacOS').mkdir(parents=True)
        (fixture / 'MacOS/markdownx').write_text('test binary')
        (fixture / 'Info.plist').write_text('valid test app')
        self.package = self.base / 'test.dmg'
        self.package.write_bytes(b'synthetic image, never mounted')
        self.applications = self.base / 'Applications'
        commands = {
            'uname': 'case "$1" in -s) echo Darwin ;; -m) echo arm64 ;; esac',
            'hdiutil': '''case "$1" in
              verify|detach) exit 0 ;;
              attach) while [ "$#" -gt 0 ]; do if [ "$1" = -mountpoint ]; then shift; dest=$1; fi; shift; done; cp -R "$TEST_FIXTURE/MarkdownX.app" "$dest/MarkdownX.app" ;;
              *) exit 1 ;; esac''',
            'ditto': 'cp -R "$1" "$2"',
            'shasum': 'shift 2; if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else /usr/bin/shasum -a 256 "$1"; fi',
            'codesign': '''if [ "${TEST_BAD_SIGNATURE:-0}" = 1 ]; then exit 1; fi
              if [ "$1" = -dv ]; then echo Signature=adhoc >&2; fi''',
            'spctl': 'exit 1',
            'PlistBuddy': '''case "$2" in *CFBundleIdentifier*) if grep -q INVALID "$3"; then echo another.app; else echo com.neko233.markdownx; fi ;; *CFBundleShortVersionString*) echo 0.1.13 ;; *) exit 1 ;; esac''',
            'ps': 'exit 0',
        }
        for name, body in commands.items():
            path = tools / name
            path.write_text('#!/bin/sh\n' + body + '\n', encoding='utf-8', newline='\n')
            path.chmod(0o755)
        self.env = dict(os.environ, HOME=shell_name(self.base), TEST_FIXTURE=shell_name(fixture.parent.parent), TEST_TOOLS=shell_name(tools),
                        PATH=str(tools) + os.pathsep + os.environ.get('PATH', ''))

    def run_installer(self, *args):
        command = 'PATH="$TEST_TOOLS:$PATH"; export PATH; file=$1; shift; . "$file"'
        return subprocess.run([shell_path(), '-c', command, 'installer-test', shell_name(ROOT / 'install.sh'), '--package', shell_name(self.package),
                               '--applications-dir', shell_name(self.applications), *args],
                              env=self.env, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE)

    def test_unsigned_requires_explicit_opt_in_and_dry_run_does_not_install(self):
        denied = self.run_installer('--dry-run')
        self.assertNotEqual(denied.returncode, 0, denied.stdout + denied.stderr)
        self.assertIn('--allow-unsigned', denied.stderr)
        allowed = self.run_installer('--allow-unsigned', '--dry-run')
        self.assertEqual(allowed.returncode, 0, allowed.stdout + allowed.stderr)
        self.assertIn('Ready: MarkdownX 0.1.13', allowed.stdout)
        self.assertFalse(self.applications.exists())

    def test_install_and_replace_recognized_app(self):
        result = self.run_installer('--allow-unsigned')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        app = self.applications / 'MarkdownX.app'
        (app / 'old-file').write_text('previous')
        result = self.run_installer('--allow-unsigned')
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        self.assertFalse((app / 'old-file').exists())
        self.assertTrue((app / 'Contents/MacOS/markdownx').exists())

    def test_rejects_wrong_checksum_and_unrecognized_existing_app(self):
        checksum = self.package.with_name(self.package.name + '.sha256')
        checksum.write_text('0' * 64 + '  test.dmg\n')
        result = self.run_installer('--allow-unsigned')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('SHA-256', result.stderr)
        checksum.unlink()
        existing = self.applications / 'MarkdownX.app/Contents'
        existing.mkdir(parents=True)
        (existing / 'Info.plist').write_text('INVALID')
        result = self.run_installer('--allow-unsigned')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual((existing / 'Info.plist').read_text(), 'INVALID')


@unittest.skipUnless(os.name == 'nt' and shutil.which('pwsh'), 'PowerShell 7 and Windows Installer required')
class WindowsInstallerTests(unittest.TestCase):
    def test_restarted_gui_process_outlives_launcher_and_keeps_arguments(self):
        pythonw = Path(sys.executable).with_name('pythonw.exe')
        if not pythonw.exists():
            self.skipTest('pythonw.exe is unavailable')
        with tempfile.TemporaryDirectory(prefix='markdownx restart test ') as tmp:
            folder = Path(tmp)
            worker = folder / 'worker with spaces.py'
            result = folder / 'result with spaces.txt'
            worker.write_text("import pathlib,sys,time\ntime.sleep(1)\npathlib.Path(sys.argv[1]).write_text(sys.argv[2])\n")
            ps = lambda p: "'" + str(p).replace("'", "''") + "'"
            launcher = folder / 'launcher.ps1'
            launcher.write_text(f'''$ErrorActionPreference = 'Stop'
$ast = [System.Management.Automation.Language.Parser]::ParseFile({ps(ROOT / 'install.ps1')}, [ref]$null, [ref]$null)
$fn = $ast.Find({{ param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Start-InstalledApp' }}, $true)
. ([scriptblock]::Create($fn.Extent.Text))
$appPath = {ps(pythonw)}
$installRoot = {ps(folder)}
Start-InstalledApp @({ps(worker)}, {ps(result)}, 'argument with spaces')
''', encoding='utf-8')
            launched = subprocess.run(['pwsh', '-NoProfile', '-File', str(launcher)], text=True, capture_output=True)
            self.assertEqual(launched.returncode, 0, launched.stdout + launched.stderr)
            deadline = time.monotonic() + 10
            while not result.exists() and time.monotonic() < deadline:
                time.sleep(0.1)
            self.assertTrue(result.exists(), 'the restarted GUI process did not survive its launcher')
            self.assertEqual(result.read_text(), 'argument with spaces')

    def test_local_release_and_hash_failures_are_read_only(self):
        packages = sorted((ROOT / 'dist').glob('MarkdownX-*-x64.msi'))
        if not packages:
            self.skipTest('an existing MSI is required')
        package = packages[-1]
        name = package.name
        content = package.read_bytes()
        digest = hashlib.sha256(content).hexdigest()
        with tempfile.TemporaryDirectory(prefix='markdownx-installer-http-') as tmp:
            isolated = Path(tmp)
            shutil.copy2(ROOT / 'install.ps1', isolated / 'install.ps1')
            state = {'bad': False}
            class Handler(BaseHTTPRequestHandler):
                def do_GET(self):
                    base = f'http://127.0.0.1:{self.server.server_port}'
                    if self.path == '/releases':
                        body = json.dumps([{'draft': False, 'prerelease': True, 'assets': [
                            {'name': name, 'size': len(content), 'digest': 'sha256:' + digest, 'browser_download_url': base + '/package'},
                            {'name': name + '.sha256', 'browser_download_url': base + '/checksum'}]}]).encode()
                    elif self.path == '/package':
                        body = content
                    elif self.path == '/checksum':
                        body = (('0' * 64 if state['bad'] else digest) + '  ' + name + '\n').encode()
                    else:
                        self.send_error(404)
                        return
                    self.send_response(200)
                    self.send_header('Content-Length', str(len(body)))
                    self.end_headers()
                    self.wfile.write(body)
                def log_message(self, *args):
                    pass
            server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            thread.start()
            self.addCleanup(server.server_close)
            self.addCleanup(server.shutdown)
            env = dict(os.environ, LOCALAPPDATA=str(isolated / 'localappdata'))
            command = ['pwsh', '-NoProfile', '-File', str(isolated / 'install.ps1'), '-DryRun',
                       '-ReleaseApi', f'http://127.0.0.1:{server.server_port}/releases']
            good = subprocess.run(command, cwd=isolated, env=env, text=True, capture_output=True)
            self.assertEqual(good.returncode, 0, good.stdout + good.stderr)
            self.assertIn('SHA-256 verified', good.stdout)
            state['bad'] = True
            bad = subprocess.run(command, cwd=isolated, env=env, text=True, capture_output=True)
            self.assertNotEqual(bad.returncode, 0)
            self.assertIn('disagree', bad.stderr)


if __name__ == '__main__':
    unittest.main()
