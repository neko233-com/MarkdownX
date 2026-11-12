"""Verify both compiled starter libraries match their editable sources."""
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent


class LocalizedGuidesTest(unittest.TestCase):
    def test_generated_guides_match_bundled_documents(self):
        with tempfile.TemporaryDirectory(prefix="markdownx-guide-test-") as tmp:
            documents = []
            for language, bundle in (("en", "library"), ("zh", "library-zh")):
                output = Path(tmp) / language
                subprocess.run([sys.executable, str(ROOT / "shared-mock/guides/build.py"), str(output), language], check=True, capture_output=True)
                generated = {p.name: p.read_bytes() for p in (output / "docs").glob("*.xml")}
                embedded = {p.name: p.read_bytes().replace(b"\r\n", b"\n") for p in (ROOT / "shared-mock" / bundle / "docs").glob("*.xml")}
                self.assertEqual(generated, embedded)
                self.assertEqual(len(generated), 3)
                documents.append(generated)
            self.assertEqual(set(documents[0]), set(documents[1]))
            welcome = "4ea72c5a-82e0-54a4-88aa-3a9914755634.xml"
            self.assertIn("Welcome to MarkdownX", documents[0][welcome].decode("utf-8"))
            self.assertIn("欢迎使用 MarkdownX", documents[1][welcome].decode("utf-8"))
            self.assertNotIn("Welcome to MarkdownX", documents[1][welcome].decode("utf-8"))


if __name__ == "__main__":
    unittest.main()
