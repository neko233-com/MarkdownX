"""Exercise the app's own Windows caption messages in disposable windows/libraries.

Run after a devtools build: python tools/harness/window_controls.py
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    if os.name != 'nt':
        raise SystemExit('This check exercises the native Windows window procedure.')
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target' / 'release' / 'markdownx.exe'))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    subprocess.run([sys.executable, str(ROOT / 'scripts/clean_test_artifacts.py')], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='markdownx-window-controls-') as tmp:
        base = Path(tmp).resolve()
        root = base / 'workspace'
        root.mkdir()
        (root / '.markdownx-window-test').touch()
        file = root / 'note.md'
        (base / 'documents').mkdir()

        def run(name, steps, expected, expected_text):
            library = base / ('library-' + name)
            shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
            file.write_text('# Original\n', encoding='utf-8')
            script = base / (name + '.txt')
            script.write_text(steps, encoding='utf-8')
            env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(base / 'logs'),
                       MARKDOWNX_WINDOW_TEST_ROOT=str(root), FDL_SCRIPT=str(script), FDL_SHOTS=str(base))
            try:
                result = subprocess.run([str(binary)], cwd=base, env=env, capture_output=True, text=True,
                                        encoding='utf-8', errors='replace', timeout=30)
            except subprocess.TimeoutExpired as error:
                captured = b'\n'.join(part for part in (error.stdout, error.stderr) if part).decode('utf-8', 'replace')
                raise SystemExit(f'{name}: native window did not close in time:\n{captured[-4000:]}') from error
            output = result.stdout + result.stderr
            if result.returncode or any(marker not in output for marker in expected):
                raise SystemExit(f'{name}: native window check failed:\n' + output[:5000] + '\n…\n' + output[-1500:])
            if expected_text is not None and file.read_text(encoding='utf-8') != expected_text:
                raise SystemExit(f'{name}: the temporary document did not contain the expected saved text')

        if '--probe-only' in sys.argv:
            run('probe', '''wait 800
window-native prepare
window-native probe
quit
''', ('window-native: probe PASS',), None)
            return
        if '--click-only' in sys.argv:
            run('click', '''wait 800
window-native prepare
window-native maximize
wait 600
window-native expect-maximized
quit
''', ('window-native: maximize PASS', 'window-native: expect-maximized PASS'), None)
            return
        if '--conflict-only' not in sys.argv:
            run('success', '''wait 800
window-native prepare
window-native probe
window-native maximize
wait 600
window-native expect-maximized
window-native restore
wait 600
window-native expect-restored
window-native zoom-action
wait 600
window-native expect-maximized
window-native zoom-action
wait 600
window-native expect-restored
window-native minimize
wait 600
window-native expect-minimized
window-native show-no-activate
wait 600
check-window-close prepare-success
window-native close
wait 6000
''', tuple(f'window-native: {part} PASS' for part in ('probe', 'maximize', 'expect-maximized', 'restore', 'expect-restored', 'zoom-action', 'minimize', 'expect-minimized')) + ('window-close: prepare-success PASS',),
                '# Saved on close\n\nTemporary content.\n')
            run('fullscreen', '''wait 800
window-native prepare
window-native fullscreen-action
wait 600
window-native expect-fullscreen
window-client maximize
wait 600
window-native expect-not-fullscreen
window-native fullscreen-action
wait 600
window-native expect-fullscreen
window-client minimize
wait 600
window-native expect-minimized
window-native show-no-activate
wait 600
window-native expect-fullscreen
check-window-close prepare-success
window-client close
wait 6000
''', ('window-native: expect-fullscreen PASS', 'window-native: expect-not-fullscreen PASS',
      'window-client: maximize PASS', 'window-client: minimize PASS', 'window-native: expect-minimized PASS',
      'window-close: prepare-success PASS'), '# Saved on close\n\nTemporary content.\n')
        run('conflict', '''wait 800
window-native prepare
check-window-close prepare-conflict
window-native close
wait 1800
check-window-close resolve-conflict
wait 350
window-native close
wait 6000
''', ('window-close: prepare-conflict PASS', 'window-close: resolve-conflict PASS'), '# External change\n')
        if '--conflict-only' in sys.argv:
            print('Native Windows close conflict guard: PASS.')
        else:
            print('Windows caption controls: PASS (hit tests, maximize/restore, minimize, fullscreen controls, save on close and conflict guard).')


if __name__ == '__main__':
    main()
