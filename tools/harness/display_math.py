"""Exercise display equations and autosave in a real isolated desktop window."""
import argparse
import json
import unicodedata
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
        raise SystemExit('Build the Windows app with --features devtools first.')
    with tempfile.TemporaryDirectory(prefix='markdownx-math-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        source = root / 'paper.md'
        original = '# Paper\r\n\r\nInline ($x_i^2$).\r\n\r\n$$\r\n\\frac{a_i}{\\sqrt{1+b}}\r\n$$\r\n\r\n```tex\r\n$$not_math$$\r\n```\r\n'
        source.write_bytes(original.encode('utf-8'))
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {source}\nopen-file\nwait 2500\ncheck-display-math initial\nwait 500\ncheck-display-math open\nwait 500\ntype \\frac{{z}}{{2}}\nwait 500\ncheck-display-math edited\nkey escape\nwait 4000\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        run = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                             timeout=50, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(p.read_text(encoding='utf-8', errors='replace') for p in (root / 'logs').glob('*') if p.is_file())
        if run.returncode or not all(f'display-math: {phase} PASS' in logs for phase in ('initial', 'open', 'edited')):
            raise SystemExit(f'Desktop formula test failed: {run.returncode}\n{run.stderr[-2000:]}\n{logs[-4000:]}')
        expected = original.replace('\\frac{a_i}{\\sqrt{1+b}}', '\\frac{z}{2}')
        if source.read_bytes() != expected.encode('utf-8'):
            raise SystemExit('Formula autosave changed unrelated Markdown: ' + repr(source.read_bytes()))
        html = root / 'paper.html'
        result = subprocess.run([str(binary), 'convert', str(source), '-o', str(html), '--strategy', 'builtin'], capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=60, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode or 'class="mathb"' not in html.read_text(encoding='utf-8') or '<mfrac>' not in html.read_text(encoding='utf-8'):
            raise SystemExit(f'CLI display math failed: {result.stderr}')
        pdf = root / 'paper.pdf'
        result = subprocess.run([str(binary), 'convert', str(source), '-o', str(pdf), '--strategy', 'builtin'], capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=90, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode or not pdf.is_file() or not pdf.read_bytes().startswith(b'%PDF-'):
            raise SystemExit(f'CLI formula PDF failed: {result.stderr}')
        worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=pdf.read_bytes(), capture_output=True,
                                timeout=30, creationflags=subprocess.CREATE_NO_WINDOW)
        extracted = unicodedata.normalize('NFKC', ' '.join(json.loads(worker.stdout).get('pages', []))) if not worker.returncode else ''
        if 'z' not in extracted or '2' not in extracted:
            raise SystemExit(f'Formula text not found in rendered PDF: {worker.stderr!r} {worker.stdout[:500]!r}')
    print('Desktop formula layout, edit, autosave and HTML/PDF export: PASS')


if __name__ == '__main__':
    main()
