import copy
import hashlib
import importlib.util
import json
import tempfile
import tomllib
import unittest
import tarfile
import zipfile
from unittest.mock import patch
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("release", ROOT / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.repo = Path(self.temp.name)
        self.dist = self.repo / "dist"
        self.dist.mkdir()
        (self.repo / "downloads").mkdir()
        (self.repo / "shadow.toml").write_text((ROOT / "shadow.toml").read_text())
        for source in ("github", "tos"):
            (self.repo / "downloads" / f"{source}.json").write_text(json.dumps({
                "schema": 1, "source": source, "channels": {"stable": None, "dev": None}}))
        self.info = {"tag_name": "dev-build", "published_at": "2026-11-15T00:00:00Z", "assets": []}
        for platform in ("windows-x86_64", "linux-x86_64"):
            path = self.dist / f"eorzea-app-dev-test-{platform}.zip"
            path.write_bytes(platform.encode())
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            self.info["assets"].append({"name": path.name, "size": path.stat().st_size,
                "digest": f"sha256:{digest}", "state": "uploaded",
                "browser_download_url": f"https://github.com/icexiv/eorzea/releases/download/dev-build/{path.name}"})

    def publish(self, source="github", info=None, build=1):
        release.update_manifest(self.repo, self.dist, info or self.info, "dev", source, "a" * 40, "dev-test", build)
        return json.loads((self.repo / "downloads" / f"{source}.json").read_text())

    def test_github_only_manifest(self):
        manifest = self.publish()
        self.assertIsNone(manifest["channels"]["stable"])
        self.assertEqual(len(manifest["channels"]["dev"]["assets"]), 2)
        self.assertTrue(all("github.com" in a["url"] for a in manifest["channels"]["dev"]["assets"].values()))

    def test_tos_requires_verified_refs_and_never_contains_github_downloads(self):
        with self.assertRaises(FileNotFoundError):
            self.publish("tos")
        release.stage(self.repo, self.dist, "dev")
        refs = self.repo / ".shadow/refs/published/dev"
        refs.mkdir(parents=True)
        for path in release.files(self.dist):
            digest = hashlib.sha256(path.read_bytes()).hexdigest()
            (refs / (path.name + ".ref")).write_text(f'version = 1\noid = "sha256:{digest}"\nsize = {path.stat().st_size}\n')
        manifest = self.publish("tos")
        self.assertNotIn("github.com", json.dumps(manifest))
        self.assertIn("/eorzea/objects/sha256/", json.dumps(manifest))

    def test_hash_mismatch_and_missing_platform_leave_old_manifest_intact(self):
        before = (self.repo / "downloads/github.json").read_text()
        invalid = copy.deepcopy(self.info)
        invalid["assets"][0]["digest"] = "sha256:" + "0" * 64
        with self.assertRaisesRegex(ValueError, "digest mismatch"):
            self.publish(info=invalid)
        self.assertEqual((self.repo / "downloads/github.json").read_text(), before)
        next(self.dist.iterdir()).unlink()
        with self.assertRaisesRegex(ValueError, "packages missing"):
            self.publish()

    def test_does_not_replace_newer_build(self):
        self.publish(build=2)
        with self.assertRaisesRegex(ValueError, "newer channel"):
            self.publish(build=1)

    def test_staging_preserves_other_channel_refs(self):
        stable = self.repo / ".shadow/refs/published/stable/keep.zip.ref"
        stable.parent.mkdir(parents=True)
        stable.write_text("keep")
        release.stage(self.repo, self.dist, "dev")
        self.assertTrue(stable.exists())

    def test_packages_windows_app_and_cli_independently(self):
        target = self.repo / "target"
        target.mkdir()
        for binary in ("eorzea-app.exe", "eoz.exe"):
            (target / binary).write_bytes(b"executable")
        release.package(target, "windows-x86_64", "dev-test", self.dist)
        for binary in ("eorzea-app", "eoz"):
            archive = self.dist / f"{binary}-dev-test-windows-x86_64.zip"
            with zipfile.ZipFile(archive) as package:
                self.assertEqual(package.namelist(), [binary + ".exe"])

    def test_linux_package_preserves_executable_permission(self):
        target = self.repo / "target"
        target.mkdir()
        for binary in ("eorzea-app", "eoz"):
            (target / binary).write_bytes(b"executable")
        with patch.object(release.subprocess, "run"):
            release.package(target, "linux-x86_64", "dev-test", self.dist)
        with tarfile.open(self.dist / "eorzea-app-dev-test-linux-x86_64.tar.gz") as package:
            self.assertEqual(package.getnames(), ["eorzea-app"])
            self.assertEqual(package.getmember("eorzea-app").mode & 0o777, 0o755)
