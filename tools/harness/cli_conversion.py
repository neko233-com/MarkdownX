"""Exercise the real command-line HTML and PDF converters in temporary files."""
import argparse
import base64
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if not binary.is_file():
        raise SystemExit('Build the release app first.')
    hidden = getattr(subprocess, 'CREATE_NO_WINDOW', 0)
    def run(*args):
        return subprocess.run([str(binary), *map(str, args)], capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=150, creationflags=hidden)
    version = run('--version')
    if version.returncode or 'MarkdownX' not in version.stdout:
        raise SystemExit(f'CLI version failed: {version.returncode}: {version.stdout!r} {version.stderr!r}')
    with tempfile.TemporaryDirectory(prefix='markdownx-cli-ui-') as tmp:
        root = Path(tmp)
        if os.name == 'nt':
            console = binary.with_name('markdownx-console.exe')
            if not console.is_file(): raise SystemExit('Build the console entry point first.')
            bindir = root / 'bin'
            bindir.mkdir()
            shutil.copyfile(binary, bindir / 'markdownx.exe')
            shutil.copyfile(console, bindir / 'markdownx.com')
            env = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ.get('PATH', ''))
            terminal = subprocess.run(['pwsh', '-NoProfile', '-Command', 'markdownx --version; exit $LASTEXITCODE'], env=env,
                                      capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=20, creationflags=hidden)
            if terminal.returncode or 'MarkdownX' not in terminal.stdout:
                raise SystemExit(f'PowerShell entry point failed: {terminal.returncode}: {terminal.stdout!r} {terminal.stderr!r}')
        (root / 'shots').mkdir()
        image = root / 'shots/图 1.svg'
        image.write_text('<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20"><rect width="40" height="20" fill="red"/></svg>', encoding='utf-8')
        (root / 'game.js').write_text("document.body.dataset.ready='yes';", encoding='utf-8')
        source = '# Story\n\n合同文本\n\n| Shot | Frame |\n| --- | --- |\n| 1 | ![Frame](./shots/%E5%9B%BE%201.svg) |\n| 2 | ![Frame](shots/%E5%9B%BE%201.svg) |\n\n<canvas></canvas>\n<script src="./game.js"></script>\n'
        markdown = root / 'story.md'
        markdown.write_text(source, encoding='utf-8')
        html = root / 'story.html'
        result = run('convert', markdown, '--to', 'html', '--strategy', 'builtin')
        if result.returncode or not html.is_file():
            raise SystemExit(f'HTML conversion failed: {result.returncode}: {result.stdout!r} {result.stderr!r}')
        text = html.read_text(encoding='utf-8')
        if text.count('data:image/svg+xml;base64,') != 2 or 'data:text/javascript;base64,' not in text:
            raise SystemExit('HTML did not embed the relative images and script')
        storyboard = root / 'storyboard.md'
        shutil.copyfile(ROOT / 'MarkdownX-Demo/storyboard-shots/01.svg', root / 'storyboard.svg')
        storyboard.write_text('# Storyboard\n\n![shot](./storyboard.svg)\n', encoding='utf-8')
        result = run('convert', storyboard, '--to', 'html', '--strategy', 'builtin')
        storyboard_html = (root / 'storyboard.html').read_text(encoding='utf-8') if (root / 'storyboard.html').exists() else ''
        storyboard_match = re.search(r'data:image/svg\+xml;base64,([A-Za-z0-9+/=]+)', storyboard_html)
        if result.returncode or not storyboard_match or '01 / 持表' not in base64.b64decode(storyboard_match.group(1)).decode('utf-8'):
            raise SystemExit(f'Export lost storyboard SVG text: {result.stderr!r}')
        pixels = b'\0\0\xff' * 32 * 24
        bmp_bytes = (struct.pack('<2sIHHI', b'BM', 54 + len(pixels), 0, 0, 54)
                     + struct.pack('<IiiHHIIiiII', 40, 32, 24, 1, 24, 0, len(pixels), 0, 0, 0, 0) + pixels)
        bmp_uri = 'data:image/bmp;base64,' + base64.b64encode(bmp_bytes).decode('ascii')
        svg_data = '<svg xmlns="http://www.w3.org/2000/svg" width="80" height="40"><image href="file:///secret.png" width="10" height="10"/><rect width="80" height="40" fill="#00ff00"/><text x="3" y="25" font-size="18" fill="#0000ff">镜头01</text></svg>'.encode('utf-8')
        svg_uri = 'data:image/svg+xml;base64,' + base64.b64encode(svg_data).decode('ascii')
        embedded = root / 'embedded.md'
        embedded_source = f'# Embedded\n\n![BMP]({bmp_uri})\n\n| Shot | Frame |\n| --- | --- |\n| 1 | before ![SVG]({svg_uri}) after |\n'
        embedded.write_text(embedded_source, encoding='utf-8')
        result = run('convert', embedded, '--to', 'html', '--strategy', 'builtin')
        rendered = (root / 'embedded.html').read_text(encoding='utf-8') if (root / 'embedded.html').exists() else ''
        match = re.search(r'data:image/svg\+xml;base64,([A-Za-z0-9+/=]+)', rendered)
        safe_svg = base64.b64decode(match.group(1)) if match else b''
        if result.returncode or bmp_uri not in rendered or b'secret.png' in safe_svg or '镜头01' not in safe_svg.decode('utf-8'):
            raise SystemExit(f'Embedded image HTML export failed: {result.stderr!r}')
        if embedded.read_text(encoding='utf-8') != embedded_source:
            raise SystemExit('Embedded image export changed the Markdown source')
        embedded_pdf = root / 'embedded.pdf'
        result = run('convert', embedded, '-o', embedded_pdf, '--strategy', 'builtin')
        if result.returncode or not embedded_pdf.is_file() or not embedded_pdf.read_bytes().startswith(b'%PDF-'):
            raise SystemExit(f'Embedded image PDF export failed: {result.stderr!r}')
        if shutil.which('pdftoppm'):
            raster = root / 'embedded-page'
            raster_result = subprocess.run(['pdftoppm', '-f', '1', '-l', '1', '-r', '144', '-singlefile', str(embedded_pdf), str(raster)],
                                           capture_output=True, timeout=30, creationflags=hidden)
            if raster_result.returncode or not raster.with_suffix('.ppm').is_file():
                raise SystemExit('Could not rasterize embedded image PDF')
            rgb = raster.with_suffix('.ppm').read_bytes().split(b'\n', 3)[3]
            red = green = blue = 0
            for r, g, b in zip(rgb[0::3], rgb[1::3], rgb[2::3]):
                red += r > 220 and g < 40 and b < 40
                green += g > 220 and r < 40 and b < 40
                blue += b > 200 and r < 60 and g < 60
            if red < 20 or green < 20 or blue < 20:
                raise SystemExit('Embedded BMP, SVG or SVG text is missing from rendered PDF')
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例视频.mp4', root / 'clip.mp4')
        shutil.copyfile(ROOT / 'MarkdownX-Demo/示例音频.wav', root / 'sound.wav')
        (root / 'poster.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="60"><rect width="100" height="60" fill="#ff0000"/></svg>', encoding='utf-8')
        print_doc = root / 'print-media.md'
        print_doc.write_text('# Print\n\n<video controls src="./clip.mp4" poster="./poster.svg"></video>\n\n<audio controls><source src="sound.wav" type="audio/wav"></audio>\n\n<canvas width="100" height="60" style="background:#0000ff"></canvas>\n', encoding='utf-8')
        print_pdf = root / 'print-media.pdf'
        result = run('convert', print_doc, '-o', print_pdf, '--strategy', 'builtin')
        if result.returncode or not print_pdf.is_file():
            raise SystemExit(f'Interactive PDF conversion failed: {result.stderr!r}')
        if os.name == 'nt':
            worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=print_pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
            if worker.returncode or not (b'Audio' in worker.stdout or '音频'.encode() in worker.stdout):
                raise SystemExit('Audio playback note is missing from the PDF')
        if shutil.which('pdftoppm'):
            page = root / 'print-media-page'
            raster_result = subprocess.run(['pdftoppm', '-f', '1', '-l', '1', '-r', '100', '-singlefile', str(print_pdf), str(page)],
                                           capture_output=True, timeout=30, creationflags=hidden)
            if raster_result.returncode or not page.with_suffix('.ppm').is_file():
                raise SystemExit('Could not rasterize interactive PDF')
            rgb = page.with_suffix('.ppm').read_bytes().split(b'\n', 3)[3]
            red = blue = 0
            for r, g, b in zip(rgb[0::3], rgb[1::3], rgb[2::3]):
                red += r > 220 and g < 40 and b < 40
                blue += b > 220 and r < 40 and g < 40
            if red < 200 or blue < 200:
                raise SystemExit('Video poster or Canvas background is missing from rendered PDF')
        table_game = root / 'table-game.md'
        table_game.write_text('# Table game\n\n| Shot | Game |\n| --- | --- |\n| 1 | <canvas></canvas><script src="./game.js"></script> |\n', encoding='utf-8')
        result = run('convert', table_game, '--to', 'html', '--strategy', 'builtin')
        table_html = (root / 'table-game.html').read_text(encoding='utf-8') if (root / 'table-game.html').exists() else ''
        if result.returncode or 'data:text/javascript;base64,' not in table_html or '<iframe' not in table_html:
            raise SystemExit(f'Table Canvas script was not embedded in HTML: {result.stderr!r}')
        (root / '合同 1.md').write_text('# Contract', encoding='utf-8')
        links = root / 'links.md'
        links.write_text('# Links\n\n[合同](./%E5%90%88%E5%90%8C%201.md#%E6%9D%A1%E6%AC%BE)\n[Here](#目标-标题)\n\n## 目标 标题\n', encoding='utf-8')
        nested_html = root / 'export/links.html'
        result = run('convert', links, '-o', nested_html, '--strategy', 'builtin')
        if result.returncode or 'href="../%E5%90%88%E5%90%8C%201.md#%E6%9D%A1%E6%AC%BE"' not in nested_html.read_text(encoding='utf-8') or 'id="目标-标题"' not in nested_html.read_text(encoding='utf-8'):
            raise SystemExit(f'Exported relative link was not rebased: {result.stderr!r}')
        invalid = run('convert', links, '--to', 'html', '-o', root / 'invalid.html', '--pdf-engine', 'typst')
        if invalid.returncode == 0 or (root / 'invalid.html').exists():
            raise SystemExit('PDF engine was silently ignored for an incompatible conversion')
        invalid = run('convert', links, '--to', 'html', '-o', root / 'invalid.html', '--browser', 'missing-browser')
        if invalid.returncode == 0 or (root / 'invalid.html').exists():
            raise SystemExit('Browser was silently ignored for an incompatible conversion')
        pdf = root / 'story.pdf'
        result = run('convert', markdown, '-o', pdf, '--strategy', 'auto')
        if result.returncode or not pdf.is_file() or not pdf.read_bytes().startswith(b'%PDF-'):
            raise SystemExit(f'PDF conversion failed: {result.returncode}: {result.stdout!r} {result.stderr!r}')
        if os.name == 'nt':
            worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=pdf.read_bytes(), capture_output=True, timeout=20, creationflags=hidden)
            if worker.returncode or b'Story' not in worker.stdout or '合同文本'.encode() not in worker.stdout:
                raise SystemExit(f'PDF text check failed: {worker.returncode}, output={worker.stdout[:1200]!r}, error={worker.stderr[:1200]!r}')
        original = pdf.read_bytes()
        if run('convert', markdown, '-o', pdf).returncode == 0 or pdf.read_bytes() != original:
            raise SystemExit('Existing output was replaced without --force')
        image.unlink()
        if run('convert', markdown, '-o', pdf, '--force').returncode == 0 or pdf.read_bytes() != original:
            raise SystemExit('Failed conversion replaced an existing PDF')
        if markdown.read_text(encoding='utf-8') != source:
            raise SystemExit('Conversion changed the source Markdown')
        (root / 'part').mkdir()
        nested = root / 'part/nested.md'
        nested.write_text('# Nested\n\n![outside](../shots/frame.svg)\n', encoding='utf-8')
        (root / 'shots/frame.svg').write_text('<svg xmlns="http://www.w3.org/2000/svg"/>', encoding='utf-8')
        if run('convert', nested, '--to', 'html').returncode == 0:
            raise SystemExit('Default asset root allowed a parent directory')
        result = run('convert', nested, '--to', 'html', '--root', root)
        if result.returncode or 'data:image/svg+xml;base64,' not in (root / 'part/nested.html').read_text(encoding='utf-8'):
            raise SystemExit('Explicit root did not resolve a parent-relative image')
        if shutil.which('pandoc') is None:
            failed = run('convert', markdown, '--to', 'html', '-o', root / 'pandoc.html', '--strategy', 'pandoc')
            if failed.returncode == 0 or (root / 'pandoc.html').exists():
                raise SystemExit('Missing Pandoc did not produce a safe failure')
        demo = root / 'demo'
        shutil.copytree(ROOT / 'MarkdownX-Demo', demo)
        demo_source = demo / '分镜与互动演示.md'
        before = demo_source.read_bytes()
        for suffix in ('html', 'pdf'):
            output = demo / ('export.' + suffix)
            result = run('convert', demo_source, '-o', output, '--strategy', 'builtin')
            if result.returncode or not output.is_file():
                raise SystemExit(f'Full storyboard {suffix} conversion failed: {result.stderr!r}')
        rendered = (demo / 'export.html').read_text(encoding='utf-8')
        if rendered.count('data:image/svg+xml;base64,') < 10 or rendered.count('<iframe') < 6 or 'data:text/javascript;base64,' not in rendered:
            raise SystemExit('Full storyboard export is missing images, media or external scripts')
        if demo_source.read_bytes() != before:
            raise SystemExit('Full storyboard conversion changed its source')
        print('CLI HTML/PDF conversion, embedded assets, output protection and missing strategy: PASS')


if __name__ == '__main__':
    main()
