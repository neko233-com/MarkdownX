"""Exercise global search with real keys and an isolated library and folder.

Run after building with --features devtools: python tools/harness/global_search.py
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target' / 'release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    subprocess.run([sys.executable, str(ROOT / 'scripts/clean_test_artifacts.py')], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='markdownx-global-search-') as tmp:
        temp = Path(tmp)
        folder = temp / 'folder'
        (folder / 'notes').mkdir(parents=True)
        (folder / '.markdownx-global-search-test').touch()
        empty = temp / 'empty'
        empty.mkdir()
        (empty / '.markdownx-global-search-test').touch()
        for name in ('a.md', 'b.md'):
            (folder / 'notes' / name).write_text('# Test\n\nglobalsearchuniquexyz in a file\n', encoding='utf-8')
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        doc = next((library / 'docs').glob('*.xml'))
        with doc.open('a', encoding='utf-8') as out:
            out.write('\n<p>globalsearchuniquexyz in the library</p>\n')
        modifier = 'cmd' if sys.platform == 'darwin' else 'ctrl'
        script = temp / 'steps.txt'
        script.write_text(f'''wait 700
pick {folder}
open-folder
wait 1200
resize 480 600
wait 250
check-global-search ready {folder}
key {modifier}-k
wait 250
check-global-search open {folder}
type stalequery
key {modifier}-a
type globalsearchuniquexyz
wait 1600
check-global-search results {folder}
key down down
wait 200
check-global-search moved {folder}
key enter
wait 500
check-global-search picked {folder}
key {modifier}-shift-k
wait 250
check-global-search link {folder}
key escape
wait 200
key {modifier}-k
wait 250
check-global-search open {folder}
key escape
wait 200
key {modifier}-j
wait 200
check-global-search legacy {folder}
pick {empty}
open-folder
wait 800
check-global-search empty {empty}
key {modifier}-k
wait 250
check-global-search open {empty}
key escape
wait 200
check-global-search closed {empty}
key {modifier}-k
wait 250
check-global-search open {empty}
type previousquery
key {modifier}-k
type globalsearchuniquexyz
pick {folder}
open-folder
wait 1600
check-global-search open {folder}
check-global-search results {folder}
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'), FDL_SCRIPT=str(script), FDL_SHOTS=str(temp))
        result = subprocess.run([str(binary)], cwd=temp, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=90)
        output = result.stdout + result.stderr
        expected = ('ready', 'open', 'results', 'moved', 'picked', 'link', 'legacy', 'empty', 'closed')
        if result.returncode or any(f'global-search: {phase} PASS' not in output for phase in expected):
            raise SystemExit('Global search UI check failed:\n' + output[:6000] + '\n…\n' + output[-1500:])
        print('Global search UI: PASS (library, folders, stale query, keyboard selection, focus, link and legacy shortcuts).')


if __name__ == '__main__':
    main()
