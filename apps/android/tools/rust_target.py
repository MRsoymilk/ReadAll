"""Project-local Android standard library; only an explicit prepare-rust command downloads.

Use the exact upstream compiler identity, HTTPS distribution manifest and SHA-256.
Never install into the system sysroot or run an archive-provided installation script.
"""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import subprocess
import tarfile
import tempfile
import time
import tomllib
import urllib.parse
import urllib.request

HOST = "static.rust-lang.org"
TARGETS = {"aarch64-linux-android", "x86_64-linux-android"}

class TargetError(RuntimeError):
    pass

def compiler(rustc: Path) -> tuple[str, str]:
    if not rustc.is_absolute():
        raise TargetError("Rust compiler path must be absolute")
    process = subprocess.run([str(rustc), "--version"], check=True, capture_output=True, text=True, timeout=15)
    identity = process.stdout.strip().removeprefix("rustc ")
    match = re.fullmatch(r"(\d+\.\d+\.\d+) \([0-9a-f]+ \d{4}-\d{2}-\d{2}\)", identity)
    if not match:
        raise TargetError("project-local rust-std requires an official stable compiler identity")
    return match.group(1), identity

def destination(root: Path, release: str, target: str) -> Path:
    if not re.fullmatch(r"\d+\.\d+\.\d+", release) or target not in TARGETS:
        raise TargetError("unsupported Rust version or target")
    return root / "target/android/toolchains" / f"rust-{release}-{target}"

def official_url(url: str) -> str:
    parsed = urllib.parse.urlsplit(url)
    if parsed.scheme != "https" or parsed.netloc != HOST or not parsed.path.startswith("/dist/") or parsed.query or parsed.fragment:
        raise TargetError("Rust component URL must use the official HTTPS distribution host")
    return url

def fetch(url: str, limit: int) -> bytes:
    with urllib.request.urlopen(official_url(url), timeout=30) as response:
        official_url(response.geturl())
        data = response.read(limit + 1)
    if len(data) > limit:
        raise TargetError("Rust distribution metadata exceeds its size limit")
    return data

def select_component(manifest: bytes, identity: str, target: str) -> dict:
    if target not in TARGETS:
        raise TargetError("unsupported Android target")
    try:
        package = tomllib.loads(manifest.decode("utf-8"))["pkg"]["rust-std"]
        if package["version"] != identity:
            raise TargetError("Rust standard library/compiler identities do not match")
        component = package["target"][target]
        if not component.get("available"):
            raise TargetError("matching Android standard library is unavailable")
        official_url(component["xz_url"])
        if not re.fullmatch(r"[0-9a-f]{64}", component["xz_hash"]):
            raise TargetError("invalid Rust component SHA-256")
        return component
    except (KeyError, ValueError, UnicodeError) as error:
        raise TargetError("invalid Rust distribution manifest") from error

def unpack(archive: Path, stage: Path, release: str, target: str) -> dict[str, str]:
    """Copy regular target libraries only; do not extract links or use tar.extractall."""
    prefix = PurePosixPath(f"rust-std-{release}-{target}/rust-std-{target}/lib/rustlib/{target}/lib")
    hashes: dict[str, str] = {}
    total = 0
    with tarfile.open(archive, "r:xz") as tar:
        for index, member in enumerate(tar):
            if index > 2048:
                raise TargetError("too many Rust component archive entries")
            path = PurePosixPath(member.name)
            if path.is_absolute() or ".." in path.parts or "\\" in member.name:
                raise TargetError("unsafe Rust component archive path")
            if not path.is_relative_to(prefix) or member.isdir():
                continue
            relative = path.relative_to(prefix)
            if not member.isfile() or len(relative.parts) != 1 or member.size < 0:
                raise TargetError("non-regular or nested Rust target library")
            total += member.size
            if total > 512 * 1024 * 1024:
                raise TargetError("unpacked Rust target exceeds size limit")
            name = f"lib/rustlib/{target}/lib/{relative}"
            output = stage / name
            output.parent.mkdir(parents=True, exist_ok=True)
            source = tar.extractfile(member)
            if source is None:
                raise TargetError("missing Rust target archive payload")
            digest = hashlib.sha256()
            with source, output.open("xb") as sink:
                remaining = member.size
                while remaining:
                    block = source.read(min(1024 * 1024, remaining))
                    if not block:
                        raise TargetError("truncated Rust target archive payload")
                    sink.write(block); digest.update(block); remaining -= len(block)
            hashes[name] = digest.hexdigest()
    if not any(Path(name).name.startswith("libstd-") and name.endswith(".rlib") for name in hashes):
        raise TargetError("Rust target archive has no standard library")
    return hashes

def prepared(root: Path, rustc: Path, target: str) -> Path | None:
    release, identity = compiler(rustc)
    path = destination(root, release, target)
    report = path / "readall-rust-target.json"
    if not report.is_file():
        return None
    try:
        data = json.loads(report.read_text())
    except (ValueError, OSError) as error:
        raise TargetError("invalid project-local Rust target metadata") from error
    if data.get("compiler") != identity or data.get("target") != target:
        raise TargetError("project-local Rust target does not match the current compiler")
    if not list((path / "lib/rustlib" / target / "lib").glob("libstd-*.rlib")):
        raise TargetError("project-local Rust target is incomplete; prepare it again in a fresh directory")
    return path

def prepare(root: Path, rustc: Path, target: str) -> Path:
    release, identity = compiler(rustc)
    output = destination(root, release, target)
    existing = prepared(root, rustc, target)
    if existing:
        print("Matching project-local Rust target:", existing, flush=True)
        return existing
    if output.exists():
        raise TargetError("refusing to overwrite an unrecognized project-local Rust target")
    manifest_url = f"https://{HOST}/dist/channel-rust-{release}.toml"
    manifest = fetch(manifest_url, 4 * 1024 * 1024)
    checksum = fetch(manifest_url + ".sha256", 1024).decode("ascii").split()[0]
    if hashlib.sha256(manifest).hexdigest() != checksum:
        raise TargetError("Rust manifest checksum mismatch")
    component = select_component(manifest, identity, target)
    output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="prepare-", dir=output.parent) as temporary:
        temporary = Path(temporary)
        archive = temporary / "rust-std.tar.xz"
        digest = hashlib.sha256(); size = 0; started = time.monotonic()
        print("Downloading exact matching Rust target:", identity, target, flush=True)
        with urllib.request.urlopen(component["xz_url"], timeout=30) as response, archive.open("xb") as sink:
            official_url(response.geturl())
            while True:
                data = response.read(1024 * 1024)
                if not data:
                    break
                size += len(data)
                if size > 128 * 1024 * 1024 or time.monotonic() - started > 90:
                    raise TargetError("Rust component download exceeds its budget")
                sink.write(data); digest.update(data)
        if digest.hexdigest() != component["xz_hash"]:
            raise TargetError("Rust component SHA-256 mismatch")
        stage = temporary / "sysroot"; stage.mkdir()
        files = unpack(archive, stage, release, target)
        (stage / "readall-rust-target.json").write_text(json.dumps({"compiler": identity, "target": target, "url": component["xz_url"], "sha256": component["xz_hash"], "files": files}, indent=2) + "\n")
        if output.exists():
            raise TargetError("project-local Rust target appeared during preparation")
        os.rename(stage, output)
    print("Prepared", len(files), "target libraries in", output, flush=True)
    return output
