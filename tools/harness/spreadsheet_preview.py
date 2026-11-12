"""Exercise XLSX preview in a temporary workspace and an off-screen window."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-spreadsheet-ui-') as tmp:
        temp = Path(tmp).resolve()
        (temp / '.markdownx-workspace-test').touch()
        documents = temp / 'documents'
        documents.mkdir()
        library = temp / 'library'
        shutil.copytree(ROOT / 'tools/harness/lib_clean', library)
        fixture = temp / 'fixture.xlsx'
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/spreadsheet.xlsx', fixture)
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/calculation.xlsx', temp / 'calculation.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/protected_ranges.xlsx', temp / 'protected_ranges.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/number_formats.xlsx', temp / 'number_formats.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/dates_1904.xlsx', temp / 'dates_1904.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/replacements.xlsx', temp / 'replacements.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/arithmetic.xlsx', temp / 'arithmetic.xlsx')
        shutil.copyfile(ROOT / 'crates/core/tests/fixtures/sheet_protection.xlsx', temp / 'sheet_protection.xlsx')
        script = temp / 'steps.txt'
        primary = 'ctrl' if os.name == 'nt' else 'cmd'
        replace_shortcut = 'ctrl-h' if os.name == 'nt' else 'cmd-shift-h'
        script.write_text('wait 1200\ncheck-spreadsheet-preview start\nwait 1400\ncheck-spreadsheet-preview done\nwait 1600\ncheck-spreadsheet-preview saved\nwait 1600\ncheck-spreadsheet-preview undone\nwait 1600\ncheck-spreadsheet-preview redone\ncheck-spreadsheet-preview jump\nwait 300\ncheck-spreadsheet-preview jump-done\ncheck-spreadsheet-preview nav-start\nwait 1600\ncheck-spreadsheet-preview nav-done\nkey tab\nwait 300\ncheck-spreadsheet-preview nav-key\n' + f'key {primary}-z\nwait 1600\ncheck-spreadsheet-preview key-undone\nkey {primary}-y\nwait 1600\ncheck-spreadsheet-preview key-redone\n' + 'check-spreadsheet-preview paste\nwait 1600\ncheck-spreadsheet-preview pasted\nwait 1600\ncheck-spreadsheet-preview paste-undone\nwait 1600\ncheck-spreadsheet-preview paste-redone\n' + f'key {primary}-f\nwait 300\n' + 'check-spreadsheet-preview find-start\nwait 300\ncheck-spreadsheet-preview find-done\ncheck-spreadsheet-preview clear-start\nwait 1600\ncheck-spreadsheet-preview clear-saved\nwait 1600\ncheck-spreadsheet-preview clear-undone\nwait 1600\ncheck-spreadsheet-preview fill-saved\nwait 1600\ncheck-spreadsheet-preview fill-undone\ncheck-spreadsheet-preview conflict\nwait 1600\ncheck-spreadsheet-preview verify\n' + f'pick {temp / "recovered.xlsx"}\ncheck-spreadsheet-preview copy\nwait 1600\ncheck-spreadsheet-preview copy-done\ncheck-spreadsheet-preview replace-start\nresize 600 650\nwait 2000\ncheck-spreadsheet-preview replace-loaded\n' + f'key {replace_shortcut}\n' + 'check-spreadsheet-preview replace-text\nwait 2000\ncheck-spreadsheet-preview replace-text-saved\nwait 2000\ncheck-spreadsheet-preview replace-text-undone\nwait 2000\ncheck-spreadsheet-preview replace-text-redone\nwait 2000\ncheck-spreadsheet-preview replace-formula-saved\nwait 2000\ncheck-spreadsheet-preview replace-formula-undone\nwait 2000\ncheck-spreadsheet-preview replace-formula-redone\nwait 2500\ncheck-spreadsheet-preview replace-boolean-saved\nwait 2500\ncheck-spreadsheet-preview replace-percentage-saved\nwait 2500\ncheck-spreadsheet-preview replace-date-saved\nwait 2500\ncheck-spreadsheet-preview replace-date-undone\nwait 2500\ncheck-spreadsheet-preview replace-date-redone\nresize 1280 800\ncheck-spreadsheet-preview arithmetic-start\nwait 2500\ncheck-spreadsheet-preview arithmetic-multiply\nwait 2500\ncheck-spreadsheet-preview arithmetic-multiplied\nwait 2500\ncheck-spreadsheet-preview arithmetic-undone\nwait 2500\ncheck-spreadsheet-preview arithmetic-redone\nwait 2500\ncheck-spreadsheet-preview arithmetic-added\nwait 2500\ncheck-spreadsheet-preview arithmetic-add-undone\nwait 2500\ncheck-spreadsheet-preview arithmetic-add-redone\n' + 'check-spreadsheet-preview calc-start\nwait 2000\ncheck-spreadsheet-preview calc-loaded\nwait 2500\ncheck-spreadsheet-preview calc-after\nwait 1600\ncheck-spreadsheet-preview formula-fill-saved\nwait 1600\ncheck-spreadsheet-preview formula-fill-undone\nwait 1600\ncheck-spreadsheet-preview clipboard-pasted\nwait 1600\ncheck-spreadsheet-preview clipboard-undone\nwait 1600\ncheck-spreadsheet-preview clipboard-cross-saved\nwait 1600\ncheck-spreadsheet-preview clipboard-cross-undone\nwait 1600\ncheck-spreadsheet-preview sheet-reference-refused\nwait 1600\ncheck-spreadsheet-preview sheet-delete-refused\ncheck-spreadsheet-preview protected-start\nwait 1600\ncheck-spreadsheet-preview protected-edit\nwait 1600\ncheck-spreadsheet-preview protected-check\nwait 2000\ncheck-spreadsheet-preview protected-replace-refused\ncheck-spreadsheet-preview sheet-protection-start\nwait 2500\ncheck-spreadsheet-preview sheet-protection-locked\nwait 2500\ncheck-spreadsheet-preview sheet-protection-blocked\nwait 2500\ncheck-spreadsheet-preview sheet-protection-unlocked\nwait 2500\ncheck-spreadsheet-preview sheet-protection-batch-blocked\nwait 2500\ncheck-spreadsheet-preview sheet-protection-pasted\nwait 2500\ncheck-spreadsheet-preview sheet-protection-row-saved\ncheck-spreadsheet-preview format-start\nwait 2500\ncheck-spreadsheet-preview format-loaded\nwait 2500\ncheck-spreadsheet-preview format-saved\nwait 2500\ncheck-spreadsheet-preview format-pasted\nwait 2500\ncheck-spreadsheet-preview format-explicit-saved\nwait 2500\ncheck-spreadsheet-preview format-column-saved\nwait 2500\ncheck-spreadsheet-preview format-row-saved\nwait 2500\ncheck-spreadsheet-preview format-date-saved\nwait 2500\ncheck-spreadsheet-preview format-date-undone\nwait 2500\ncheck-spreadsheet-preview format-date-redone\nwait 2500\ncheck-spreadsheet-preview format-blank-date-saved\nwait 2500\ncheck-spreadsheet-preview format-inherited-date-saved\nwait 2500\ncheck-spreadsheet-preview format-date-cleared\nwait 2500\ncheck-spreadsheet-preview format-date-restored\ncheck-spreadsheet-preview date-1904-start\nwait 2500\ncheck-spreadsheet-preview date-1904-loaded\nwait 2500\ncheck-spreadsheet-preview date-1904-saved\nwait 2500\ncheck-spreadsheet-preview date-1904-blank-saved\ncheck-spreadsheet-preview new\nwait 2000\ncheck-spreadsheet-preview new-done\nwait 300\ncheck-spreadsheet-preview sheet-prompt\nwait 1600\ncheck-spreadsheet-preview sheet-added\nwait 1600\ncheck-spreadsheet-preview sheet-edited\nwait 1600\ncheck-spreadsheet-preview sheet-renamed\nwait 1600\ncheck-spreadsheet-preview sheet-delete-setup\nwait 1600\ncheck-spreadsheet-preview sheet-delete-confirm\nwait 1600\ncheck-spreadsheet-preview sheet-delete-done\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(library), MARKDOWNX_LOG_DIR=str(temp / 'logs'),
                   MARKDOWNX_TEST_STARTUP_WORKSPACE='1', MARKDOWNX_TEST_DOCUMENTS_DIR=str(documents),
                   MARKDOWNX_TEST_WORKSPACE_ROOT=str(temp), FDL_SCRIPT=str(script), FDL_SHOTS=str(temp))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=temp, env=env, capture_output=True, text=True,
                                encoding='utf-8', errors='replace', timeout=290,
                                creationflags=subprocess.CREATE_NO_WINDOW if os.name == 'nt' else 0)
        output = result.stdout + result.stderr
        if result.returncode or any(x not in output for x in ('spreadsheet-preview: start PASS', 'spreadsheet-preview: done PASS', 'spreadsheet-preview: saved PASS', 'spreadsheet-preview: undone PASS', 'spreadsheet-preview: redone PASS', 'spreadsheet-preview: jump PASS', 'spreadsheet-preview: jump-done PASS', 'spreadsheet-preview: nav-start PASS', 'spreadsheet-preview: nav-done PASS', 'spreadsheet-preview: nav-key PASS', 'spreadsheet-preview: key-undone PASS', 'spreadsheet-preview: key-redone PASS', 'spreadsheet-preview: paste PASS', 'spreadsheet-preview: pasted PASS', 'spreadsheet-preview: paste-undone PASS', 'spreadsheet-preview: paste-redone PASS', 'spreadsheet-preview: find-start PASS', 'spreadsheet-preview: find-done PASS', 'spreadsheet-preview: clear-start PASS', 'spreadsheet-preview: clear-saved PASS', 'spreadsheet-preview: clear-undone PASS', 'spreadsheet-preview: fill-saved PASS', 'spreadsheet-preview: fill-undone PASS', 'spreadsheet-preview: conflict PASS', 'spreadsheet-preview: verify PASS', 'spreadsheet-preview: copy PASS', 'spreadsheet-preview: copy-done PASS', 'spreadsheet-preview: replace-start PASS', 'spreadsheet-preview: replace-loaded PASS', 'spreadsheet-preview: replace-text PASS', 'spreadsheet-preview: replace-text-saved PASS', 'spreadsheet-preview: replace-text-undone PASS', 'spreadsheet-preview: replace-text-redone PASS', 'spreadsheet-preview: replace-formula-saved PASS', 'spreadsheet-preview: replace-formula-undone PASS', 'spreadsheet-preview: replace-formula-redone PASS', 'spreadsheet-preview: replace-boolean-saved PASS', 'spreadsheet-preview: replace-percentage-saved PASS', 'spreadsheet-preview: replace-date-saved PASS', 'spreadsheet-preview: replace-date-undone PASS', 'spreadsheet-preview: replace-date-redone PASS', 'spreadsheet-preview: arithmetic-start PASS', 'spreadsheet-preview: arithmetic-multiply PASS', 'spreadsheet-preview: arithmetic-multiplied PASS', 'spreadsheet-preview: arithmetic-undone PASS', 'spreadsheet-preview: arithmetic-redone PASS', 'spreadsheet-preview: arithmetic-added PASS', 'spreadsheet-preview: arithmetic-add-undone PASS', 'spreadsheet-preview: arithmetic-add-redone PASS', 'spreadsheet-preview: calc-loaded PASS', 'spreadsheet-preview: calc-after PASS', 'spreadsheet-preview: formula-fill-saved PASS', 'spreadsheet-preview: formula-fill-undone PASS', 'spreadsheet-preview: clipboard-pasted PASS', 'spreadsheet-preview: clipboard-undone PASS', 'spreadsheet-preview: clipboard-cross-saved PASS', 'spreadsheet-preview: clipboard-cross-undone PASS', 'spreadsheet-preview: sheet-reference-refused PASS', 'spreadsheet-preview: sheet-delete-refused PASS', 'spreadsheet-preview: protected-start PASS', 'spreadsheet-preview: protected-edit PASS', 'spreadsheet-preview: protected-check PASS', 'spreadsheet-preview: protected-replace-refused PASS', 'spreadsheet-preview: sheet-protection-start PASS', 'spreadsheet-preview: sheet-protection-locked PASS', 'spreadsheet-preview: sheet-protection-blocked PASS', 'spreadsheet-preview: sheet-protection-unlocked PASS', 'spreadsheet-preview: sheet-protection-batch-blocked PASS', 'spreadsheet-preview: sheet-protection-pasted PASS', 'spreadsheet-preview: sheet-protection-row-saved PASS', 'spreadsheet-preview: format-start PASS', 'spreadsheet-preview: format-loaded PASS', 'spreadsheet-preview: format-saved PASS', 'spreadsheet-preview: format-pasted PASS', 'spreadsheet-preview: format-explicit-saved PASS', 'spreadsheet-preview: format-column-saved PASS', 'spreadsheet-preview: format-row-saved PASS', 'spreadsheet-preview: format-date-saved PASS', 'spreadsheet-preview: format-date-undone PASS', 'spreadsheet-preview: format-date-redone PASS', 'spreadsheet-preview: format-blank-date-saved PASS', 'spreadsheet-preview: format-inherited-date-saved PASS', 'spreadsheet-preview: format-date-cleared PASS', 'spreadsheet-preview: format-date-restored PASS', 'spreadsheet-preview: date-1904-start PASS', 'spreadsheet-preview: date-1904-loaded PASS', 'spreadsheet-preview: date-1904-saved PASS', 'spreadsheet-preview: date-1904-blank-saved PASS', 'spreadsheet-preview: new PASS', 'spreadsheet-preview: new-done PASS', 'spreadsheet-preview: sheet-prompt PASS', 'spreadsheet-preview: sheet-added PASS', 'spreadsheet-preview: sheet-edited PASS', 'spreadsheet-preview: sheet-renamed PASS', 'spreadsheet-preview: sheet-delete-setup PASS', 'spreadsheet-preview: sheet-delete-confirm PASS', 'spreadsheet-preview: sheet-delete-done PASS')):
            raise SystemExit('Spreadsheet preview check failed:\n' + output[:6000] + '\n…\n' + output[-3000:])
        if fixture.read_bytes() != b'external change':
            raise SystemExit('External change was not preserved during conflict')
        try:
            import openpyxl
        except ModuleNotFoundError:
            print('Independent openpyxl compatibility check: SKIPPED (not installed)')
        else:
            books = list(documents.rglob('游戏数值.xlsx'))
            if len(books) != 1:
                raise SystemExit('New workbook was not found in the isolated workspace')
            book = openpyxl.load_workbook(books[0], data_only=False)
            if book.sheetnames != ['Sheet1', '数值平衡'] or book['数值平衡']['A1'].value != 99:
                raise SystemExit('Independent openpyxl verification of the new worksheet failed')
            book.close()
            replacements = openpyxl.load_workbook(temp / 'replacements.xlsx', data_only=False)
            if replacements['Balance']['A1'].value != '生命 MP MP' or replacements['Balance']['A2'].value != 'MP 冷却' or replacements['Balance']['B1'].value != '=A2&"HP"' or replacements['Other']['A1'].value != 'HP' or replacements['Balance']['D1'].value != 0.5 or replacements['Balance']['D1'].number_format != '0.00%' or replacements['Balance']['E1'].value.strftime('%Y-%m-%d') != '2026-02-03' or replacements['Balance']['F1'].value is not False:
                raise SystemExit('Independent replacement verification failed')
            replacements.close()
            arithmetic = openpyxl.load_workbook(temp / 'arithmetic.xlsx', data_only=False)
            sheet = arithmetic['Balance']
            if sheet['A1'].value != 20.1 or sheet['A2'].value != 40.1 or sheet['B1'].value != '=(A1*2)+(0.1)' or sheet['B2'].value != 0.6 or sheet['B2'].number_format != '0.0%' or sheet['A4'].value.strftime('%Y-%m-%d') != '2025-01-02' or sheet['A3'].value != 'notes' or sheet['B3'].value is not True:
                raise SystemExit('Independent arithmetic verification failed')
            arithmetic.close()
            protected = openpyxl.load_workbook(temp / 'sheet_protection.xlsx', data_only=False)
            sheet = protected['Protected']
            if not sheet.protection.sheet or sheet['A1'].value != 'locked' or sheet['B1'].value != 'updated' or sheet['C1'].value != 11 or sheet['D1'].value != 22 or sheet['A2'].value != 'row' or sheet['B2'].value != 'locked despite row':
                raise SystemExit('Independent sheet protection verification failed')
            if any(sheet[cell].protection.locked for cell in ('B1', 'C1', 'D1', 'A2')):
                raise SystemExit('Unlocked cell protection was not retained')
            protected.close()
            formatted = openpyxl.load_workbook(temp / 'number_formats.xlsx', data_only=False)
            sheet = formatted['Formats']
            if sheet['A1'].value != 0.5 or sheet['A1'].number_format != '0.00%' or sheet['A3'].value != 0.25 or sheet['A3'].number_format != '0%' or sheet['B1'].value != '=A1*2':
                raise SystemExit('Independent openpyxl verification of number formats failed')
            for cell, value, fmt in [('B4', 0.5, '0.00%'), ('C5', 0.25, '0%'), ('A6', 0.75, '0.0%'), ('B6', 0.4, '0.0%'), ('C6', 0.25, '0.0%'), ('D6', 2, 'General')]:
                if sheet[cell].value != value or sheet[cell].number_format != fmt:
                    raise SystemExit(f'Independent inherited-format verification failed: {cell}')
            if sheet['A4'].value.strftime('%Y-%m-%d') != '2026-02-03' or sheet['A4'].number_format != 'yyyy-mm-dd':
                raise SystemExit('Independent date edit verification failed')
            for cell, expected in [('B7', '2028-03-04'), ('F7', '2026-02-04'), ('F8', '2026-02-05')]:
                if sheet[cell].value.strftime('%Y-%m-%d') != expected or sheet[cell].number_format != 'yyyy-mm-dd':
                    raise SystemExit(f'Independent blank date verification failed: {cell}')
            formatted.close()
            dates_1904 = openpyxl.load_workbook(temp / 'dates_1904.xlsx', data_only=False)
            if dates_1904.epoch.year != 1904 or dates_1904['Dates']['A1'].value.strftime('%Y-%m-%d') != '2025-01-03' or dates_1904['Dates']['B1'].value != '=A1+1' or dates_1904['Dates']['E2'].value.strftime('%Y-%m-%d') != '2025-01-04':
                raise SystemExit('Independent 1904 epoch verification failed')
            dates_1904.close()
            print('Independent openpyxl compatibility check: PASS')
        print('Spreadsheet edit, save and conflict protection: PASS (isolated off-screen workspace)')


if __name__ == '__main__':
    main()
