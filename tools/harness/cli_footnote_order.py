"""Verify real HTML/PDF exports number interleaved paper footnotes correctly."""
import argparse
from html.parser import HTMLParser
import os
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


class Links(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = set()
        self.refs = []
        self.backlinks = []
        self.in_reference = False
        self.current = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if 'id' in attrs:
            if attrs['id'] in self.ids:
                raise AssertionError(f'duplicate id: {attrs["id"]}')
            self.ids.add(attrs['id'])
        if tag == 'sup' and attrs.get('class') == 'footnote-ref':
            self.in_reference = True
        elif tag == 'a' and self.in_reference:
            self.current = [attrs.get('id'), attrs.get('href'), '']
        elif tag == 'a' and attrs.get('aria-label') == '返回引用':
            self.backlinks.append(attrs.get('href'))

    def handle_data(self, text):
        if self.current is not None:
            self.current[2] += text

    def handle_endtag(self, tag):
        if tag == 'a' and self.current is not None:
            self.refs.append(tuple(self.current))
            self.current = None
        elif tag == 'sup':
            self.in_reference = False


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if not binary.is_file():
        raise SystemExit('Build the release app first.')
    hidden = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
    source = ('# Paper[^a]\n\n[Jump to section](#sectiona)\n\nFirst[^a].\n\n[^a]: nested[^c] and repeat[^b]\n\n'
              'Later[^b] and again[^a].\n\n## Section[^a]\n\nSection body.\n\n[^b]: back[^a]\n\n[^c]: Last.\n')
    with tempfile.TemporaryDirectory(prefix='markdownx-footnote-cli-') as tmp:
        root = Path(tmp)
        markdown = root / 'paper.md'
        markdown.write_text(source, encoding='utf-8', newline='')
        for extension in ('html', 'pdf'):
            output = root / f'paper.{extension}'
            result = subprocess.run([str(binary), 'convert', str(markdown), '-o', str(output), '--strategy', 'builtin'],
                                    capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=120, creationflags=hidden)
            if result.returncode or not output.is_file():
                raise SystemExit(f'{extension} conversion failed: {result.stderr}')
        html = (root / 'paper.html').read_text(encoding='utf-8')
        links = Links()
        links.feed(html)
        if [number for _, _, number in links.refs] != ['1', '1', '2', '1', '1', '3', '2', '1']:
            raise SystemExit(f'incorrect reference order: {links.refs!r}')
        if 'sectiona' not in links.ids or 'href="#sectiona"' not in html or '<title>Paper1</title>' not in html:
            raise SystemExit('heading anchors or the browser title did not retain the source meaning')
        if any(not href or not href.startswith('#') or href[1:] not in links.ids for _, href, _ in links.refs):
            raise SystemExit('a footnote reference has no target')
        if sorted(links.backlinks) != sorted('#' + ref_id for ref_id, _, _ in links.refs):
            raise SystemExit('footnote backlinks do not match references')
        if not (root / 'paper.pdf').read_bytes().startswith(b'%PDF-'):
            raise SystemExit('PDF export is invalid')
        try:
            import fitz
        except ImportError:
            print('PDF content extraction skipped: PyMuPDF is unavailable')
        else:
            with fitz.open(root / 'paper.pdf') as document:
                text = ''.join(page.get_text() for page in document)
                destinations = [link.get('nameddest') for page in document for link in page.get_links()]
            if not all(word in text for word in ('First', 'Later', 'nested', 'repeat', 'Last')):
                raise SystemExit(f'PDF is missing footnote content: {text!r}')
            if not re.search(r'First\s*1.*Later\s*2', text, re.S):
                raise SystemExit(f'PDF main-text footnote numbering is incorrect: {text!r}')
            if 'sectiona' not in destinations:
                raise SystemExit(f'PDF heading link is missing: {destinations!r}')
        if markdown.read_text(encoding='utf-8') != source:
            raise SystemExit('conversion modified the source')
    print('Interleaved footnote HTML links and PDF content: PASS')


if __name__ == '__main__':
    main()
