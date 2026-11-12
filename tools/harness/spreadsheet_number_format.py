"""Exercise number-format save, undo and redo in an isolated desktop window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-number-format-') as tmp:
        root = Path(tmp).resolve()
        (root / '.markdownx-workspace-test').touch()
        documents = root / 'documents'
        documents.mkdir()
        library = root / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        book = root / 'number_formats.xlsx'
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/number_formats.xlsx', book)
        hidden = root / 'hidden_formulas.xlsx'
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/hidden_formulas.xlsx', hidden)
        shutil.copyfile(hidden, root / 'hidden_new.xlsx')
        appearance = root / 'cell_appearance.xlsx'
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/cell_appearance.xlsx', appearance)
        script = root / 'steps.txt'
        phases = ['number-style-start', 'number-style-apply', 'number-style-saved', 'number-style-undone', 'number-style-redone',
                  'hidden-formula-start', 'hidden-formula-selected', 'hidden-formula-visible',
                  'hidden-new-start', 'hidden-new-write', 'hidden-new-saved', 'hidden-new-undone', 'appearance-start', 'appearance-loaded']
        script.write_text('wait 1200\n' + ''.join(f'check-spreadsheet-preview {phase}\nwait 2500\n' for phase in phases) + 'quit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(root), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=60,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        output = result.stdout + result.stderr
        if result.returncode or any(f'spreadsheet-preview: {phase} PASS' not in output for phase in phases):
            raise SystemExit('Number-format desktop check failed:\n' + output[-6000:])
        try:
            import openpyxl
        except ModuleNotFoundError:
            raise SystemExit('openpyxl is required for independent XLSX verification')
        workbook = openpyxl.load_workbook(book, data_only=False)
        sheet = workbook['Formats']
        if sheet['A1'].value != 0.125 or sheet['A1'].number_format != '0.00' or sheet['B1'].value != '=A1*2' or sheet['B1'].number_format != '0.0%':
            raise SystemExit('Independent XLSX format verification failed')
        workbook.close()
        if hidden.read_bytes() != (ROOT / 'crates/core/tests/fixtures/hidden_formulas.xlsx').read_bytes():
            raise SystemExit('Protected workbook changed during the hidden-formula check')
        if appearance.read_bytes() != (ROOT / 'crates/core/tests/fixtures/cell_appearance.xlsx').read_bytes():
            raise SystemExit('Workbook changed during the appearance check')
        print('Number-format desktop save, undo, redo and XLSX compatibility: PASS')


if __name__ == '__main__':
    main()
