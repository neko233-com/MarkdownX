"""Verify formula text is not mistaken for assets during desktop book packaging."""
import argparse
import base64
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools first.')
    with tempfile.TemporaryDirectory(prefix='markdownx-math-assets-ui-') as tmp:
        root = Path(tmp).resolve()
        (root / '.markdownx-workspace-test').touch()
        (root / 'documents').mkdir()
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        folder = root / 'book'
        folder.mkdir()
        svg = b'<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10"/></svg>'
        (folder / 'real.svg').write_bytes(svg)
        source = ('# Story\n\n' + r'$\text{![fake](missing.png)}$' + '\n\n'
                  + r'$\text{![ref][shot]}$' + '\n\n$$\n' + r'\text{![block][shot]}' + '\n$$\n\n'
                  + '> $$\n> ' + r'\text{![quote][shot]}' + '\n> $$\n\n'
                  + '![real](./real.svg)\n\n[shot]: missing.png\n')
        english = source.replace('# Story', '# English')
        primary, translated = folder / 'story.md', folder / 'story.en.md'
        primary.write_text(source, encoding='utf-8')
        translated.write_text(english, encoding='utf-8')
        output = root / 'book.mdxb'
        script = root / 'steps.txt'
        script.write_text(f'wait 1000\npick {folder}\nopen-folder\nwait 1200\npick {output}\ncheck-book-languages package-folder\nwait 2200\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), MARKDOWNX_TEST_WORKSPACE_ROOT=str(root),
                   FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=40, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode or not output.is_file():
            raise SystemExit(f'Desktop math packaging failed: {result.returncode}\n{result.stdout[-1500:]}\n{result.stderr[-2500:]}')
        book = json.loads(output.read_text(encoding='utf-8'))
        chapter = book['chapters'][0]
        document = ET.fromstring('<root>' + chapter['content'] + '</root>')
        formulas = [''.join(element.itertext()) for element in document.iter('latex')]
        for expected in (r'\text{![fake](missing.png)}', r'\text{![ref][shot]}', r'\text{![block][shot]}', r'\text{![quote][shot]}'):
            if expected not in formulas: raise SystemExit(f'Formula changed during packaging: {expected!r}; {formulas!r}')
        body = chapter.get('content_en', '')
        for expected in (r'$\text{![fake](missing.png)}$', r'$\text{![ref][shot]}$', r'\text{![block][shot]}', r'\text{![quote][shot]}'):
            if expected not in body: raise SystemExit(f'Translated formula changed: {expected!r}\n{body}')
        assets = [base64.b64decode(item['data']) for item in book['assets'].values()]
        if assets != [svg]: raise SystemExit(f'Wrong embedded assets: {len(assets)}')
        if primary.read_text(encoding='utf-8') != source or translated.read_text(encoding='utf-8') != english:
            raise SystemExit('Packaging changed the source files')
        html = root / 'story.html'
        result = subprocess.run([str(binary), 'convert', str(primary), '-o', str(html), '--strategy', 'builtin'], capture_output=True,
                                text=True, encoding='utf-8', errors='replace', timeout=45, creationflags=subprocess.CREATE_NO_WINDOW)
        if result.returncode: raise SystemExit(f'CLI math export failed: {result.stderr}')
        rendered = html.read_text(encoding='utf-8')
        if '![ref][shot]' not in rendered or '![block][shot]' not in rendered or '![quote][shot]' not in rendered:
            raise SystemExit('CLI math export changed literal reference text')
    print('Desktop bilingual math packaging and CLI reference preservation: PASS')


if __name__ == '__main__':
    main()
