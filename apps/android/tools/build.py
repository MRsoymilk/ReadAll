#!/usr/bin/python3
"""SDK-only Android build. All tools are absolute paths; never edits shell profiles.

Examples:
  /usr/bin/python3 apps/android/tools/build.py doctor --sdk /opt/android-sdk
  /usr/bin/python3 apps/android/tools/build.py build --sdk /opt/android-sdk
  /usr/bin/python3 apps/android/tools/build.py host-test --vendor /absolute/vendor

No automatic SDK downloads, license acceptance, system Rust changes, or device changes.
Only `prepare-rust` downloads the exact matching target into the project target directory.
Only the explicit `install` command installs the resulting debug APK on a device.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import socket
import struct
import subprocess
import sys
import tempfile
import tomllib
import zipfile
import xml.etree.ElementTree as ET
import rust_target

ROOT = Path(__file__).resolve().parents[3]
APP = ROOT / "apps/android"
TARGETS = {"arm64-v8a": ("aarch64-linux-android", "aarch64-linux-android", 183), "x86_64": ("x86_64-linux-android", "x86_64-linux-android", 62)}

class BuildError(RuntimeError):
    pass

def absolute(value: str) -> Path:
    result = Path(value).expanduser()
    if not result.is_absolute():
        raise argparse.ArgumentTypeError("please supply an absolute path")
    return result

def run(args: list[str | Path], *, capture: bool = False, timeout: int = 900) -> subprocess.CompletedProcess[str]:
    values = [str(arg) for arg in args]
    if not Path(values[0]).is_absolute():
        raise BuildError("tool path must be absolute: " + values[0])
    # inherit the caller's environment without changing or adding variables.
    print("+ " + " ".join(values), flush=True)
    result = subprocess.run(values, cwd=ROOT, text=True, capture_output=capture, timeout=timeout, check=False)
    if result.returncode:
        if capture:
            print(result.stdout, end=""); print(result.stderr, end="", file=sys.stderr)
        raise BuildError(f"command failed ({result.returncode}): {values[0]}")
    return result

def version(path: Path) -> tuple[int, ...]:
    return tuple(int(n) for n in re.findall(r"\d+", path.name))

def latest(directory: Path, label: str) -> Path:
    candidates = [p for p in directory.glob("*") if p.is_dir() and re.fullmatch(r"\d+(\.\d+)*", p.name)]
    if not candidates:
        raise BuildError(f"{label} not visible/installed: {directory}")
    return max(candidates, key=version)

def executable(path: Path) -> Path:
    if not path.is_file() or not os.access(path, os.X_OK):
        raise BuildError(f"tool not visible or executable: {path}")
    return path

def cargo_options(args: argparse.Namespace) -> list[str]:
    values = ["--manifest-path", str(ROOT / "Cargo.toml"), "--locked", "--config", f"build.rustc={json.dumps(str(args.rustc))}"]
    if args.offline:
        values.append("--offline")
    if args.vendor:
        values.extend(["--config", 'source.crates-io.replace-with="android-vendor"', "--config", f"source.android-vendor.directory={json.dumps(str(args.vendor))}"])
    return values

def android_flags(sysroot: Path | None = None) -> list[str]:
    flags = ["-C", "link-arg=-Wl,-z,max-page-size=16384", "-C", "link-arg=-Wl,-z,common-page-size=16384"]
    if sysroot is not None:
        flags += ["--sysroot", str(sysroot)]
    return flags

def java_bootclasspath(found: dict[str, Path]) -> str:
    return os.pathsep.join(str(found[key]) for key in ["lambda_stubs", "platform"])

def prepare_rust(args: argparse.Namespace) -> int:
    if args.offline:
        raise BuildError("prepare-rust needs an explicit online download; build remains offline-capable")
    rust_target.prepare(ROOT, executable(args.rustc), TARGETS[args.abi][0])
    return 0

def tools(args: argparse.Namespace) -> dict[str, Path]:
    if not args.sdk.is_dir():
        raise BuildError(f"SDK root is not visible in this execution environment: {args.sdk}. In EndlessVibe this can mean a missing read-only toolchain mount, not a missing host installation.")
    build = args.sdk / "build-tools" / args.build_tools if args.build_tools else latest(args.sdk / "build-tools", "SDK Build Tools")
    ndk = args.ndk or latest(args.sdk / "ndk", "Android NDK")
    platform = args.sdk / "platforms" / f"android-{args.api}" / "android.jar"
    if not platform.is_file():
        raise BuildError(f"SDK platform jar missing: {platform}")
    llvm = ndk / "toolchains/llvm/prebuilt/linux-x86_64/bin"
    triple, clang_target, _ = TARGETS[args.abi]
    linker = executable(llvm / f"{clang_target}{args.min_api}-clang")
    sysroot = getattr(args, "rust_sysroot", None)
    query = [executable(args.rustc), "--print", "target-libdir", "--target", triple]
    rustlibs = run([*query, *(["--sysroot", sysroot] if sysroot else [])], capture=True).stdout.strip()
    if not list(Path(rustlibs).glob("libstd-*.rlib")) and sysroot is None:
        sysroot = rust_target.prepared(ROOT, args.rustc, triple)
        if sysroot:
            rustlibs = run([*query, "--sysroot", sysroot], capture=True).stdout.strip()
    if not list(Path(rustlibs).glob("libstd-*.rlib")):
        raise BuildError(f"Rust Android standard library missing for {triple}: {rustlibs}. Run the explicit prepare-rust command to add the matching target inside this project, or supply --rust-sysroot. The system Rust installation is left unchanged.")
    result = {"aapt2": executable(build / "aapt2"), "zipalign": executable(build / "zipalign"), "linker": linker, "platform": platform, "java": executable(args.java / "bin/java"), "javac": executable(args.java / "bin/javac"), "keytool": executable(args.java / "bin/keytool"), "ndk": ndk, "build_tools": build}
    if sysroot:
        result["rust_sysroot"] = sysroot
    lambda_stubs = build / "core-lambda-stubs.jar"
    if not lambda_stubs.is_file():
        raise BuildError(f"SDK Java 8 lambda stubs missing: {lambda_stubs}")
    result["lambda_stubs"] = lambda_stubs
    for key, name in [("d8", "d8.jar"), ("apksigner", "apksigner.jar")]:
        path = build / "lib" / name
        if not path.is_file():
            raise BuildError(f"SDK build jar missing: {path}")
        result[key] = path
    return result

def adb_devices() -> str:
    # Query an existing server only. Never start a USB-blind ADB daemon in a sandbox.
    def receive(sock: socket.socket, size: int) -> bytes:
        out = b""
        while len(out) < size:
            chunk = sock.recv(size-len(out))
            if not chunk:
                raise BuildError("ADB server returned a truncated response")
            out += chunk
        return out
    with socket.create_connection(("127.0.0.1", 5037), timeout=2) as sock:
        request = b"host:devices-l"
        sock.sendall(f"{len(request):04x}".encode()+request)
        status = receive(sock, 4)
        size = int(receive(sock, 4), 16)
        if size > 65536:
            raise BuildError("ADB response too large")
        text = receive(sock, size).decode("utf-8", "replace")
        if status != b"OKAY":
            raise BuildError(text)
        return text

def doctor(args: argparse.Namespace) -> int:
    print("SDK:", args.sdk, "(visible)" if args.sdk.is_dir() else "(not visible)")
    print("Java:", args.java, "Cargo:", args.cargo, "Rust:", args.rustc)
    errors = []
    for tool in [args.java / "bin/java", args.java / "bin/javac", args.cargo, args.rustc]:
        try:
            executable(tool)
        except BuildError as error:
            errors.append(str(error))
    try:
        found = tools(args)
        for key, path in found.items():
            print(key + ":", path)
    except (BuildError, rust_target.TargetError) as error:
        errors.append(str(error))
    try:
        devices = adb_devices().strip()
        print("Existing host ADB:", devices or "no devices")
    except (OSError, ValueError, BuildError) as error:
        print("Existing host ADB unavailable:", error)
    for error in errors:
        print("MISSING:", error)
    print("No environment variables or system settings were changed.")
    return int(bool(errors))

def elf_alignment(path: Path, expected_machine: int) -> list[int]:
    data = path.read_bytes()
    if len(data) < 64 or data[:6] != b"\x7fELF\x02\x01":
        raise BuildError("expected little-endian ELF64 native library")
    machine = struct.unpack_from("<H", data, 18)[0]
    if machine != expected_machine:
        raise BuildError(f"wrong native ABI: ELF machine {machine}")
    offset = struct.unpack_from("<Q", data, 32)[0]
    entry_size, count = struct.unpack_from("<HH", data, 54)
    if entry_size < 56 or offset + entry_size * count > len(data):
        raise BuildError("invalid ELF program header table")
    aligns = []
    for i in range(count):
        at = offset + i * entry_size
        if struct.unpack_from("<I", data, at)[0] == 1:
            file_offset, address = struct.unpack_from("<QQ", data, at + 8)
            alignment = struct.unpack_from("<Q", data, at + 48)[0]
            if alignment < 16384 or file_offset % 16384 != address % 16384:
                raise BuildError("native PT_LOAD is not compatible with 16 KiB pages")
            aligns.append(alignment)
    if not aligns:
        raise BuildError("native library has no loadable segments")
    return aligns

def bundled_font(args: argparse.Namespace) -> Path:
    candidates = [args.font] if args.font else []
    if not args.font:
        for profile in ["debug", "release"]:
            candidates += list((ROOT / "target" / profile / "build").glob("readall-*/out/LXGWWenKaiLite-Regular.ttf"))
    for path in candidates:
        if path and path.is_file() and path.stat().st_size == 13872424:
            return path.resolve()
    raise BuildError("Chinese font not found; pass --font /absolute/LXGWWenKaiLite-Regular.ttf or build the desktop app first. No font download is performed.")

def configured_manifest(destination: Path, min_api: int, target_api: int) -> Path:
    # AAPT2's min/target flags are defaults, not replacements for existing values.
    # Change only the generated copy; keep the checked-in manifest unchanged.
    namespace = "http://schemas.android.com/apk/res/android"
    ET.register_namespace("android", namespace)
    tree = ET.parse(APP / "AndroidManifest.xml")
    sdk = tree.getroot().find("uses-sdk")
    if sdk is None:
        raise BuildError("Android manifest has no uses-sdk element")
    sdk.set("{" + namespace + "}minSdkVersion", str(min_api))
    sdk.set("{" + namespace + "}targetSdkVersion", str(target_api))
    tree.write(destination, encoding="utf-8", xml_declaration=True)
    return destination

def build(args: argparse.Namespace) -> int:
    found = tools(args)
    font = bundled_font(args)
    output = ROOT / "target/android"
    output.mkdir(parents=True, exist_ok=True)
    triple, _, machine = TARGETS[args.abi]
    rust_target = output / "rust"
    flags = android_flags(found.get("rust_sysroot"))
    run([executable(args.cargo), "build", "-p", "readall-android", "--release", "--target", triple, "--target-dir", rust_target, *cargo_options(args), "--config", f"target.{triple}.linker={json.dumps(str(found['linker']))}", "--config", f"target.{triple}.rustflags={json.dumps(flags)}"])
    native = rust_target / triple / "release/libreadall_android.so"
    alignments = elf_alignment(native, machine)
    version_name = tomllib.loads((ROOT / "Cargo.toml").read_text())["workspace"]["package"]["version"] + "-android-preview"
    with tempfile.TemporaryDirectory(prefix="build-", dir=output) as temporary:
        stage = Path(temporary)
        assets, classes, generated, dex = [stage / name for name in ["assets", "classes", "generated", "dex"]]
        for path in [assets, classes, generated, dex]:
            path.mkdir()
        shutil.copy2(font, assets / "LXGWWenKaiLite-Regular.ttf")
        for name in ["LXGW_WenKai_Lite_OFL.txt", "foliate-js-MIT.txt"]:
            shutil.copy2(ROOT / "licenses" / name, assets / name)
        compiled = stage / "resources.zip"
        run([found["aapt2"], "compile", "--dir", APP / "res", "-o", compiled])
        base = stage / "resources.apk"
        manifest = configured_manifest(stage / "AndroidManifest.xml", args.min_api, args.api)
        run([found["aapt2"], "link", "-o", base, "--manifest", manifest, "-I", found["platform"], compiled, "-A", assets, "--java", generated, "--min-sdk-version", str(args.min_api), "--target-sdk-version", str(args.api), "--version-name", version_name, "--replace-version"])
        sources = sorted((APP / "src").rglob("*.java")) + sorted(generated.rglob("*.java"))
        # android.jar's LambdaMetafactory is intentionally not javac's bootstrap
        # implementation. Supply SDK stubs first; D8 desugars lambdas afterwards.
        bootclasspath = java_bootclasspath(found)
        run([found["javac"], "-encoding", "UTF-8", "-source", "8", "-target", "8", "-bootclasspath", bootclasspath, "-d", classes, *sources])
        jar = stage / "classes.jar"
        with zipfile.ZipFile(jar, "w", zipfile.ZIP_DEFLATED) as archive:
            for path in sorted(classes.rglob("*.class")):
                archive.write(path, str(path.relative_to(classes)))
        run([found["java"], "-cp", found["d8"], "com.android.tools.r8.D8", "--min-api", str(args.min_api), "--lib", found["platform"], "--output", dex, jar])
        unsigned = stage / "unsigned.apk"
        shutil.copy2(base, unsigned)
        with zipfile.ZipFile(unsigned, "a") as archive:
            for path in sorted(dex.glob("*.dex")):
                archive.write(path, path.name, compress_type=zipfile.ZIP_DEFLATED)
            archive.write(native, f"lib/{args.abi}/libreadall_android.so", compress_type=zipfile.ZIP_STORED)
        aligned = stage / "aligned.apk"
        run([found["zipalign"], "-P", "16", "-f", "4", unsigned, aligned])
        key = output / "debug.keystore"
        if not key.exists():
            run([found["keytool"], "-genkeypair", "-keystore", key, "-storepass", "android", "-keypass", "android", "-alias", "androiddebugkey", "-keyalg", "RSA", "-keysize", "2048", "-validity", "10000", "-dname", "CN=ReadAll Debug,O=ReadAll,C=JP"])
            key.chmod(0o600)
        signed = stage / "signed.apk"
        run([found["java"], "-jar", found["apksigner"], "sign", "--ks", key, "--ks-pass", "pass:android", "--key-pass", "pass:android", "--out", signed, aligned])
        run([found["java"], "-jar", found["apksigner"], "verify", "--verbose", signed])
        run([found["zipalign"], "-c", "-P", "16", "4", signed])
        apk = output / f"readall-android-debug-{args.abi}.apk"
        os.replace(signed, apk)
        (output / "build-report.json").write_text(json.dumps({"apk": str(apk), "sha256": hashlib.sha256(apk.read_bytes()).hexdigest(), "abi": args.abi, "version": version_name, "api": args.api, "min_api": args.min_api, "elf_load_alignment": alignments, "ndk": str(found["ndk"]), "rustc": str(args.rustc), "rust_sysroot": str(found.get("rust_sysroot", "system")), "debug_only": True}, indent=2)+"\n")
        print("APK:", apk)
    return 0

def host_test(args: argparse.Namespace) -> int:
    output = ROOT / "target/android-host"
    output.mkdir(parents=True, exist_ok=True)
    shared = ["--target-dir", str(output), *cargo_options(args)]
    run([executable(args.cargo), "build", "-p", "readall-android", *shared])
    run([args.cargo, "build", "-p", "readall", "--no-default-features", "--features", "mobile", "--example", "mobile_fixture", *shared])
    # Every run gets isolated progress/annotations; prior smoke tests must not
    # change the expected initial page or touch the user's real library.
    fixture = Path(tempfile.mkdtemp(prefix="fixture-", dir=output))
    run([output / "debug/examples/mobile_fixture", fixture])
    classes = output / "java"
    classes.mkdir(exist_ok=True)
    run([executable(args.java / "bin/javac"), "-encoding", "UTF-8", "-d", classes, APP / "src/xin/soymilk/readall/NativeReader.java", APP / "src/xin/soymilk/readall/TouchRouter.java", APP / "tests/JniSmoke.java", APP / "tests/TouchSmoke.java", APP / "tests/UiCapture.java", APP / "src/xin/soymilk/readall/ReaderViewport.java", APP / "tests/ViewportSmoke.java", APP / "tests/DensitySmoke.java"])
    run([executable(args.java / "bin/java"), "-Xcheck:jni", f"-Djava.library.path={output / 'debug'}", "-cp", classes, "xin.soymilk.readall.JniSmoke", fixture], timeout=120)
    run([executable(args.java / "bin/java"), "-cp", classes, "xin.soymilk.readall.TouchSmoke"], timeout=30)
    run([executable(args.java / "bin/java"), "-cp", classes, "xin.soymilk.readall.ViewportSmoke"], timeout=30)
    run([executable(args.java / "bin/java"), "-Xcheck:jni", f"-Djava.library.path={output / 'debug'}", "-cp", classes, "xin.soymilk.readall.DensitySmoke", fixture], timeout=120)
    print("PASS host JVM/JNI/shared UI, native density and touch smoke tests; this is NOT Android device validation.")
    return 0

def install(args: argparse.Namespace) -> int:
    apk = ROOT / "target/android" / f"readall-android-debug-{args.abi}.apk"
    if not apk.is_file():
        raise BuildError("APK does not exist; build and verify it first")
    lines = [line.split() for line in adb_devices().splitlines() if line.strip()]
    authorized = [line[0] for line in lines if len(line)>1 and line[1]=="device"]
    serial = args.serial
    if serial:
        if serial not in authorized:
            raise BuildError("selected ADB serial is not an authorized online device")
    elif len(authorized)==1:
        serial = authorized[0]
    else:
        raise BuildError("select exactly one authorized device with --serial")
    adb = executable(args.adb or args.sdk / "platform-tools/adb")
    prefix = [adb, "-H", "127.0.0.1", "-P", "5037", "-s", serial]
    abi = run([*prefix, "shell", "getprop", "ro.product.cpu.abilist"], capture=True).stdout.strip().split(",")
    if args.abi not in abi:
        raise BuildError("APK ABI does not match the selected device")
    run([*prefix, "install", "-r", apk], timeout=120)
    run([*prefix, "shell", "am", "start", "-n", "xin.soymilk.readall/.MainActivity"], timeout=30)
    return 0

def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["doctor", "prepare-rust", "build", "host-test", "install"])
    parser.add_argument("--sdk", type=absolute, default=Path("/opt/android-sdk"))
    parser.add_argument("--ndk", type=absolute)
    parser.add_argument("--java", type=absolute, default=Path("/usr/lib/jvm/openjdk-17"))
    parser.add_argument("--cargo", type=absolute, default=Path("/usr/bin/cargo"))
    parser.add_argument("--rustc", type=absolute, default=Path("/usr/bin/rustc"))
    parser.add_argument("--rust-sysroot", type=absolute)
    parser.add_argument("--font", type=absolute)
    parser.add_argument("--vendor", type=absolute)
    parser.add_argument("--adb", type=absolute)
    parser.add_argument("--serial")
    parser.add_argument("--api", type=int, default=36)
    parser.add_argument("--min-api", type=int, default=26)
    parser.add_argument("--abi", choices=TARGETS, default="arm64-v8a")
    parser.add_argument("--build-tools")
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    if not (26 <= args.min_api <= args.api <= 99 and args.api >= 33):
        parser.error("require 26 <= min-api <= api <= 99 and compile API >= 33")
    if args.build_tools and not re.fullmatch(r"\d+(\.\d+)*", args.build_tools):
        parser.error("--build-tools must be a numeric SDK package version")
    try:
        return {"doctor": doctor, "prepare-rust": prepare_rust, "build": build, "host-test": host_test, "install": install}[args.command](args)
    except (OSError, BuildError, rust_target.TargetError, subprocess.SubprocessError) as error:
        print("ReadAll Android:", error, file=sys.stderr)
        return 1

if __name__ == "__main__":
    raise SystemExit(main())
