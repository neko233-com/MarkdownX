"""Exercise conditional cell appearance and recalculation in an isolated window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-conditional-') as tmp:
        root = Path(tmp).resolve()
        (root / '.markdownx-workspace-test').touch()
        documents = root / 'documents'
        documents.mkdir()
        library = root / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        for name in ('conditional_appearance.xlsx', 'conditional_oversized.xlsx'):
            shutil.copyfile(ROOT / 'crates/core/tests/fixtures' / name, root / name)
        phases = ['conditional-start', 'conditional-loaded', 'conditional-edited', 'conditional-undone', 'conditional-large-start', 'conditional-large-loaded']
        script = root / 'steps.txt'
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
            raise SystemExit('Conditional-format desktop check failed:\n' + output[-6000:])
        try:
            import openpyxl
        except ModuleNotFoundError:
            raise SystemExit('openpyxl is required for independent XLSX verification')
        book = openpyxl.load_workbook(root / 'conditional_appearance.xlsx', data_only=False)
        sheet = book['Balance']
        if sheet['A1'].value != 50 or sheet['A3'].value != '=A1*4' or len(sheet.conditional_formatting) != 5 or sheet['A2'].border.right.style != 'thick':
            raise SystemExit('Independent conditional-format preservation check failed')
        book.close()
        print('Conditional styles, recalculation, undo and bounded ranges: PASS')


if __name__ == '__main__':
    main()
