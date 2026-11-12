"""Exercise multiple workspace trees and safe file actions in isolated libraries.

Run after building with --features devtools: python tools/harness/workspace_startup.py
"""
import json
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
    with tempfile.TemporaryDirectory(prefix='markdownx-workspace-startup-') as tmp:
        temp = Path(tmp).resolve()
        (temp / '.markdownx-workspace-test').touch()
        documents = temp / 'documents'
        documents.mkdir()
        default = documents / 'MarkDown-Demo'
        repo = temp / 'repository'
        (repo / '.git').mkdir(parents=True)
        chosen = repo / '所选目录'
        chosen.mkdir()
        note = chosen / 'note.md'
        note.write_text('# Second\n\nmultiworkspacesearch\n', encoding='utf-8')
        image = chosen / '示例 image.png'
        image.write_bytes(b'\x89PNG\r\n')
        replacement = temp / 'replacement'
        replacement.mkdir()
        (replacement / 'keep.md').write_text('# Keep\n', encoding='utf-8')
        override = temp / '临时目录'
        override.mkdir()
        missing = temp / 'unavailable-folder'
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        library_doc = next((library / 'docs').glob('*.xml'))
        original_library_doc = library_doc.read_bytes()
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(temp), FDL_SHOTS=str(temp))
        env.pop('FDL_NOPARK', None)
        mod = 'cmd' if sys.platform == 'darwin' else 'ctrl'

        def run(name, steps, expected, *args):
            script = temp / f'{name}.txt'
            script.write_text(steps, encoding='utf-8')
            result = subprocess.run([str(binary), *map(str, args)], cwd=temp, env={**env, 'FDL_SCRIPT': str(script)},
                                    capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=120)
            output = result.stdout + result.stderr
            if result.returncode or any(phase not in output for phase in expected):
                raise SystemExit(f'Workspace {name} failed:\n' + output[:7000] + '\n…\n' + output[-3000:])

        run('manage', f'''wait 900
resize 480 600
wait 200
check-workspace-startup default {default}
pick {chosen}|{missing}
open-folder
wait 1300
pick {chosen}
open-folder
wait 500
check-workspace-startup selected {chosen}
check-multi-workspace file-types
check-multi-workspace library-rename-start
wait 250
type Renamed from Home
key enter
wait 300
check-multi-workspace library-rename-done
check-multi-workspace prepare
wait 800
check-multi-workspace switch-start
wait 1800
check-multi-workspace switch-done
key {mod}-k
type multiworkspacesearch
wait 1600
check-multi-workspace search
key {mod}-j
wait 200
check-multi-workspace legacy
check-multi-workspace new-file
wait 250
type draft
key enter
wait 1100
check-multi-workspace new-file-done
check-multi-workspace new-dir
wait 250
type nested
key enter
wait 1100
check-multi-workspace new-dir-done
check-multi-workspace rename-file
wait 250
type renamed.md
key enter
wait 1500
check-multi-workspace rename-done
check-multi-workspace remove
wait 1800
check-multi-workspace removed
pick {replacement}
check-multi-workspace replace
wait 1200
check-multi-workspace replaced
check-multi-workspace label
wait 250
type 新的工作区
key enter
wait 300
check-multi-workspace label-done
quit
''', ('workspace-startup: default PASS', 'workspace-startup: selected PASS',
      *(f'multi-workspace: {phase} PASS' for phase in ('file-types', 'library-rename-done', 'switch-done', 'search', 'legacy', 'new-file-done', 'new-dir-done', 'rename-done', 'removed', 'replaced', 'label-done'))))
        assert default.is_dir() and (default / 'alpha.md').is_file()
        assert not missing.exists()
        prefs_path = library / 'preferences.json'
        prefs = json.loads(prefs_path.read_text(encoding='utf-8'))
        assert [Path(r['path']) for r in prefs['workspaces']] == [replacement, missing]
        run('restore', f'''wait 900
check-workspace-startup restored {replacement}
quit
''', ('workspace-startup: restored PASS',))
        run('override', f'''wait 900
check-workspace-startup override {override}
quit
''', ('workspace-startup: override PASS',), override)
        assert json.loads(prefs_path.read_text(encoding='utf-8'))['workspaces'] == prefs['workspaces']
        prefs['workspaces'] = []
        prefs_path.write_text(json.dumps(prefs, ensure_ascii=False), encoding='utf-8')
        run('empty', f'''wait 600
check-workspace-startup empty {replacement}
quit
''', ('workspace-startup: empty PASS',))
        assert library_doc.read_bytes() == original_library_doc
        assert image.read_bytes() == b'\x89PNG\r\n'
        print('Multiple workspaces: PASS (default, add/deduplicate, missing root, background save, cross-root search, create/rename, remove/replace, restart and temporary startup override).')


if __name__ == '__main__':
    main()
