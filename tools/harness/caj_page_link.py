"""Follow a page link to a PDF-backed CAJ without changing either file."""
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
    with tempfile.TemporaryDirectory(prefix='markdownx-caj-page-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        workspace = root / 'workspace'
        workspace.mkdir()
        caj = workspace / 'paper.caj'
        shutil.copyfile(ROOT / 'MarkdownX-Demo/论文阅读示例.pdf', caj)
        original = caj.read_bytes()
        source = workspace / 'paper.md'
        markdown = '# Paper\n\n[Page two](./paper.caj#page=2)\n'
        source.write_text(markdown, encoding='utf-8')
        steps = root / 'steps.txt'
        steps.write_text(f'wait 1000\npick {workspace}\nopen-folder\nwait 1000\npick {source}\nopen-file\nwait 1500\nfollow-relative-link ./paper.caj#page=2\nwait 6000\ncheck-media-preview caj\ncheck-media-preview pdf-page2\nquit\n', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(steps), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=35, creationflags=subprocess.CREATE_NO_WINDOW)
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        if result.returncode or any(f'media-preview: {phase} PASS' not in logs for phase in ('caj', 'pdf-page2')):
            raise SystemExit(f'CAJ page link failed: {result.returncode}\n{result.stderr[:3000]}\n{logs[-4000:]}')
        if caj.read_bytes() != original or source.read_text(encoding='utf-8') != markdown:
            raise SystemExit('CAJ page navigation changed a source file')
        print('PDF-backed CAJ relative page link: PASS')


if __name__ == '__main__':
    main()
