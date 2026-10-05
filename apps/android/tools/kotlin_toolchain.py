"""Pinned Kotlin/JVM tooling. No shell profiles, environment variables or SDK changes.

Only prepare-kotlin downloads; normal builds are offline-capable. JetBrains artifacts
come from Maven Central and are pinned by SHA-256. Compiler dependencies (including
reflect/coroutines) are build-time only; the APK receives stdlib and annotations.
Kotlin 2.1.21 is compatible with installed SDK 36.0.0's D8 8.10.9.
Sources: https://developer.android.com/build/kotlin-support
         https://kotlinlang.org/docs/compiler-reference.html
"""
from __future__ import annotations
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.request

VERSION = "2.1.21"
MIN_D8 = (8, 6, 17)
BASE = "https://repo.maven.apache.org/maven2/"
# (group, artifact, version, installed name, SHA-256, exact bytes)
ARTIFACTS = (
    ("org.jetbrains.kotlin", "kotlin-compiler-embeddable", VERSION, "kotlin-compiler.jar", "67a2e3673765f097725608cc7c843b0d8fdd94bd9af41327211361614aa57d49", 56958642),
    ("org.jetbrains.kotlin", "kotlin-stdlib", VERSION, "kotlin-stdlib.jar", "263bdc679e1f62012db7b091796279b6d71cf36f4797a98ff1ace05835f201c8", 1724058),
    ("org.jetbrains.kotlin", "kotlin-script-runtime", VERSION, "kotlin-script-runtime.jar", "d8221f445854f30ac92c248d543609e0ecb5d85bb5ba34c043684b013cb5b897", 43365),
    ("org.jetbrains.kotlin", "kotlin-reflect", "1.6.10", "kotlin-reflect.jar", "3277ac102ae17aad10a55abec75ff5696c8d109790396434b496e75087854203", 3038560),
    ("org.jetbrains.kotlin", "kotlin-daemon-embeddable", VERSION, "kotlin-daemon-embeddable.jar", "9401effd82de8606df3289308a275cc1d3573768665ca5edd09dc319c71715f4", 345429),
    ("org.jetbrains.intellij.deps", "trove4j", "1.0.20200330", "trove4j.jar", "c5fd725bffab51846bf3c77db1383c60aaaebfe1b7fe2f00d23fe1b7df0a439d", 572985),
    ("org.jetbrains.kotlinx", "kotlinx-coroutines-core-jvm", "1.8.0", "kotlinx-coroutines-core-jvm.jar", "9860906a1937490bf5f3b06d2f0e10ef451e65b95b269f22daf68a3d1f5065c5", 1548360),
    ("org.jetbrains", "annotations", "13.0", "annotations-13.0.jar", "ace2a10dc8e2d5fd34925ecac03e4988b2c0f851650c94b8cef49ba1bd111478", 17536),
)

class KotlinError(RuntimeError):
    pass

def default_home(root: Path) -> Path:
    return root / "target/android/toolchains" / f"kotlin-{VERSION}" / "kotlinc"

def cache_directory(root: Path) -> Path:
    return root / "target/android/downloads" / f"kotlin-{VERSION}"

def verify(path: Path, digest: str, size: int) -> None:
    if not path.is_file() or path.is_symlink() or path.stat().st_size != size:
        raise KotlinError("Kotlin artifact missing/invalid size: " + str(path))
    if hashlib.sha256(path.read_bytes()).hexdigest() != digest:
        raise KotlinError("Kotlin artifact SHA-256 mismatch: " + str(path))

def fetch(url: str, destination: Path, digest: str, size: int, offline: bool) -> None:
    if destination.exists():
        verify(destination, digest, size)
        return
    if offline:
        raise KotlinError("Offline Kotlin artifact missing: " + str(destination))
    # Completed artifacts survive interruptions; partial files are never installed.
    destination.parent.mkdir(parents=True, exist_ok=True)
    fd, name = tempfile.mkstemp(prefix="download-", suffix=".part", dir=destination.parent)
    partial = Path(name)
    try:
        with os.fdopen(fd, "wb") as out, urllib.request.urlopen(urllib.request.Request(url, headers={"User-Agent": "ReadAll-build"}), timeout=60) as source:
            count = 0
            while block := source.read(256 * 1024):
                count += len(block)
                if count > size:
                    raise KotlinError("Kotlin download exceeds pinned size")
                out.write(block)
        verify(partial, digest, size)
        os.replace(partial, destination)
    finally:
        partial.unlink(missing_ok=True)

