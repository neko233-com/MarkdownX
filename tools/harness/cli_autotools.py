"""Verify fresh Windows converter downloads, cached reuse and offline failure."""
import argparse
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file():
        raise SystemExit('Build the Windows release application before running this check.')
    hidden = subprocess.CREATE_NO_WINDOW
    with tempfile.TemporaryDirectory(prefix='markdownx-autotools-') as tmp:
        root = Path(tmp)
        source = root / '论文.md'
        original = '# Research\n\nA result [@example].\n\n# References\n'
        source.write_text(original, encoding='utf-8')
        refs = root / 'refs.bib'
        refs.write_text('@article{example, author={Doe, Jane}, title={Example Research}, year={2024}}\n', encoding='utf-8')
        system = Path(os.environ.get('SystemRoot', r'C:\Windows'))
        tools = root / '格式工具 with spaces'
        env = dict(os.environ, PATH=str(system / 'System32'), MARKDOWNX_HOME=str(root / 'library'),
                   MARKDOWNX_FORMAT_TOOLS_DIR=str(tools))
        env.pop('MARKDOWNX_FORMATS_OFFLINE', None)
        def command(output, *options):
            return [str(binary), 'convert', str(source), '-o', str(output), '--citeproc', '--bibliography', str(refs), *options]
        def run(output, *options, environment=env):
            return subprocess.run(command(output, *options), env=environment,
                                  capture_output=True, text=True, encoding='utf-8', errors='replace',
                                  timeout=600, creationflags=hidden)
        docx = root / 'paper.docx'
        parallel = root / 'parallel.docx'
        processes = [subprocess.Popen(command(output), env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                      text=True, encoding='utf-8', errors='replace', creationflags=hidden)
                     for output in (docx, parallel)]
        results = [(process, *process.communicate(timeout=600)) for process in processes]
        if any(process.returncode for process, _, _ in results) or not (tools / 'pandoc/pandoc.exe').is_file() or not (tools / 'pandoc/COPYING.rtf').is_file():
            raise SystemExit(f'Concurrent first-use Pandoc setup failed: {[(p.returncode, err) for p, _, err in results]}')
        if sum(err.count('downloading pandoc') for _, _, err in results) != 1 or not parallel.is_file():
            raise SystemExit('Concurrent conversions did not share one Pandoc download')
        with zipfile.ZipFile(docx) as archive:
            if b'Example Research' not in archive.read('word/document.xml'):
                raise SystemExit('DOCX lost the bibliography')
        cached = root / 'cached.docx'
        offline = dict(env, MARKDOWNX_FORMATS_OFFLINE='1')
        result = run(cached, environment=offline)
        if result.returncode or 'downloading' in result.stderr.lower() or not cached.is_file():
            raise SystemExit(f'Offline cached Pandoc failed: {result.stderr}')
        pdf = root / 'paper.pdf'
        result = run(pdf, '--no-browser')
        if result.returncode or not pdf.is_file() or not pdf.read_bytes().startswith(b'%PDF-') or not (tools / 'typst/LICENSE').is_file():
            raise SystemExit(f'Automatic PDF fallback and first-use Typst setup failed: code={result.returncode}, pdf={pdf.is_file()}, typst={(tools / "typst/typst.exe").is_file()}, stdout={result.stdout}, stderr={result.stderr}')
        worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=pdf.read_bytes(),
                                capture_output=True, timeout=30, creationflags=hidden)
        if worker.returncode or b'Example Research' not in worker.stdout:
            raise SystemExit('Typst PDF lost the bibliography')
        explicit = root / 'explicit.pdf'
        result = run(explicit, '--pdf-engine', 'typst', environment=offline)
        if result.returncode or not explicit.read_bytes().startswith(b'%PDF-'):
            raise SystemExit(f'Offline cached Typst failed: {result.stderr}')
        missing = root / 'offline.docx'
        missing_env = dict(offline, MARKDOWNX_FORMAT_TOOLS_DIR=str(root / 'missing-tools'))
        result = run(missing, environment=missing_env)
        if result.returncode == 0 or missing.exists() or 'downloads are disabled' not in result.stderr:
            raise SystemExit(f'Offline missing-tool failure was unsafe: {result.stderr}')
        damaged = root / 'damaged-tools/pandoc'
        damaged.mkdir(parents=True)
        (damaged / 'keep.txt').write_text('keep', encoding='utf-8')
        conflict = root / 'conflict.docx'
        result = run(conflict, environment=dict(env, MARKDOWNX_FORMAT_TOOLS_DIR=str(damaged.parent)))
        if result.returncode == 0 or conflict.exists() or (damaged / 'keep.txt').read_text(encoding='utf-8') != 'keep':
            raise SystemExit(f'Incomplete tool directory was modified: {result.stderr}')
        if source.read_text(encoding='utf-8') != original:
            raise SystemExit('Conversion modified the source Markdown')
        subprocess.run([sys.executable, str(ROOT / 'scripts/clean_test_artifacts.py')], check=True, creationflags=hidden)
        suite_env = dict(offline, PATH=os.pathsep.join([str(tools / 'pandoc'), str(tools / 'typst'), os.environ.get('PATH', '')]))
        suite = subprocess.run([sys.executable, str(ROOT / 'tools/harness/cli_office.py'), '-Quiet'], env=suite_env,
                               capture_output=True, text=True, encoding='utf-8', errors='replace', timeout=600, creationflags=hidden)
        if suite.returncode:
            raise SystemExit(f'Office and citation regression failed: {suite.stdout}\n{suite.stderr}')
        print(suite.stdout.strip())
        print('Concurrent first-use Pandoc, Typst, cached offline reuse and safe failure: PASS')


if __name__ == '__main__':
    main()
