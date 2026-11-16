"""Exercise the on-demand demo in an isolated Windows desktop workspace."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-create-demo-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        parent = root / 'destination'
        existing = parent / 'MarkdownX-Demo'
        existing.mkdir(parents=True)
        (existing / 'keep.txt').write_text('untouched', encoding='utf-8')
        created = parent / 'MarkdownX-Demo-2'
        script = root / 'steps.txt'
        script.write_text(f'''resize 1100 760
wait 1000
pick {parent}
create-demo
wait 5000
check-relative-file {created / '开始使用.md'}
pick {created / '分镜与互动演示.md'}
open-file
wait 2000
follow-relative-link ./分镜资料.md#镜头说明
wait 2000
check-relative-file {created / '分镜资料.md'}
follow-relative-link ./分镜与互动演示.md#互动示例
wait 2000
check-relative-file {created / '分镜与互动演示.md'}
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), MARKDOWNX_TEST_LANGUAGE='zh', FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=40, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('relative-link: file PASS') != 3 or logs.count('relative-link: heading PASS') != 2:
            raise SystemExit(f'Demo desktop test failed: {result.returncode}\n{result.stderr[-2000:]}\n{logs[-4000:]}')
        if (existing / 'keep.txt').read_text(encoding='utf-8') != 'untouched' or not (created / '分镜与互动演示.md').is_file() or not (created / 'storyboard-shots/01.svg').is_file():
            raise SystemExit('Demo files are incomplete or an existing folder was modified')
        print('On-demand demo creation, folder tree and existing-file protection: PASS')


if __name__ == '__main__':
    main()
