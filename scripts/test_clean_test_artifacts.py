"""Keep a usable Windows installer until the update is installed."""
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import TestCase, main
from unittest.mock import patch

import clean_test_artifacts as cleaner


class InstallerRetentionTests(TestCase):
    def test_keeps_installed_version_until_update_is_installed(self):
        with TemporaryDirectory(prefix="markdownx-clean-test-") as tmp:
            root = Path(tmp)
            (root / "Cargo.toml").write_text('version = "0.1.12"\n', encoding="utf-8")
            dist = root / "dist"
            dist.mkdir()
            old = dist / "MarkdownX-0.1.11-x64.msi"
            current = dist / "MarkdownX-0.1.12-x64.msi"
            old.touch()
            current.touch()
            old_checksum = old.with_name(old.name + ".sha256")
            old_checksum.touch()
            with patch.object(cleaner, "ROOT", root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 11)}):
                self.assertNotIn(old, cleaner.targets())
                self.assertNotIn(old_checksum, cleaner.targets())
            with patch.object(cleaner, "ROOT", root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 12)}):
                self.assertIn(old, cleaner.targets())
                self.assertIn(old_checksum, cleaner.targets())
            with patch.object(cleaner, "ROOT", root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 11), (0, 1, 12)}):
                self.assertNotIn(old, cleaner.targets())
                self.assertNotIn(old_checksum, cleaner.targets())
            current.unlink()
            with patch.object(cleaner, "ROOT", root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 12)}):
                self.assertNotIn(old, cleaner.targets())
                self.assertNotIn(old_checksum, cleaner.targets())


if __name__ == "__main__":
    main()
