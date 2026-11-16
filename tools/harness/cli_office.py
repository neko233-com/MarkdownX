"""Exercise Pandoc-backed office exports without touching the user's files."""
import argparse
import base64
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import xml.etree.ElementTree as ET
import zipfile

ROOT = Path(__file__).resolve().parents[2]
W = 'http://schemas.openxmlformats.org/wordprocessingml/2006/main'


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if not binary.is_file() or not shutil.which('pandoc'):
        raise SystemExit('Build the release app and put Pandoc on PATH first.')
    hidden = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
    def run(*args, env=None):
        return subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True, encoding='utf-8',
                              errors='replace', timeout=150, creationflags=hidden, env=env)

    with tempfile.TemporaryDirectory(prefix='markdownx-cli-office-') as tmp:
        root = Path(tmp)
        (root / 'shots').mkdir()
        (root / 'shots/frame.png').write_bytes(base64.b64decode(
            'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNk+A8AAQUBAScY42YAAAAASUVORK5CYII='))
        refs = root / 'refs.bib'
        refs.write_text('@article{example2024, author={Doe, Jane}, title={Research Title}, journal={Journal}, year={2024}}\n', encoding='utf-8')
        csl = root / 'custom.csl'
        csl.write_text('''<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" class="in-text">
  <info><title>Test</title><id>https://example.test/style</id><updated>2024-01-01T00:00:00+00:00</updated></info>
  <citation><layout prefix="[" suffix="]"><text value="CUSTOMCITATION"/></layout></citation>
  <bibliography><layout><text variable="title"/></layout></bibliography>
</style>''', encoding='utf-8')
        source = '# 合同与论文\n\n引用示例 [@example2024]。\n\n[资料](./refs.bib#part) [本页](#section) [网络](https://example.test/)\n\n![镜头](./shots/frame.png)\n\n## Section\n'
        markdown = root / 'paper.md'
        markdown.write_text(source, encoding='utf-8')
        docx = root / 'paper.docx'
        result = run('convert', markdown, '-o', docx, '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode or not docx.is_file():
            raise SystemExit(f'DOCX conversion failed: {result.stderr}')
        with zipfile.ZipFile(docx) as archive:
            names = archive.namelist()
            body = archive.read('word/document.xml').decode('utf-8')
            if '合同与论文' not in body or 'research title' not in body.lower() or 'CUSTOMCITATION' not in body or not any(name.startswith('word/media/') for name in names):
                raise SystemExit('DOCX is missing the Chinese text, bibliography or embedded image')
            files = {name: archive.read(name) for name in names}
        styles = files['word/styles.xml'].decode('utf-8')
        normal = re.search(r'<w:style\b[^>]*\bw:styleId="Normal"[^>]*>.*?</w:style>', styles, re.DOTALL)
        if normal is None or '<w:rPr' in normal.group(): raise SystemExit('Unexpected Pandoc Normal style')
        changed = normal.group().replace('</w:style>', '<w:rPr><w:color w:val="123456"/></w:rPr></w:style>')
        files['word/styles.xml'] = (styles[:normal.start()] + changed + styles[normal.end():]).encode('utf-8')
        reference = root / 'reference.docx'
        with zipfile.ZipFile(reference, 'w', zipfile.ZIP_DEFLATED) as archive:
            for name, data in files.items(): archive.writestr(name, data)
        styled = root / 'styled.docx'
        result = run('convert', markdown, '-o', styled, '--strategy', 'pandoc', '--reference-doc', reference)
        if result.returncode: raise SystemExit(f'DOCX template conversion failed: {result.stderr}')
        with zipfile.ZipFile(styled) as archive:
            styles = ET.fromstring(archive.read('word/styles.xml'))
            normal = styles.find(f".//{{{W}}}style[@{{{W}}}styleId='Normal']")
            color = normal.find(f'{{{W}}}rPr/{{{W}}}color') if normal is not None else None
            if color is None or color.get(f'{{{W}}}val') != '123456':
                raise SystemExit('DOCX reference style did not survive export')
        odt = root / 'paper.odt'
        result = run('convert', markdown, '-o', odt, '--strategy', 'pandoc', '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode: raise SystemExit(f'ODT conversion failed: {result.stderr}')
        with zipfile.ZipFile(odt) as archive:
            body = archive.read('content.xml').decode('utf-8')
            if archive.read('mimetype') != b'application/vnd.oasis.opendocument.text' or '合同与论文' not in body or 'research title' not in body.lower() or 'CUSTOMCITATION' not in body or not any(name.startswith('Pictures/') for name in archive.namelist()):
                raise SystemExit('ODT is missing the Chinese text, bibliography or embedded image')
        rtf = root / 'paper.rtf'
        result = run('convert', markdown, '-o', rtf, '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode or not rtf.is_file() or not rtf.read_bytes().startswith(b'{\\rtf'):
            raise SystemExit(f'RTF conversion failed: {result.stderr}')
        rtf_text = rtf.read_bytes()
        if b'research title' not in rtf_text.lower() or b'\\pict' not in rtf_text:
            raise SystemExit('RTF is missing the bibliography or embedded image')
        epub = root / 'paper.epub'
        result = run('convert', markdown, '-o', epub, '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode: raise SystemExit(f'EPUB conversion failed: {result.stderr}')
        with zipfile.ZipFile(epub) as archive:
            names = archive.namelist()
            body = '\n'.join(archive.read(name).decode('utf-8') for name in names if name.endswith('.xhtml'))
            if archive.read('mimetype') != b'application/epub+zip' or '合同与论文' not in body or 'research title' not in body.lower() or not any(name.endswith('.png') for name in names):
                raise SystemExit('EPUB is missing the Chinese text, bibliography or embedded image')
        demo = ROOT / 'MarkdownX-Demo/论文引用示例.md'
        demo_output = root / 'demo.docx'
        result = run('convert', demo, '-o', demo_output, '--citeproc')
        if result.returncode: raise SystemExit(f'Bundled paper demo export failed: {result.stderr}')
        with zipfile.ZipFile(demo_output) as archive:
            body = ''.join(ET.fromstring(archive.read('word/document.xml')).itertext())
            if 'Storyboard and Visual Narration' not in body:
                raise SystemExit('Bundled paper demo did not resolve its relative bibliography')
        bad = root / 'bad.docx'
        if run('convert', markdown, '-o', bad, '--strategy', 'builtin').returncode == 0 or bad.exists():
            raise SystemExit('Unsupported built-in office conversion did not fail safely')
        if run('convert', markdown, '-o', bad, '--bibliography', refs).returncode == 0 or bad.exists():
            raise SystemExit('Bibliography without citation processing did not fail safely')
        missing_env = dict(os.environ, PATH=str(root / 'no-pandoc'), MARKDOWNX_FORMATS_OFFLINE='1', MARKDOWNX_FORMAT_TOOLS_DIR=str(root / 'no-tools'))
        if run('convert', markdown, '-o', bad, env=missing_env).returncode == 0 or bad.exists():
            raise SystemExit('Missing Pandoc did not fail safely')
        if markdown.read_text(encoding='utf-8') != source:
            raise SystemExit('Office export changed the source Markdown')
        (root / 'export').mkdir()
        html = root / 'export/paper-auto.html'
        result = run('convert', markdown, '-o', html, '--citeproc', '--bibliography', refs, '--csl', csl)
        cited_html = html.read_text(encoding='utf-8') if html.exists() else ''
        if result.returncode or 'CUSTOMCITATION' not in cited_html or 'research title' not in cited_html.lower() or 'data:image/png;base64,' not in cited_html:
            raise SystemExit(f'Auto HTML did not preserve citations, bibliography and image: {result.stderr}')
        if 'href="../refs.bib#part"' not in cited_html or 'href="#section"' not in cited_html or 'href="https://example.test/"' not in cited_html:
            raise SystemExit('Auto HTML did not rebase local links while preserving anchors and external links')
        explicit_html = root / 'export/paper-pandoc.html'
        result = run('convert', markdown, '-o', explicit_html, '--strategy', 'pandoc', '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode or 'href="../refs.bib#part"' not in explicit_html.read_text(encoding='utf-8'):
            raise SystemExit(f'Pandoc HTML did not rebase the local link: {result.stderr}')
        pdf = root / 'paper-auto.pdf'
        result = run('convert', markdown, '-o', pdf, '--citeproc', '--bibliography', refs, '--csl', csl)
        if result.returncode or not pdf.is_file() or not pdf.read_bytes().startswith(b'%PDF-'):
            raise SystemExit(f'Auto citation PDF conversion failed: {result.stderr}')
        if os.name == 'nt':
            worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
            if worker.returncode or b'CUSTOMCITATION' not in worker.stdout or b'research title' not in worker.stdout.lower():
                raise SystemExit('Auto PDF did not render citations and bibliography')
        demo_html = root / 'demo-auto.html'
        result = run('convert', demo, '-o', demo_html, '--citeproc')
        demo_output_text = demo_html.read_text(encoding='utf-8') if demo_html.exists() else ''
        if result.returncode or 'Storyboard and Visual Narration' not in demo_output_text:
            raise SystemExit(f'Auto HTML did not resolve the demo bibliography: {result.stderr}')
        if os.name == 'nt' and demo.drive.casefold() != demo_html.drive.casefold():
            target = (demo.parent / '分镜与互动演示.md').resolve().as_uri()
            if f'href="{target}"' not in demo_output_text:
                raise SystemExit('Cross-volume HTML link does not point to the original document')
        demo_pdf = root / 'demo-auto.pdf'
        result = run('convert', demo, '-o', demo_pdf, '--citeproc')
        if result.returncode or not demo_pdf.is_file():
            raise SystemExit(f'Auto PDF did not resolve the demo bibliography: {result.stderr}')
        if shutil.which('typst'):
            typst_pdf = root / 'paper-typst.pdf'
            result = run('convert', markdown, '-o', typst_pdf, '--citeproc', '--bibliography', refs, '--csl', csl, '--pdf-engine', 'typst')
            if result.returncode or not typst_pdf.is_file() or not typst_pdf.read_bytes().startswith(b'%PDF-'):
                raise SystemExit(f'Auto PDF did not select the requested Typst engine: {result.stderr}')
            absolute_typst_pdf = root / 'paper-typst-absolute.pdf'
            result = run('convert', markdown, '-o', absolute_typst_pdf, '--citeproc', '--bibliography', refs, '--csl', csl, '--no-browser', '--pdf-engine', shutil.which('typst'))
            if result.returncode or not absolute_typst_pdf.is_file() or not absolute_typst_pdf.read_bytes().startswith(b'%PDF-'):
                raise SystemExit(f'Absolute Typst engine path lost local images: {result.stderr}')
            if os.name == 'nt':
                worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=typst_pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
                if worker.returncode or b'CUSTOMCITATION' not in worker.stdout or b'research title' not in worker.stdout.lower():
                    raise SystemExit('Typst PDF did not render citations and bibliography')
            if os.name == 'nt' and not shutil.which('pdflatex'):
                bin_dir = root / 'converter-bin'
                bin_dir.mkdir()
                shutil.copyfile(shutil.which('pandoc'), bin_dir / 'pandoc.exe')
                shutil.copyfile(shutil.which('typst'), bin_dir / 'typst.exe')
                isolated = dict(os.environ, PATH=str(bin_dir), PROGRAMFILES=str(root / 'no-browser'),
                                LOCALAPPDATA=str(root / 'no-browser'))
                isolated['PROGRAMFILES(X86)'] = str(root / 'no-browser')
                fallback_pdf = root / 'paper-fallback.pdf'
                result = run('convert', markdown, '-o', fallback_pdf, '--citeproc', '--bibliography', refs, '--csl', csl, '--no-browser', env=isolated)
                if result.returncode or not fallback_pdf.is_file() or not fallback_pdf.read_bytes().startswith(b'%PDF-'):
                    raise SystemExit(f'Auto PDF fallback did not select installed Typst: {result.stderr}')
                worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=fallback_pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
                if worker.returncode or b'CUSTOMCITATION' not in worker.stdout or b'research title' not in worker.stdout.lower():
                    raise SystemExit('Auto PDF fallback lost citations or bibliography')
                shutil.copyfile(shutil.which('pandoc'), bin_dir / 'pdflatex.exe')
                retry_pdf = root / 'paper-retry.pdf'
                result = run('convert', markdown, '-o', retry_pdf, '--citeproc', '--bibliography', refs, '--csl', csl, '--no-browser', env=isolated)
                if result.returncode or not retry_pdf.is_file() or not retry_pdf.read_bytes().startswith(b'%PDF-'):
                    raise SystemExit(f'Auto PDF did not retry Typst after a failed LaTeX engine: {result.stderr}')
                worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=retry_pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
                if worker.returncode or b'CUSTOMCITATION' not in worker.stdout or b'research title' not in worker.stdout.lower():
                    raise SystemExit('Auto PDF retry lost citations or bibliography')
        invalid = root / 'invalid.pdf'
        if run('convert', markdown, '-o', invalid, '--citeproc', '--strategy', 'builtin').returncode == 0 or invalid.exists():
            raise SystemExit('Built-in conversion silently ignored citation processing')
        if run('convert', markdown, '-o', invalid, '--pdf-engine', 'typst', '--browser', 'unused').returncode == 0 or invalid.exists():
            raise SystemExit('Conflicting PDF engine and browser were silently accepted')
        if run('convert', markdown, '-o', invalid, '--no-browser', '--browser', 'unused').returncode == 0 or invalid.exists():
            raise SystemExit('Conflicting browser flags were silently accepted')
        missing_html = root / 'missing-pandoc.html'
        if run('convert', markdown, '-o', missing_html, '--citeproc', env=missing_env).returncode == 0 or missing_html.exists():
            raise SystemExit('Missing Pandoc did not fail safely for auto HTML citations')
        unresolved = root / 'unresolved.md'
        unresolved.write_text('# Paper\n\nMissing [@does-not-exist].\n', encoding='utf-8')
        protected = root / 'protected.html'
        protected.write_bytes(b'keep original output')
        result = run('convert', unresolved, '-o', protected, '--citeproc', '--bibliography', refs, '--force')
        if result.returncode == 0 or protected.read_bytes() != b'keep original output' or 'does-not-exist' not in result.stderr:
            raise SystemExit(f'Unresolved citation did not fail safely: {result.stderr}')
        print('DOCX/ODT/RTF/EPUB, citations, images, reference styles and failure handling: PASS')


if __name__ == '__main__':
    main()
