"""Exercise localized first launch, upgrades and restoration in temporary libraries."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[2]
WELCOME = '4ea72c5a-82e0-54a4-88aa-3a9914755634.xml'


def main():
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release' / ('markdownx.exe' if os.name == 'nt' else 'markdownx')))
    if not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the app with --features devtools before running this check.')
    subprocess.run([sys.executable, str(ROOT / 'scripts/clean_test_artifacts.py')], cwd=ROOT, check=True)
    with tempfile.TemporaryDirectory(prefix='markdownx-onboarding-') as tmp:
        base = Path(tmp).resolve()

        def setup(name):
            root = base / name
            root.mkdir()
            (root / '.markdownx-workspace-test').touch()
            (root / 'documents').mkdir()
            return root

        def run(root, phase, language, *args, expected_file=None, change_language=None, expect_english=False):
            script = root / 'steps.txt'
            steps = f'wait 1600\ncheck-onboarding {phase}\n'
            if change_language:
                steps += f'onboarding-language {change_language}\nwait 300\ncheck-onboarding ui-en\ncheck-onboarding {phase}\n'
            if expect_english:
                steps += 'check-onboarding ui-en\n'
            script.write_text(steps + 'quit\n', encoding='utf-8')
            env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                       MARKDOWNX_TEST_FIRST_RUN='1', MARKDOWNX_TEST_LANGUAGE=language,
                       MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), MARKDOWNX_TEST_WORKSPACE_ROOT=str(root),
                       FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
            env.pop('FDL_NOPARK', None)
            if expected_file is not None:
                env['MARKDOWNX_TEST_EXPECTED_FILE'] = str(expected_file)
            result = subprocess.run([str(binary), *map(str, args)], cwd=root, env=env, capture_output=True,
                                    text=True, encoding='utf-8', errors='replace', timeout=120)
            output = result.stdout + result.stderr
            if result.returncode or f'onboarding: {phase} PASS' not in output or ((change_language or expect_english) and 'onboarding: ui-en PASS' not in output):
                raise SystemExit(f'Onboarding {root.name}/{phase} failed:\n' + output[-7000:])

        for language, filename, title in (('zh', '开始使用.md', '欢迎使用 MarkdownX'), ('en', 'Getting started.md', 'Welcome to MarkdownX')):
            root = setup(language)
            run(root, language, language, change_language='en' if language == 'zh' else None)
            path = root / 'documents' / 'MarkDown-Demo' / filename
            text = path.read_text(encoding='utf-8')
            assert title in text and ('Ctrl+K' in text if sys.platform != 'darwin' else '⌘ K' in text)
            guide = (root / 'library/docs' / WELCOME).read_text(encoding='utf-8')
            assert title in guide
            assert ('Welcome to MarkdownX' not in guide) if language == 'zh' else ('欢迎使用 MarkdownX' not in guide)
            changed = text + '\nUser content remains untouched.\n'
            path.write_text(changed, encoding='utf-8')
            run(root, language, 'zh', expect_english=language == 'zh')
            assert path.read_text(encoding='utf-8') == changed
            assert json.loads((root / 'library/preferences.json').read_text(encoding='utf-8'))['starter_offered']

        root = setup('existing')
        folder = root / 'documents/MarkDown-Demo'
        folder.mkdir()
        existing = folder / 'My notes.md'
        existing.write_text('# Existing\n\nDo not change.\n', encoding='utf-8')
        run(root, 'existing', 'zh')
        assert [p.name for p in folder.iterdir()] == [existing.name]
        assert existing.read_text(encoding='utf-8') == '# Existing\n\nDo not change.\n'

        for edited in (False, True):
            root = setup('edited-legacy' if edited else 'upgrade')
            shutil.copytree(ROOT / 'shared-mock/library', root / 'library')
            legacy = (ROOT / 'shared-mock/guides/legacy/v0.1.14-welcome.xml').read_bytes()
            if edited:
                legacy = legacy.replace(b'MarkdownX is a local-first', b'My edited local-first')
            old = root / 'library/docs' / WELCOME
            old.write_bytes(legacy)
            run(root, 'edited-legacy' if edited else 'zh', 'zh')
            assert old.read_bytes() == legacy
            if edited:
                assert not list((root / 'documents/MarkDown-Demo').glob('*.md'))

        root = setup('target')
        target = root / 'chosen.md'
        target.write_text('# A chosen file\n', encoding='utf-8')
        run(root, 'target', 'zh', target, expected_file=target)
        assert not list((root / 'documents/MarkDown-Demo').glob('*.md'))
        assert target.read_text(encoding='utf-8') == '# A chosen file\n'
        print('Onboarding: PASS (both languages, workspace tree, restart and language change, existing files, untouched/edited legacy guides, explicit startup file).')


if __name__ == '__main__':
    main()
