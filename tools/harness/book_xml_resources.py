"""Package XML chapters and translations through the isolated desktop UI."""
import argparse
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.add_argument('--output', type=Path, help='Optional path to retain the verified book for reader testing')
    args = parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-book-xml-ui-') as tmp:
        temp = Path(tmp).resolve()
        (temp / '.markdownx-workspace-test').touch()
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        documents = temp / 'documents'
        documents.mkdir()
        folder = temp / 'book'
        (folder / 'en').mkdir(parents=True)
        primary = folder / 'story.md'
        xml = folder / 'story.docxxml'
        translation = folder / 'en/story.docxxml'
        primary.write_text('# Story\n\nOriginal Markdown\n', encoding='utf-8')
        chinese = '<title cover="./cover.svg">Story <inline-image src="./shot.svg" alt="shot"/></title><p><a href="./contract.pdf#page=2">Read PDF</a><source path="./contract.pdf" name="contract"/></p><figure view-type="Card"><source path="./contract.pdf" name="contract"/></figure>'
        english = '<title>English <inline-image src="shot.svg" alt="shot"/></title><p><a href="contract.pdf#page=2">Read PDF</a><source path="contract.pdf" name="contract"/></p><figure view-type="Card"><source path="contract.pdf" name="contract"/></figure>'
        xml.write_text(chinese, encoding='utf-8')
        translation.write_text(english, encoding='utf-8')
        def svg(color):
            return f'<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="{color}"/></svg>'.encode()
        pdf = (ROOT / 'MarkdownX-Demo/论文阅读示例.pdf').read_bytes()
        assets = {'shot.svg': svg('red'), 'cover.svg': svg('green'), 'book.svg': svg('purple'),
                  'contract.pdf': pdf, 'en/shot.svg': svg('blue'), 'en/contract.pdf': pdf + b'\n%translated'}
        for name, data in assets.items(): (folder / name).write_bytes(data)
        manifest = {'format': 'markdownx.book', 'version': 1, 'cover': './book.svg', 'chapters': [
            {'file': 'story.md', 'source': 'story.docxxml', 'content_en_file': 'en/story.docxxml'}]}
        (folder / 'book.json').write_text(json.dumps(manifest), encoding='utf-8')
        output = temp / 'result.mdxb'
        steps = temp / 'steps.txt'
        steps.write_text(f'wait 1000\npick {folder}\nopen-folder\nwait 1200\npick {output}\ncheck-book-languages package-folder\nwait 2500\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents), MARKDOWNX_TEST_WORKSPACE_ROOT=str(temp), FDL_SCRIPT=str(steps), FDL_SHOTS=str(temp))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=temp, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode or not output.is_file():
            raise SystemExit(f'Desktop book packaging failed: {result.returncode}\n{result.stderr[-4000:]}')
        book = json.loads(output.read_text(encoding='utf-8'))
        assert len(book['chapters']) == 1, book['chapters']
        chapter = book['chapters'][0]
        assert chapter['format'] == 'docxxml' and chapter.get('format_en', 'docxxml') == 'docxxml'
        assert chapter['content'].count('assets/') >= 4 and chapter['content_en'].count('assets/') >= 3
        decoded = [base64.b64decode(asset['data']) for asset in book['assets'].values()]
        assert all(data in decoded for data in assets.values())
        assert '#page=2' in chapter['content'] and '#page=2' in chapter['content_en']
        assert book['cover'] in book['assets']
        assert xml.read_text(encoding='utf-8') == chinese and translation.read_text(encoding='utf-8') == english
        if args.output: shutil.copyfile(output, args.output)
        print('Desktop XML book, translation, cover and relative assets: PASS')


if __name__ == '__main__':
    main()
