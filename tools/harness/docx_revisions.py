"""Exercise inserted and deleted DOCX text in both read-only views."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-docx-revisions-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        docx = root / '修订合同.docx'
        with zipfile.ZipFile(ROOT / 'MarkdownX-Demo/示例合同.docx') as source, zipfile.ZipFile(docx, 'w') as target:
            for entry in source.infolist():
                data = source.read(entry.filename)
                if entry.filename == 'word/document.xml':
                    marker = b'<w:sectPr'
                    revision = b'<w:p><w:del w:id="1" w:author="Demo"><w:r><w:delText>OLD_CLAUSE</w:delText></w:r></w:del><w:ins w:id="2" w:author="Demo"><w:r><w:t>NEW_CLAUSE</w:t></w:r></w:ins></w:p>'
                    assert marker in data
                    data = data.replace(marker, revision + marker, 1)
                target.writestr(entry, data)
        original = docx.read_bytes()
        script = root / 'steps.txt'
        script.write_text(f'''resize 1100 760
wait 1000
pick {docx}
open-file
wait 12000
check-research-preview [Deleted: OLD_CLAUSE]
check-research-preview [Inserted: NEW_CLAUSE]
check-inline-html docx-revisions
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=45, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('research-preview: PASS') < 2 or 'inline-html: docx-revisions PASS' not in logs:
            raise SystemExit(f'DOCX revision test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-5000:]}')
        if docx.read_bytes() != original:
            raise SystemExit('DOCX preview changed the source file')
        print('DOCX revision layout, extracted text and source protection: PASS')


if __name__ == '__main__':
    main()
