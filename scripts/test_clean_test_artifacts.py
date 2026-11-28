"""Keep a usable Windows installer until the update is installed."""
import hashlib
import json
from pathlib import Path
from tempfile import TemporaryDirectory
from unittest import TestCase, main
from unittest.mock import patch

import clean_test_artifacts as cleaner


class InstallerRetentionTests(TestCase):
    def test_agent_bytecode_is_a_repository_test_artifact(self):
        with TemporaryDirectory(prefix="markdownx-clean-test-") as tmp:
            root = Path(tmp)
            cache = root / "tools" / "agent" / "__pycache__"
            cache.mkdir(parents=True)
            with patch.object(cleaner, "ROOT", root):
                self.assertIn(cache, cleaner.targets())

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


class LocalPackageRetentionTests(TestCase):
    def setUp(self):
        temp = TemporaryDirectory(prefix="markdownx-clean-test-")
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name).resolve()
        (self.root / "mygo.json").write_text(json.dumps({"version": "0.1.7"}), encoding="utf-8")
        self.release = self.root / "dist" / "release"
        for number in range(1, 8):
            version = f"0.1.{number}"
            platform = self.release / version / "windows-amd64"
            platform.mkdir(parents=True)
            (platform / ".mygo-files").write_text("MarkdownX.exe\nlicenses\n", encoding="utf-8")
            contents = []
            for asset in ("MarkdownX.exe", f"markdownx-setup-{version}-amd64.exe"):
                data = f"fixture {version} {asset}".encode()
                (platform / asset).write_bytes(data)
                contents.append(f"{hashlib.sha256(data).hexdigest()}  {asset}\n")
            (platform / "SHA256SUMS").write_text("".join(contents), encoding="utf-8")

    def test_waits_for_installation_and_keeps_last_three(self):
        with patch.object(cleaner, "ROOT", self.root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 6)}):
            self.assertEqual(cleaner.release_targets(), [])
        with patch.object(cleaner, "ROOT", self.root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 7), (0, 1, 2)}):
            self.assertEqual([p.name for p in cleaner.release_targets()], ["0.1.4", "0.1.3", "0.1.1"])
            cleaner.clean(releases=True)
        self.assertEqual(sorted(p.name for p in self.release.iterdir()), ["0.1.2", "0.1.5", "0.1.6", "0.1.7"])

    def test_keeps_unknown_files_broken_checksums_and_linked_packages(self):
        (self.release / "0.1.2" / "notes.txt").touch()
        (self.release / "0.1.3" / "windows-amd64" / "MarkdownX.exe").write_bytes(b"changed")
        backup = self.root / "backup"
        (self.release / "0.1.1").rename(backup)
        try:
            (self.release / "0.1.1").symlink_to(backup, target_is_directory=True)
        except OSError:
            pass
        with patch.object(cleaner, "ROOT", self.root), patch.object(cleaner, "installed_msi_versions", return_value={(0, 1, 7)}):
            self.assertEqual([p.name for p in cleaner.release_targets()], ["0.1.4"])
            cleaner.clean(releases=True)
        self.assertTrue((backup / "windows-amd64" / "MarkdownX.exe").is_file())
        self.assertTrue((self.release / "0.1.2" / "notes.txt").is_file())
        self.assertTrue((self.release / "0.1.3" / "windows-amd64" / "MarkdownX.exe").is_file())


if __name__ == "__main__":
    main()
