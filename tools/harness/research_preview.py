"""Verify read-only legal and research previews in an isolated desktop window."""
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
    parser.add_argument('-Quiet', action='store_true', help='Keep the test window offscreen')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-research-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        ofd = root / 'contract.ofd'
        with zipfile.ZipFile(ofd, 'w') as z:
            z.writestr('OFD.xml', '<ofd:OFD><ofd:DocBody><ofd:DocRoot>Doc/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>')
            z.writestr('Doc/Document.xml', '<ofd:Document><ofd:Pages><ofd:Page BaseLoc="Page.xml"/></ofd:Pages></ofd:Document>')
            z.writestr('Doc/Page.xml', '<ofd:Page><ofd:TextObject><ofd:TextCode>CONTRACT OFD CONTENT</ofd:TextCode></ofd:TextObject></ofd:Page>')
        docx = root / 'paper.docx'
        with zipfile.ZipFile(docx, 'w') as z:
            z.writestr('word/document.xml', '<w:document><w:p><w:r><w:t>PAPER DOCX CONTENT</w:t></w:r></w:p></w:document>')
        rtf = root / 'contract.rtf'
        rtf.write_bytes(br"{\rtf1\ansi\ansicpg936 LEGAL RTF CONTENT \'d6\'d0\'ce\'c4{\v  hidden}}")
        qmd = root / 'paper.qmd'
        qmd.write_text('# QUARTO SOURCE\n\nResearch text.\n', encoding='utf-8')
        demo = root / 'demo.rtf'
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例合同.rtf', demo)
        originals = {path: path.read_bytes() for path in (ofd, docx, rtf, qmd, demo)}
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {ofd}\nopen-file\nwait 2500\ncheck-research-preview CONTRACT OFD CONTENT\ntype MARKDOWNX_RESEARCH_EDIT_PROBE\nwait 500\ncheck-research-preview CONTRACT OFD CONTENT\npick {docx}\nopen-file\nwait 2500\ncheck-research-preview PAPER DOCX CONTENT\ntype MARKDOWNX_RESEARCH_EDIT_PROBE\nwait 500\ncheck-research-preview PAPER DOCX CONTENT\npick {rtf}\nopen-file\nwait 2500\ncheck-research-preview LEGAL RTF CONTENT 中文\ntype MARKDOWNX_RESEARCH_EDIT_PROBE\nwait 500\ncheck-research-preview LEGAL RTF CONTENT 中文\npick {qmd}\nopen-file\nwait 2500\ncheck-research-preview QUARTO SOURCE\ntype MARKDOWNX_RESEARCH_EDIT_PROBE\nwait 500\ncheck-research-preview QUARTO SOURCE\npick {demo}\nopen-file\nwait 2500\ncheck-research-preview 合同示例\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=60, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('research-preview: PASS') < 9:
            raise SystemExit(f'Research desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-5000:]}')
        if any(path.read_bytes() != original for path, original in originals.items()):
            raise SystemExit('Read-only preview changed the original document')
        print('OFD, DOCX, RTF and Quarto desktop read-only previews: PASS')


if __name__ == '__main__':
    main()
