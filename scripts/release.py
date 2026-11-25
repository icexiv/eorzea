"""Package release binaries and publish source-specific download manifests."""

import argparse
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import tarfile
import tomllib
import zipfile
from pathlib import Path
from urllib.parse import urlsplit

PLATFORMS = ("windows-x86_64", "linux-x86_64", "macos-aarch64")


def package(target, platform, version, output):
    if platform not in PLATFORMS or not re.fullmatch(r"[A-Za-z0-9._+-]+", version):
        raise ValueError("invalid platform or version")
    output.mkdir(parents=True, exist_ok=True)
    windows = platform.startswith("windows")
    for binary in ("eorzea-app", "eoz"):
        source = target / (binary + (".exe" if windows else ""))
        if not source.is_file():
            raise ValueError(f"missing binary: {source}")
        if not windows:
            source.chmod(0o755)
            subprocess.run(["strip", str(source)], check=True)
        stem = f"{binary}-{version}-{platform}"
        if windows:
            with zipfile.ZipFile(output / f"{stem}.zip", "w", zipfile.ZIP_DEFLATED) as archive:
                archive.write(source, source.name)
        elif platform == "macos-aarch64" and binary == "eorzea-app":
            bundle = output / "macos-staging" / "Eorzea.app" / "Contents"
            (bundle / "MacOS").mkdir(parents=True, exist_ok=True)
            shutil.copy2(source, bundle / "MacOS" / "eorzea-app")
            (bundle / "Resources").mkdir(parents=True, exist_ok=True)
            shutil.copy2(Path(__file__).resolve().parents[1] / "assets/eorzea.icns",
                         bundle / "Resources/eorzea.icns")
            with (bundle / "Info.plist").open("wb") as file:
                app_version = tomllib.loads((Path(__file__).resolve().parents[1] / "Cargo.toml").read_text())["package"]["version"]
                plistlib.dump({"CFBundleExecutable": "eorzea-app", "CFBundleName": "Eorzea",
                    "CFBundleIdentifier": "top.icexiv.eorzea", "CFBundlePackageType": "APPL",
                    "CFBundleIconFile": "eorzea.icns",
                    "CFBundleShortVersionString": app_version.split("-")[0],
                    "CFBundleVersion": os.environ.get("GITHUB_RUN_NUMBER", "1"),
                    "NSHighResolutionCapable": True}, file)
            (bundle.parent.parent / "Applications").symlink_to("/Applications")
            subprocess.run(["hdiutil", "create", "-volname", "Eorzea", "-srcfolder",
                str(bundle.parent.parent), "-ov", "-format", "UDZO", str(output / f"{stem}.dmg")], check=True)
        else:
            with tarfile.open(output / f"{stem}.tar.gz", "w:gz") as archive:
                archive.add(source, arcname=source.name)


def files(directory):
    return sorted(path for path in directory.iterdir() if path.is_file()
                  and path.name.startswith(("eorzea-app-", "eoz-"))
                  and path.name.endswith((".zip", ".tar.gz", ".dmg", ".exe", "-linux-x86_64", "-macos-aarch64")))


def stage(repo, directory, channel):
    """Replace only the selected channel's refs; keep the other channel reachable."""
    selected = files(directory)
    if not selected:
        raise ValueError("no release packages")
    ref_root = repo / ".shadow/refs/published" / channel
    if ref_root.exists():
        for ref in ref_root.rglob("*.ref"):
            ref.unlink()
    destination = repo / "published" / channel
    destination.mkdir(parents=True, exist_ok=True)
    for old in destination.iterdir():
        if old.is_file():
            old.unlink()
    for source in selected:
        shutil.copy2(source, destination / source.name)


def update_manifest(repo, directory, release, channel, source, commit, version, build):
    if release.get("draft") or (channel == "stable" and release.get("prerelease")):
        raise ValueError("release is not eligible for this channel")
    manifest_path = repo / "downloads" / f"{source}.json"
    manifest = json.loads(manifest_path.read_text())
    if manifest["schema"] != 1 or manifest["source"] != source:
        raise ValueError("invalid existing manifest")
    previous = manifest["channels"][channel]
    if previous and int(previous.get("build", 0)) > build:
        raise ValueError("refusing to replace a newer channel build")
    remote = {asset["name"]: asset for asset in release["assets"]}
    config = tomllib.loads((repo / "shadow.toml").read_text())
    backend = config["backend"]
    host = urlsplit(backend["endpoint"]).netloc
    prefix = backend.get("prefix", "").strip("/")
    base = f"https://{backend['bucket']}.{host}/" + (prefix + "/" if prefix else "")
    assets = {}
    for path in files(directory):
        if not path.name.startswith("eorzea-app-"):
            continue
        platform = next((p for p in PLATFORMS if f"-{p}." in path.name or path.name.endswith("-" + p)), None)
        if not platform or platform in assets:
            raise ValueError(f"invalid or duplicate App platform: {path.name}")
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        size = path.stat().st_size
        attachment = remote.get(path.name)
        if not attachment or attachment.get("state") != "uploaded" or attachment["size"] != size:
            raise ValueError(f"GitHub attachment is missing or incomplete: {path.name}")
        if attachment.get("digest") and attachment["digest"] != f"sha256:{digest}":
            raise ValueError(f"GitHub attachment digest mismatch: {path.name}")
        url = attachment["browser_download_url"]
        if source == "tos":
            ref = tomllib.loads((repo / ".shadow/refs/published" / channel / (path.name + ".ref")).read_text())
            if ref["oid"] != f"sha256:{digest}" or ref["size"] != size:
                raise ValueError(f"Shadow ref mismatch: {path.name}")
            url = f"{base}{config['name']}/objects/sha256/{digest[:2]}/{digest[2:]}"
        assets[platform] = {"filename": path.name, "url": url, "sha256": digest,
                            "size": size, "experimental": platform.startswith("macos")}
    if not {"windows-x86_64", "linux-x86_64"}.issubset(assets):
        raise ValueError("required Windows/Linux App packages missing")
    manifest["channels"][channel] = {"version": version, "tag": release["tag_name"],
        "commit": commit, "build": build, "published_at": release["published_at"], "assets": assets}
    temporary = manifest_path.with_suffix(".json.tmp")
    temporary.write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + "\n")
    temporary.replace(manifest_path)


def main():
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    pack = sub.add_parser("package")
    pack.add_argument("--target", type=Path, required=True)
    pack.add_argument("--platform", required=True)
    pack.add_argument("--version", required=True)
    pack.add_argument("--output", type=Path, default=Path("dist"))
    for name in ("stage", "manifest"):
        command = sub.add_parser(name)
        command.add_argument("--repo", type=Path, required=True)
        command.add_argument("--directory", type=Path, required=True)
        command.add_argument("--channel", choices=("stable", "dev"), required=True)
        if name == "manifest":
            command.add_argument("--source", choices=("github", "tos"), required=True)
            command.add_argument("--release", type=Path, required=True)
            command.add_argument("--commit", required=True)
            command.add_argument("--version", required=True)
            command.add_argument("--build", type=int, required=True)
    args = parser.parse_args()
    if args.command == "package":
        package(args.target, args.platform, args.version, args.output)
    elif args.command == "stage":
        stage(args.repo, args.directory, args.channel)
    else:
        update_manifest(args.repo, args.directory, json.loads(args.release.read_text()),
                        args.channel, args.source, args.commit, args.version, args.build)


if __name__ == "__main__":
    main()
