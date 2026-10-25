"""Exercise the production build script through Cargo and real Git metadata."""
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class BuildMetadataTests(unittest.TestCase):
    def command(self, cwd, *args):
        result = subprocess.run(args, cwd=cwd, text=True, capture_output=True, check=True)
        return result.stdout.strip()

    def fixture(self, root):
        self.command(root, "git", "init", "-b", "main")
        self.command(root, "git", "config", "user.name", "Build fixture")
        self.command(root, "git", "config", "user.email", "fixture@example.invalid")
        (root / "src").mkdir()
        (root / "Cargo.toml").write_text('[package]\nname = "metadata-fixture"\nversion = "0.1.0"\nedition = "2021"\n')
        (root / "src/main.rs").write_text('fn main() { println!("{}{}", env!("LST_BUILD_GIT_SHA"), env!("LST_BUILD_GIT_SUFFIX")); }')
        (root / ".gitignore").write_text("/target\n")
        (root / "payload").write_text("tracked content\n")
        shutil.copy2(ROOT / "apps/lst-gpui/build.rs", root / "build.rs")
        self.command(root, "cargo", "generate-lockfile", "--offline")
        self.command(root, "git", "add", ".")
        self.command(root, "git", "commit", "-m", "Fixture")

    def build(self, root):
        self.command(root, "cargo", "build", "--offline")
        output = next((root / "target/debug/build").glob("metadata-fixture-*/output"))
        identity = self.command(root, str(root / "target/debug/metadata-fixture"))
        return identity, output.stat().st_mtime_ns

    def assert_fresh_and_current(self, root):
        expected = self.command(root, "git", "rev-parse", "HEAD")
        first = self.build(root)
        self.assertEqual(first[0], expected)
        self.assertEqual(self.build(root), first, "unchanged Cargo invocation must stay fresh")

    def advance_ref(self, root):
        head_ref = self.command(root, "git", "symbolic-ref", "HEAD")
        tree = self.command(root, "git", "rev-parse", "HEAD^{tree}")
        commit = self.command(root, "git", "commit-tree", tree, "-p", "HEAD", "-m", "Metadata-only commit")
        self.command(root, "git", "update-ref", head_ref, commit)

    def test_loose_packed_and_detached_heads_stay_fresh_and_detect_changes(self):
        with tempfile.TemporaryDirectory(prefix="lst-build-metadata-") as directory:
            root = Path(directory)
            self.fixture(root)
            self.assert_fresh_and_current(root)
            self.command(root, "git", "pack-refs", "--all", "--prune")
            self.assert_fresh_and_current(root)
            self.advance_ref(root)
            self.assert_fresh_and_current(root)
            self.command(root, "git", "checkout", "--detach")
            self.assert_fresh_and_current(root)
            (root / "payload").write_text("changed\n")
            first = self.build(root)
            self.assertTrue(first[0].endswith("-dirty"))
            self.assertEqual(self.build(root), first)
            (root / "payload").unlink()
            first = self.build(root)
            self.assertTrue(first[0].endswith("-dirty"))
            self.assertEqual(self.build(root), first, "deleted tracked files must not force endless rebuilds")

    def test_linked_worktree_tracks_common_refs_and_its_own_head(self):
        with tempfile.TemporaryDirectory(prefix="lst-build-worktree-") as directory:
            root = Path(directory) / "main"
            root.mkdir()
            self.fixture(root)
            linked = Path(directory) / "linked"
            self.command(root, "git", "worktree", "add", "-b", "linked", str(linked))
            self.command(root, "git", "pack-refs", "--all", "--prune")
            self.assert_fresh_and_current(linked)
            self.advance_ref(linked)
            self.assert_fresh_and_current(linked)
            self.command(linked, "git", "checkout", "--detach")
            self.assert_fresh_and_current(linked)


if __name__ == "__main__":
    unittest.main()
