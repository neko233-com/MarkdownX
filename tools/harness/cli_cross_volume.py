"""Check verified relative file links when HTML/PDF are written to another volume."""
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
    if os.name != 'nt' or ROOT.drive.casefold() == Path(tempfile.gettempdir()).drive.casefold():
        print('Cross-volume test skipped: two Windows volumes are unavailable')
        return
    if not binary.is_file():
        raise SystemExit('Build the release app first.')
    with tempfile.TemporaryDirectory(prefix='.dsh-cli-cross-', dir=ROOT) as src, tempfile.TemporaryDirectory(prefix='markdownx-cross-out-') as dst:
        root, out = Path(src), Path(dst)
        target = root / '合同 #1.pdf'
        target.write_bytes(b'%PDF-1.4\n%%EOF\n')
        source = root / 'paper.md'
        content = ('# Paper\n\n[One](./合同%20%231.pdf#page=2) and [Two](合同%20%231.pdf#page=2).\n\n'
                   '[Unsafe](file:///Z:/outside.txt) and [Script](javascript:alert(1)).\n')
        source.write_text(content, encoding='utf-8', newline='')
        for ext in ('html', 'pdf'):
            result = subprocess.run([str(binary), 'convert', str(source), '-o', str(out / f'paper.{ext}'), '--strategy', 'builtin'],
                                    capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=120,
                                    creationflags=subprocess.CREATE_NO_WINDOW)
            if result.returncode:
                raise SystemExit(f'{ext} cross-volume conversion failed: {result.stderr}')
        html = (out / 'paper.html').read_text(encoding='utf-8')
        expected = target.resolve().as_uri() + '#page=2'
        if html.count(f'href="{expected}"') != 2:
            raise SystemExit(f'validated relative links were not preserved: {expected}\n{html[-3000:]}')
        if 'href="file:///Z:' in html or 'href="javascript:' in html:
            raise SystemExit('an unverified file or active URL became clickable')
        if not (out / 'paper.pdf').read_bytes().startswith(b'%PDF-'):
            raise SystemExit('cross-volume PDF is invalid')
        try:
            import pymupdf
        except ImportError:
            print('PDF link inspection skipped: PyMuPDF is unavailable')
        else:
            with pymupdf.open(out / 'paper.pdf') as pdf:
                links = [link for page in pdf for link in page.get_links() if link.get('kind') == pymupdf.LINK_GOTOR]
            if len(links) != 2 or any(link.get('page') != 1 or not unquote(link.get('file', '')).endswith(target.name) for link in links):
                raise SystemExit(f'cross-volume PDF links are missing or point to the wrong page: {links!r}')
        if source.read_text(encoding='utf-8') != content:
            raise SystemExit('conversion modified the source')
    print('Cross-volume HTML/PDF links and unsafe URL filtering: PASS')


if __name__ == '__main__':
    main()
