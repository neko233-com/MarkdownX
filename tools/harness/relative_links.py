"""Exercise relative file navigation in an isolated desktop workspace."""
import argparse
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    with tempfile.TemporaryDirectory(prefix='markdownx-relative-links-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        workspace = root / 'workspace'
        part = workspace / 'part'
        part.mkdir(parents=True)
        start, second, www = part / 'start.md', part / 'second.md', part / 'www.notes.md'
        start.write_text('# Start\n\n[Next](./second.md)\n[Heading](./second.md#目标-标题)\n[WWW](www.notes.md)\n[PDF][paper]\n[Page](../paper.pdf#page=2)\n[Last](../paper.pdf#page=3)\n[Unavailable](../paper.pdf#page=99)\n[Outside](../../outside.md)\n[Missing](missing.md)\n\n[paper]: ../paper.pdf\n', encoding='utf-8')
        second.write_text('# Second\n\n[Back](start.md) [Self](#目标-标题)\n\n' + 'Content paragraph.\n\n' * 80 + '## 目标 标题\n\n' + 'More content.\n\n' * 40, encoding='utf-8')
        www.write_text('# WWW\n\n[Back](start.md)\n', encoding='utf-8')
        outside = root / 'outside.md'
        outside.write_text('# Outside\n', encoding='utf-8')
        pdf = workspace / 'paper.pdf'
        shutil.copyfile(ROOT / 'MarkdownX-Demo/论文阅读示例.pdf', pdf)
        original_pdf = pdf.read_bytes()
        script = root / 'steps.txt'
        script.write_text(f'''resize 1100 760
wait 1000
pick {workspace}
open-folder
wait 1500
pick {start}
open-file
wait 1800
check-relative-file {start}
type NAVIGATIONSAVEMARKER
wait 300
follow-relative-link ./second.md
wait 2500
check-relative-file {second}
follow-relative-link start.md
wait 2200
check-relative-file {start}
follow-relative-link ./second.md#目标-标题
wait 2200
check-relative-file {second}
check-heading-scroll
follow-relative-link #目标-标题
wait 500
follow-relative-link start.md
wait 2200
check-relative-file {start}
follow-relative-link www.notes.md
wait 2200
check-relative-file {www}
follow-relative-link start.md
wait 2200
check-relative-file {start}
follow-relative-link ../../outside.md
wait 700
check-relative-file {start}
follow-relative-link missing.md
wait 700
check-relative-file {start}
follow-relative-link ../paper.pdf
wait 5000
check-media-preview pdf-first
key escape
wait 500
follow-relative-link ../paper.pdf#page=2
wait 5000
check-media-preview pdf-page2
key escape
wait 500
follow-relative-link ../paper.pdf#page=3
wait 5000
check-media-preview pdf-last
key escape
wait 500
follow-relative-link ../paper.pdf#page=99
wait 5000
check-media-preview pdf-page-missing
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=80, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or logs.count('relative-link: file PASS') != 9 or logs.count('relative-link: heading PASS') != 2 or 'relative-link: heading scroll PASS' not in logs or any(f'media-preview: {phase} PASS' not in logs for phase in ('pdf-page2', 'pdf-last', 'pdf-page-missing')):
            raise SystemExit(f'Relative-link desktop test failed: {result.returncode}\n{result.stderr[-3000:]}\n{logs[-4000:]}')
        saved = start.read_text(encoding='utf-8')
        if 'NAVIGATIONSAVEMARKER' not in saved or pdf.read_bytes() != original_pdf or outside.read_text(encoding='utf-8') != '# Outside\n':
            raise SystemExit(f'Navigation lost an edit or changed another file: marker={"NAVIGATIONSAVEMARKER" in saved}, pdf={pdf.read_bytes() == original_pdf}, outside={outside.read_text(encoding="utf-8")!r}, saved={saved!r}\n{logs[-2000:]}')
        print('Relative workspace links, save-before-navigation, PDF page links and boundary checks: PASS')


if __name__ == '__main__':
    main()
