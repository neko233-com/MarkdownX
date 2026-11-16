"""Verify Office template variants open read-only in an isolated window."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]
MAIN = b'application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml'
TYPES = {
    'docm': b'application/vnd.ms-word.document.macroEnabled.main+xml',
    'dotx': b'application/vnd.openxmlformats-officedocument.wordprocessingml.template.main+xml',
    'dotm': b'application/vnd.ms-word.template.macroEnabledTemplate.main+xml',
}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-office-templates-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        files = []
        for ext, content_type in TYPES.items():
            path = root / f'template.{ext}'
            with zipfile.ZipFile(ROOT / 'MarkdownX-Demo/示例合同.docx') as source, zipfile.ZipFile(path, 'w') as target:
                for entry in source.infolist():
                    data = source.read(entry.filename)
                    if entry.filename == '[Content_Types].xml':
                        assert MAIN in data
                        data = data.replace(MAIN, content_type)
                    target.writestr(entry, data)
            files.append((path, '分镜制作服务合同示例', True))
        dot = root / 'template.dot'
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/legacy-doc/template.dot', dot)
        files.append((dot, '旧版合同示例', False))
        ott = root / 'template.ott'
        with zipfile.ZipFile(ott, 'w') as archive:
            archive.writestr('content.xml', '<office:document xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><text:p>OPEN DOCUMENT TEMPLATE</text:p></office:document>')
        files.append((ott, 'OPEN DOCUMENT TEMPLATE', False))
        original = {path: path.read_bytes() for path, _, _ in files}
        steps = 'resize 1100 760\nwait 1000\n'
        for path, needle, layout in files:
            steps += f'pick {path}\nopen-file\nwait 6500\ncheck-research-preview {needle}\n'
            if layout:
                steps += 'check-inline-html docx\n'
        script = root / 'steps.txt'
        script.write_text(steps + 'quit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=75, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('research-preview: PASS') < 5 or logs.count('inline-html: docx PASS') < 3:
            raise SystemExit(f'Office template desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-5000:]}')
        if any(path.read_bytes() != data for path, data in original.items()):
            raise SystemExit('Office template preview changed a source file')
        print('DOCM, DOTX, DOTM, DOT and OTT read-only previews: PASS')


if __name__ == '__main__':
    main()
