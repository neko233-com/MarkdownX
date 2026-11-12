#!/usr/bin/env python3
"""Shared macOS, Windows and Linux checks; run inside the platform build environment."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
os.chdir(ROOT)
sys.path.insert(0, str(ROOT / "scripts"))
from clean_test_artifacts import clean  # noqa: E402

# A previous failed run must not leak screenshots, MSI logs or Python caches
# into the tree before the tests start. See AGENTS.md "测试前清理".
if clean() != 0:
    raise SystemExit("artifact cleanup failed")
subprocess.run([sys.executable, "-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"], check=True)

def cargo(*args, env=None):
    subprocess.run(["cargo", *args], check=True, env=env)

cargo("test", "--release", "--workspace", "--locked")
for library in ("library", "library-zh"):
    cargo("test", "--release", "--locked", "-p", "fdl_core", "library_round_trips", "--", "--ignored",
          env={**os.environ, "FDL_ROUNDTRIP_DIR": str(Path("shared-mock", library, "docs").resolve())})
source = Path("shared-mock/guides/assets")
bundled = Path("shared-mock/library/assets/guides")
for image in source.rglob("*"):
    if image.is_file():
        target = bundled / image.relative_to(source)
        assert target.read_bytes() == image.read_bytes(), f"Bundled guide asset is stale: {target}"
print("Bundled guide assets match their sources")
cargo("clippy", "--release", "--workspace", "--locked", "--all-targets", "--", "-D", "warnings")
# The harness build (script driver compiled in) must stay clean too.
cargo("clippy", "--release", "--workspace", "--locked", "--all-targets", "--features", "devtools", "--", "-D", "warnings")
