"""Exercise Feishu import and native Canvas in an isolated off-screen window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-feishu-ui-') as tmp:
        temp = Path(tmp).resolve()
        (temp / '.markdownx-workspace-test').touch()
        documents = temp / 'documents'
        workspace = documents / 'MarkDown-Demo'
        workspace.mkdir(parents=True)
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        cli_dir = temp / 'bin'
        cli_dir.mkdir()
        fixture = json.dumps({'ok': True, 'data': {'document': {'content': '<title>Fixture</title><p>Local mock</p>', 'reference_map': {}}}}, ensure_ascii=False)
        if os.name == 'nt':
            source = temp / 'fake-cli.rs'
            source.write_text('fn main() { let a: Vec<String> = std::env::args().collect(); '
                              'assert!(a.windows(2).any(|v| v == ["--as", "user"])); '
                              'assert!(a.windows(2).any(|v| v == ["--api-version", "v2"])); '
                              'print!("{}", r###"' + fixture + '"###); }\n', encoding='utf-8')
            rustc = os.environ.get('RUSTC', 'rustc')
            subprocess.run([rustc, str(source), '-o', str(cli_dir / 'lark-cli.exe')], check=True, cwd=temp,
                           creationflags=subprocess.CREATE_NO_WINDOW)
        else:
            cli = cli_dir / 'lark-cli'
            cli.write_text('#!/bin/sh\ncase " $* " in *" --as user "*" --api-version v2 "*|*" --api-version v2 "*" --as user "*) ;; *) exit 1 ;; esac\nprintf %s ' + "'" + fixture.replace("'", "'\\''") + "'\n", encoding='utf-8')
            cli.chmod(0o755)
        files = []
        for name, spec in [('circle', {'kind': 'unitcircle', 'angle': 35}), ('projectile', {'kind': 'projectile', 'angle': 45, 'speed': 14})]:
            path = workspace / (name + '.md')
            envelope = json.dumps({'title': name, 'height': 300, 'source': json.dumps(spec)})
            path.write_text('# ' + name + '\n\n```mdxb-canvas\n' + envelope + '\n```\n', encoding='utf-8')
            files.append(path)
        script = temp / 'steps.txt'
        script.write_text(f'''wait 1200
check-workspace-startup default {workspace}
check-feishu-import start
wait 1500
check-feishu-import done
pick {files[0]}
open-file
wait 900
check-native-experiment unitcircle
pick {files[1]}
open-file
wait 900
check-native-experiment projectile
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(temp), FDL_SCRIPT=str(script), FDL_SHOTS=str(temp),
                   PATH=str(cli_dir) + os.pathsep + os.environ.get('PATH', ''))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=temp, env=env, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=120,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        output = result.stdout + result.stderr
        expected = ['feishu-import: start PASS', 'feishu-import: done PASS', 'native-experiment: unitcircle PASS', 'native-experiment: projectile PASS']
        if result.returncode or any(message not in output for message in expected):
            raise SystemExit('Feishu / Canvas UI check failed:\n' + output[:6000] + '\n…\n' + output[-3000:])
        print('Feishu import and native Canvas: PASS (mock service, isolated workspace)')


if __name__ == '__main__':
    main()
