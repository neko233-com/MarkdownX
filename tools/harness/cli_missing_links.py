"""Verify CLI links still refer to the source directory when targets are missing."""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile
from urllib.parse import unquote

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if not binary.is_file(): raise SystemExit('Build the release app before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-cli-missing-link-') as tmp:
        root = Path(tmp)
        dest = root / 'export'
        dest.mkdir()
        source = root / 'paper.md'
        text = '# Paper\n\n[First](./future%20%231.pdf#page=2)\n\n[Second][paper]\n\n[paper]: future%20%231.pdf#page=2\n'
        source.write_text(text, encoding='utf-8')
        wrong = dest / 'future #1.pdf'
        wrong.write_bytes(b'destination collision')
        outputs = []
        for ext in ('html', 'pdf'):
            output = dest / f'paper.{ext}'
            run = subprocess.run([str(binary), 'convert', str(source), '-o', str(output), '--strategy', 'builtin'], capture_output=True,
                                 text=True, encoding='utf-8', errors='replace', timeout=90,
                                 creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
            if run.returncode or not output.is_file(): raise SystemExit(f'{ext} conversion failed: {run.stderr}')
            outputs.append(output)
        html = outputs[0].read_text(encoding='utf-8')
        if html.count('href="../future%20%231.pdf#page=2"') != 2:
            raise SystemExit('HTML links did not retain the missing source-relative target: ' + html[-2000:])
        try:
            import pymupdf
        except ImportError:
            print('PDF link inspection skipped: PyMuPDF is unavailable')
        else:
            with pymupdf.open(outputs[1]) as doc:
                links = [link for page in doc for link in page.get_links() if link.get('kind') in (pymupdf.LINK_GOTOR, pymupdf.LINK_URI)]
            target = str((root / 'future #1.pdf').resolve()).replace('\\', '/').lower()
            matched = [link for link in links if target in unquote(str(link.get('file', link.get('uri', '')))).replace('\\', '/').lower() and link.get('page', 1) == 1]
            if len(matched) != 2: raise SystemExit(f'PDF links did not retain source target and page: {links!r}')
        chapter_dir = root / 'chapters'
        chapter_dir.mkdir()
        nested = chapter_dir / 'nested.md'
        nested.write_text('# Nested\n\n[Draft](../future%20%231.pdf#page=2)\n\n[Folder](./)\n', encoding='utf-8')
        blocked = dest / 'blocked.html'
        result = subprocess.run([str(binary), 'convert', str(nested), '-o', str(blocked), '--strategy', 'builtin'], capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=30, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        if result.returncode == 0 or blocked.exists() or '--root' not in result.stderr:
            raise SystemExit(f'Out-of-root link was not rejected clearly: {result.stderr!r}')
        result = subprocess.run([str(binary), 'convert', str(nested), '-o', str(blocked), '--strategy', 'builtin', '--root', str(root)], capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=30, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        if result.returncode or 'href="../future%20%231.pdf#page=2"' not in blocked.read_text(encoding='utf-8') or 'href="../chapters/"' not in blocked.read_text(encoding='utf-8'):
            raise SystemExit(f'Expanded root or folder link failed: {result.stderr!r}')
        if source.read_text(encoding='utf-8') != text or wrong.read_bytes() != b'destination collision':
            raise SystemExit('Conversion changed a source or unrelated destination file')
    print('CLI missing-target HTML/PDF links: PASS')


if __name__ == '__main__':
    main()
