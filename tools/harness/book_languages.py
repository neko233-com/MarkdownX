"""Round-trip an imported bilingual book through the real desktop export paths."""
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target' / 'release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    subprocess.run([sys.executable, str(ROOT / 'scripts/clean_test_artifacts.py')], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='markdownx-book-ui-') as tmp:
        temp = Path(tmp).resolve()
        (temp / '.markdownx-workspace-test').touch()
        documents = temp / 'documents'
        documents.mkdir()
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        source = temp / 'source.mdxb'
        original = {
            'format': 'markdownx.book', 'version': 1, 'id': 'fixture-book',
            'title': {'zh': 'Fixture Book', 'en': 'English Book'},
            'subtitle': {'zh': '副标题', 'en': 'Subtitle'}, 'authors': ['Author'],
            'languages': ['zh', 'en'], 'default_language': 'zh',
            'chapters': [
                {'id': 'first', 'title': {'zh': '第一章', 'en': 'First'}, 'format': 'markdown',
                 'content': '# 第一章\n\n![图](assets/图片.png)\n', 'content_en': '# First\n\n![image](assets/图片.png)\n'},
                {'id': 'second', 'title': {'zh': '第二章', 'en': 'Second'}, 'format': 'markdown',
                 'content': '# 第二章\n\n正文\n', 'content_en': '# Second\n\nTranslated\n'},
            ],
            'assets': {'assets/图片.png': {'mime': 'application/x-test-image', 'data': base64.b64encode(b'fixture image').decode()}},
        }
        source.write_text(json.dumps(original, ensure_ascii=False), encoding='utf-8')
        exported = temp / 'exported.mdxb'
        index = temp / 'chapters.md'
        folder = temp / 'chapters'
        repacked = temp / 'repacked.mdxb'
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(temp), MARKDOWNX_BOOK_TEST_SOURCE=str(source),
                   MARKDOWNX_BOOK_TEST_FOLDER=str(folder), MARKDOWNX_TRACE_BORROWS='1', FDL_SHOTS=str(temp))
        env.pop('FDL_NOPARK', None)

        def run(name, lines, expected):
            script = temp / (name + '.txt')
            script.write_text(lines, encoding='utf-8')
            result = subprocess.run([str(binary)], cwd=temp, env={**env, 'FDL_SCRIPT': str(script)},
                                    capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=120,
                                    creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
            output = result.stdout + result.stderr
            if result.returncode or any(item not in output for item in expected):
                raise SystemExit(name + ' failed:\n' + output[:6000] + '\n…\n' + output[-3000:])
            return output

        run('import', 'wait 1000\ncheck-book-languages import\nwait 400\nquit\n', ['book-languages: import PASS'])
        output = run('export', f'''wait 1000
check-book-languages reopen
pick {exported}
check-book-languages export-book
wait 800
pick {index}
check-book-languages export-folder
wait 1200
pick {folder}
open-folder
wait 1200
check-book-languages activate-folder
wait 400
pick {repacked}
check-book-languages package-folder
wait 1400
check-book-languages status
quit
''', ['book-languages: reopen PASS', 'book-languages: export-book PASS', 'book-languages: export-folder PASS', 'book-languages: activate-folder PASS', 'book-languages: package-folder PASS'])
        if not repacked.is_file():
            raise SystemExit('Book folder was not repackaged:\n' + output[-6000:])
        if 'borrow trace:' in output:
            raise SystemExit('Application reported a borrow error:\n' + output[:10000])
        exported_book = json.loads(exported.read_text(encoding='utf-8'))
        folder_book = json.loads(repacked.read_text(encoding='utf-8'))
        for book in (exported_book, folder_book):
            assert book['version'] == 2
            assert book['title'] == original['title']
            assert book['subtitle'] == original['subtitle']
            assert book['authors'] == original['authors']
            assert [c['id'] for c in book['chapters']] == ['first', 'second']
            assert [c['content_en'] for c in book['chapters']] == [c['content_en'] for c in original['chapters']]
            assert all(c['format_en'] == 'markdown' for c in book['chapters'])
            assert book['assets']['assets/图片.png'] == original['assets']['assets/图片.png']
        assert index.is_file() and (folder / 'book.json').is_file()
        print('Bilingual book import, restart, export and folder repack: PASS')


if __name__ == '__main__':
    main()
