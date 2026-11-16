"""Open card and inline PDF attachments in the Windows read-only viewer."""
import argparse
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from urllib.parse import quote

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.add_argument('-Demo', action='store_true')
    args = parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-pdf-attachment-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        chapter = root / 'chapter'
        chapter.mkdir()
        pdf = chapter / ('论文阅读示例.pdf' if args.Demo else '论文 示例.pdf')
        shutil.copyfile(ROOT / 'MarkdownX-Demo/论文阅读示例.pdf', pdf)
        before = hashlib.sha256(pdf.read_bytes()).digest()
        relative = quote(pdf.name)
        source = (f'<title>Attachments</title>\n<figure view-type="Card"><source path="./{relative}" name="paper.pdf"/></figure>\n'
                  f'<p>Read <source path="{relative}" name="paper.pdf"/> here.</p>\n')
        if args.Demo: source = (ROOT / 'MarkdownX-Demo/论文附件示例.docxxml').read_text(encoding='utf-8')
        doc = chapter / 'paper.docxxml'
        doc.write_text(source, encoding='utf-8')
        steps = root / 'steps.txt'
        steps.write_text(f'''resize 1100 760
wait 1000
pick {doc}
open-file
wait 4000
click-pdf-attachment
wait 500
click-pdf-attachment
wait 5000
check-media-preview pdf-attachment
key escape
wait 800
click-inline-attachment
wait 5000
check-media-preview pdf-attachment
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('media-preview: pdf-attachment PASS') != 2:
            raise SystemExit(f'PDF attachment test failed: {result.returncode}\n{result.stderr[:3500]}\n{logs[:3500]}')
        if hashlib.sha256(pdf.read_bytes()).digest() != before or doc.read_text(encoding='utf-8') != source:
            raise SystemExit('Opening the attachment changed a source file')
        print('PDF card and inline attachments open in the read-only viewer: PASS')


if __name__ == '__main__':
    main()
