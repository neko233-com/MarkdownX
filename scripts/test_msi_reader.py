"""Keep the installed offline reader and its examples complete."""
import json
from pathlib import Path
import unittest
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parent.parent


class MsiReaderTests(unittest.TestCase):
    def test_reader_and_manifest_examples_are_included_in_msi(self):
        root = ET.parse(ROOT / 'packaging/windows/markdownx.wxs').getroot()
        ns = {'w': 'http://schemas.microsoft.com/wix/2006/wi'}
        sources = {node.attrib['Source'].replace('\\', '/') for node in root.findall('.//w:File', ns) if 'Source' in node.attrib}
        required = {'web/index.html', 'web/style.css', 'examples/manifest.json', 'assets/docx-viewer/DOCX-PREVIEW-LICENSE', 'assets/docx-viewer/JSZIP-LICENSE.md', 'assets/ofd-viewer/OFDJS-LICENSE', 'assets/ofd-viewer/FFLATE-LICENSE'}
        required.update('assets/legacy-doc-viewer/' + name for name in ('UNDOC-LICENSE', 'UNPARSER-SHARED-LICENSE', 'CFB-LICENSE'))
        required.add('assets/epub-viewer/RBOOK-LICENSE')
        required.update('assets/module-parser-licenses/' + name for name in ('RESS-LICENSE', 'UNICODE-XID-LICENSE'))
        required.update(path.relative_to(ROOT).as_posix() for path in (ROOT / 'web/js').glob('*.js'))
        required.update(('web/js/katex.mjs', 'web/js/katex-LICENSE'))
        required.update(path.relative_to(ROOT).as_posix() for path in (ROOT / 'web/lib/three').rglob('*') if path.is_file())
        manifest = json.loads((ROOT / 'examples/manifest.json').read_text(encoding='utf-8'))
        required.update('examples/' + item['file'] for item in manifest['books'])
        self.assertFalse(required - sources, f'Missing MSI reader files: {sorted(required - sources)}')
        self.assertTrue(all((ROOT / name).is_file() for name in required))
        self.assertFalse(root.findall('.//w:DirectoryRef[@Id="StartupFolder"]', ns), 'Single-user install must not start collaboration at login')
        self.assertTrue(root.findall('.//w:Shortcut[@Id="StartMenuCollab"]', ns), 'Optional collaboration remains available on demand')
        installer = (ROOT / 'install.ps1').read_text(encoding='utf-8')
        self.assertNotIn('($installed -and $servers.Count -eq 0)', installer, 'Fresh installs must not start collaboration automatically')


if __name__ == '__main__':
    unittest.main()
