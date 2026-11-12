"""Round-trip mixed-language books through the public folder tooling."""
import base64
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
TOOL = ROOT / 'tools' / 'agent' / 'mdxb.py'


class MultilingualBookTests(unittest.TestCase):
    def test_mixed_formats_metadata_and_assets_survive_unpack_and_pack(self):
        with tempfile.TemporaryDirectory(prefix='markdownx-languages-') as tmp:
            root = Path(tmp)
            source = root / 'source.mdxb'
            folder = root / 'unpacked'
            packed = root / 'repacked.mdxb'
            book = {
                'format': 'markdownx.book', 'version': 2, 'id': 'original-id',
                'title': {'zh': '中文书名', 'en': 'English title'},
                'subtitle': {'zh': '副标题', 'en': 'Subtitle'}, 'authors': ['Author'],
                'languages': ['zh', 'en'], 'default_language': 'en',
                'chapters': [{'id': 'original-chapter', 'title': {'zh': '中文', 'en': 'English'},
                              'format': 'docxxml', 'content': '<title>中文</title><p>正文</p>',
                              'format_en': 'markdown', 'content_en': '# English\n\n![image](assets/图片.png)\n'}],
                'assets': {'assets/图片.png': {'mime': 'application/x-custom-image', 'data': base64.b64encode(b'picture').decode()}},
            }
            source.write_text(json.dumps(book, ensure_ascii=False), encoding='utf-8')
            subprocess.run([sys.executable, str(TOOL), 'unpack', str(source), str(folder)], check=True, capture_output=True, text=True)
            subprocess.run([sys.executable, str(TOOL), 'pack', str(folder), '-o', str(packed)], check=True, capture_output=True, text=True)
            result = json.loads(packed.read_text(encoding='utf-8'))
            self.assertEqual(result['version'], 2)
            self.assertEqual(result['title'], book['title'])
            self.assertEqual(result['subtitle'], book['subtitle'])
            self.assertEqual(result['default_language'], 'en')
            self.assertEqual(result['chapters'][0]['id'], 'original-chapter')
            self.assertEqual(result['chapters'][0]['content_en'], book['chapters'][0]['content_en'])
            self.assertEqual(result['chapters'][0]['format_en'], 'markdown')
            self.assertEqual(result['assets'], book['assets'])


if __name__ == '__main__':
    unittest.main()
