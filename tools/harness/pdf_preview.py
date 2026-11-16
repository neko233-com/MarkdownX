"""Exercise read-only PDF zoom, scrolling and navigation in an isolated desktop window."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def blank_pdf():
    objects = [b'<< /Type /Catalog /Pages 2 0 R >>', b'<< /Type /Pages /Kids [3 0 R] /Count 1 >>',
               b'<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 160] /Resources << >> /Contents 4 0 R >>',
               b'<< /Length 0 >>\nstream\n\nendstream']
    data, offsets = bytearray(b'%PDF-1.4\n'), [0]
    for i, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data.extend(f'{i} 0 obj\n'.encode() + obj + b'\nendobj\n')
    xref = len(data)
    data.extend(f'xref\n0 {len(offsets)}\n0000000000 65535 f \n'.encode())
    for offset in offsets[1:]:
        data.extend(f'{offset:010} 00000 n \n'.encode())
    data.extend(f'trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n'.encode())
    return bytes(data)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('-Quiet', action='store_true')
    parser.parse_args()
    binary = Path(os.environ.get('FDL_BIN', ROOT / 'target/release/markdownx.exe'))
    if os.name != 'nt' or not binary.is_file() or b'devtools: cannot read' not in binary.read_bytes():
        raise SystemExit('Build the Windows app with --features devtools before running this check.')
    original = ROOT / 'MarkdownX-Demo/论文阅读示例.pdf'
    before = hashlib.sha256(original.read_bytes()).digest()
    for data, valid in ((original.read_bytes(), True), (blank_pdf(), True), (b'not a PDF', False)):
        worker = subprocess.run([str(binary), '--internal-pdf-text-worker'], input=data, capture_output=True,
                                timeout=20, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        if (worker.returncode == 0) != valid:
            raise SystemExit('PDF text worker accepted invalid input or rejected a valid document')
        if valid:
            pages = json.loads(worker.stdout)['pages']
            if len(pages) != (3 if data != blank_pdf() else 1):
                raise SystemExit('PDF text worker returned the wrong page count')
    with tempfile.TemporaryDirectory(prefix='markdownx-pdf-ui-') as tmp:
        root = Path(tmp)
        shutil.copytree(ROOT / 'tools/harness/lib_clean', root / 'library')
        (root / 'documents').mkdir()
        sample = root / 'sample.pdf'
        shutil.copyfile(original, sample)
        caj = root / 'sample.caj'
        shutil.copyfile(original, caj)
        scan = root / 'scan.pdf'
        scan.write_bytes(blank_pdf())
        script = root / 'steps.txt'
        script.write_text(f'''resize 1100 760
wait 1000
pick {sample}
open-file
wait 5000
check-media-preview pdf-first
key ctrl-=
wait 800
check-media-preview pdf-zoomed
wheel 700 400 240
wait 600
check-media-preview pdf-scrolled
key pagedown
wait 4000
check-media-preview pdf-page2
key end
wait 4000
check-media-preview pdf-last
key home
wait 4000
check-media-preview pdf-first
key ctrl-0
wait 500
check-media-preview pdf-reset
key ctrl-f
wait 6000
check-media-preview pdf-indexed
type 方法与观察
wait 500
check-media-preview pdf-match2
key enter
wait 4000
check-media-preview pdf-page2
key ctrl-a
type markdownx
wait 500
check-media-preview pdf-match-all
key enter
wait 4000
check-media-preview pdf-first
key shift-enter
wait 4000
check-media-preview pdf-last
key ctrl-a
type 没有这段文字
wait 500
check-media-preview pdf-no-match
key escape
wait 500
check-media-preview pdf-search-closed
pick {scan}
open-file
wait 4000
key ctrl-f
wait 6000
type 不存在的文字
wait 500
check-media-preview pdf-scan
pick {caj}
open-file
wait 5000
check-media-preview caj
key ctrl-f
wait 6000
check-media-preview pdf-caj-indexed
type 方法与观察
wait 500
check-media-preview pdf-match2
key enter
wait 4000
check-media-preview pdf-page2
quit
''', encoding='utf-8')
        env = dict(os.environ, MARKDOWNX_HOME=str(root / 'library'), MARKDOWNX_LOG_DIR=str(root / 'logs'),
                   MARKDOWNX_TEST_DOCUMENTS_DIR=str(root / 'documents'), FDL_SCRIPT=str(script), FDL_SHOTS=str(root))
        env.pop('FDL_NOPARK', None)
        result = subprocess.run([str(binary)], cwd=root, env=env, capture_output=True, text=True, encoding='utf-8', errors='replace',
                                timeout=100, creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        logs = '\n'.join(path.read_text(encoding='utf-8', errors='replace') for path in (root / 'logs').glob('*') if path.is_file())
        expected = ('pdf-first', 'pdf-zoomed', 'pdf-scrolled', 'pdf-page2', 'pdf-last', 'pdf-reset', 'pdf-indexed', 'pdf-match2', 'pdf-match-all', 'pdf-no-match', 'pdf-search-closed', 'pdf-scan', 'pdf-caj-indexed')
        if result.returncode or any(f'media-preview: {phase} PASS' not in logs for phase in expected):
            raise SystemExit(f'PDF desktop test failed: {result.returncode}\n{result.stderr[:2500]}\n' + '\n'.join(line for line in logs.splitlines() if 'media-preview:' in line or 'panicked' in line))
        if any(hashlib.sha256(path.read_bytes()).digest() != before for path in (sample, caj, original)):
            raise SystemExit('PDF preview changed a source document')
        print('Read-only PDF zoom, scroll, text search, navigation and source preservation: PASS')


if __name__ == '__main__':
    main()
