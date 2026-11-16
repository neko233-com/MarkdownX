"""Exercise the real DOCX layout preview in an isolated Windows window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-docx-layout-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        docx = root / '合同.docx'
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例合同.docx', docx)
        original = docx.read_bytes()
        script = root / 'steps.txt'
        script.write_text(f'''resize 1100 760
wait 1000
pick {docx}
open-file
wait 12000
check-research-preview 分镜制作服务合同示例
check-inline-html docx
type MARKDOWNX_READ_ONLY_PROBE
wait 500
check-research-preview 分镜制作服务合同示例
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=45, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or 'inline-html: docx PASS' not in logs or logs.count('research-preview: PASS') < 2:
            raise SystemExit(f'DOCX layout desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-5000:]}')
        if docx.read_bytes() != original:
            raise SystemExit('DOCX preview changed the source file')
        print('DOCX read-only layout, table, image and source protection: PASS')


if __name__ == '__main__':
    main()