def prepare(root: Path, offline: bool = False) -> Path:
    home = default_home(root)
    if home.exists():
        validate(home)
        return home
    for group, artifact, version, name, digest, size in ARTIFACTS:
        filename = f"{artifact}-{version}.jar"
        url = BASE + group.replace(".", "/") + f"/{artifact}/{version}/{filename}"
        fetch(url, cache_directory(root) / filename, digest, size, offline)
    home.parent.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="kotlin-stage-", dir=home.parent.parent) as directory:
        stage = Path(directory) / "distribution"
        lib = stage / "kotlinc/lib"
        lib.mkdir(parents=True)
        for _, artifact, version, name, digest, size in ARTIFACTS:
            source = cache_directory(root) / f"{artifact}-{version}.jar"
            verify(source, digest, size)
            shutil.copyfile(source, lib / name)
        (stage / "kotlinc/build.txt").write_text(VERSION + "\n")
        (stage / "readall-toolchain.json").write_text(json.dumps({"version": VERSION, "source": BASE, "sha256": {a[3]: a[4] for a in ARTIFACTS}}, indent=2) + "\n")
        if home.parent.exists():
            raise KotlinError("Kotlin destination appeared during preparation; refusing replacement")
        os.rename(stage, home.parent)
    validate(home)
    return home

def validate(home: Path) -> None:
    if not home.is_absolute():
        raise KotlinError("Kotlin home must be absolute")
    build = home / "build.txt"
    if not build.is_file() or VERSION not in build.read_text().strip():
        raise KotlinError(f"Kotlin {VERSION} missing at {home}; run prepare-kotlin or pass --kotlin /absolute/kotlinc")
    if (home.parent / "readall-toolchain.json").is_file():
        for _, _, _, name, digest, size in ARTIFACTS:
            verify(home / "lib" / name, digest, size)
    else:
        for name in ("kotlin-compiler.jar", "kotlin-stdlib.jar", "annotations-13.0.jar"):
            if not (home / "lib" / name).is_file():
                raise KotlinError("Kotlin library missing: " + name)

def runtime(home: Path) -> list[Path]:
    return [home / "lib/kotlin-stdlib.jar", home / "lib/annotations-13.0.jar"]

def d8_version(value: str) -> tuple[int, int, int]:
    match = re.search(r"\bD8\s+(\d+)\.(\d+)\.(\d+)", value)
    if not match:
        raise KotlinError("Cannot identify D8 version")
    version = tuple(int(n) for n in match.groups())
    if version < MIN_D8:
        raise KotlinError(f"Kotlin {VERSION} requires D8 >= {'.'.join(map(str, MIN_D8))}")
    return version

def check_d8(java: Path, jar: Path) -> tuple[int, int, int]:
    result = subprocess.run([str(java), "-cp", str(jar), "com.android.tools.r8.D8", "--version"], capture_output=True, text=True, timeout=30, check=True)
    return d8_version(result.stdout + result.stderr)

def compile_command(java_home: Path, kotlin: Path, sources: list[Path], output: Path, classpath: list[Path], *, android: bool = False) -> list[str]:
    validate(kotlin)
    for path in (java_home, kotlin, output, *sources, *classpath):
        if not path.is_absolute():
            raise KotlinError("Compiler paths must be absolute: " + str(path))
    command = [str(java_home / "bin/java"), "-Xmx768m", "-cp", str(kotlin / "lib/*"), "org.jetbrains.kotlin.cli.jvm.K2JVMCompiler", "-kotlin-home", str(kotlin), "-no-stdlib", "-no-reflect", "-jvm-target", "1.8", "-module-name", "readall_android", "-classpath", os.pathsep.join(map(str, [*classpath, *runtime(kotlin)])), "-d", str(output)]
    command += ["-no-jdk"] if android else ["-jdk-home", str(java_home), "-Xjdk-release=8"]
    return command + [str(p) for p in sources]
