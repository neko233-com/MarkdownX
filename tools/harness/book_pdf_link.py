"""Verify embedded PDF links in a real imported and re-exported desktop book."""
import argparse
import base64
import hashlib
import json
import os
import re
from urllib.parse import unquote
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-book-pdf-ui-') as tmp:
        root = Path(tmp).resolve()
        source = root / 'source.mdxb'
        result = subprocess.run([sys.executable, str(ROOT / 'tools/harness/book_xml_resources.py'), '-Quiet', '--output', str(source)], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=45)
        if result.returncode: raise SystemExit('Book packaging failed:\n' + result.stdout + result.stderr)
        fixture = json.loads(source.read_text(encoding='utf-8'))
        pdf = (ROOT / 'MarkdownX-Demo/论文阅读示例.pdf').read_bytes()
        fixture['assets']['assets/论文 #%.pdf'] = {'mime': 'application/pdf', 'data': base64.b64encode(pdf).decode()}
        special = '<p><a href="assets/%E8%AE%BA%E6%96%87%20%23%25.pdf#page=2">Special PDF</a></p>'
        fixture['chapters'][0]['content'] = fixture['chapters'][0]['content'].replace('</title>', '</title>' + special, 1)
        source.write_text(json.dumps(fixture, ensure_ascii=False), encoding='utf-8')
        before = hashlib.sha256(source.read_bytes()).digest()
        (root / '.markdownx-workspace-test').touch()
        (root / 'documents').mkdir()
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        exported = root / 'exported.mdxb'
        index = root / 'chapters.md'
        folder = root / 'chapters'
        repacked = root / 'repacked.mdxb'
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\ncheck-book-languages import\nwait 1200\nfollow-book-pdf\nwait 6000\ncheck-media-preview pdf-page2\nkey escape\nwait 500\npick {exported}\ncheck-book-languages export-book\nwait 1800\npick {index}\ncheck-book-languages export-folder\nwait 1800\npick {folder}\nopen-folder\nwait 1200\ncheck-book-languages activate-folder\nwait 600\nfollow-exported-pdf\nwait 6000\ncheck-media-preview pdf-page2\nkey escape\nwait 500\npick {repacked}\ncheck-book-languages package-folder\nwait 1800\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(root), MARKDOWNX_BOOK_TEST_SOURCE=str(source), MARKDOWNX_BOOK_TEST_FOLDER=str(folder), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=75, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(p.read_text(encoding='utf-8', errors='replace') for p in (root / 'logs').glob('*') if p.is_file())
        if result.returncode or logs.count('media-preview: pdf-page2 PASS') < 2 or not exported.is_file() or not repacked.is_file():
            raise SystemExit(f'Book PDF desktop test failed: {result.returncode}\n{result.stdout[-3000:]}\n{result.stderr[-3000:]}\n{logs[-4000:]}')
        book = json.loads(exported.read_text(encoding='utf-8'))
        content = book['chapters'][0]['content']
        assert '#page=2' in content and 'assets/books/' not in content, content
        assert any(base64.b64decode(value['data']).startswith(b'%PDF-') for value in book['assets'].values())
        manifest = json.loads((folder / 'book.json').read_text(encoding='utf-8'))
        chapter_file = folder / manifest['chapters'][0]['file']
        text = chapter_file.read_text(encoding='utf-8')
        match = re.search(r'\[Special PDF\]\(([^)]+)#page=2\)', text)
        assert match, text
        copied = (chapter_file.parent / unquote(match.group(1))).resolve()
        assert copied.is_relative_to(folder) and copied.read_bytes() == pdf, copied
        source_xml = (folder / manifest['chapters'][0]['source']).read_text(encoding='utf-8')
        assert 'assets/books/' not in source_xml and '#page=2' in source_xml, source_xml
        rebuilt = json.loads(repacked.read_text(encoding='utf-8'))
        assert '#page=2' in rebuilt['chapters'][0]['content']
        assert any(base64.b64decode(asset['data']) == pdf for asset in rebuilt['assets'].values())
        assert hashlib.sha256(source.read_bytes()).digest() == before
        print('Desktop book PDF import, page 2, portable folder and repack: PASS')


if __name__ == '__main__':
    main()
