#!/usr/bin/python3
"""Offline tests for pinned Kotlin artifacts, compiler flags and D8 compatibility."""
import hashlib
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import kotlin_toolchain as k

class KotlinToolchainTests(unittest.TestCase):
    def test_sdk_d8_compatibility(self):
        self.assertEqual(k.d8_version("D8 8.10.9-dev (build sample)"), (8, 10, 9))
        self.assertEqual(k.d8_version("D8 8.6.17"), k.MIN_D8)
        for value in ("D8 8.6.16", "D8 7.0.0", "not D8"):
            with self.assertRaises(k.KotlinError): k.d8_version(value)
    def test_pins_are_complete_and_unique(self):
        self.assertEqual(len({a[3] for a in k.ARTIFACTS}), len(k.ARTIFACTS))
        for _, _, _, name, digest, size in k.ARTIFACTS:
            self.assertRegex(digest, "^[0-9a-f]{64}$")
            self.assertEqual(Path(name).name, name)
            self.assertTrue(0 < size < 70 * 1024 * 1024)
    def test_runtime_excludes_compiler_reflection_and_coroutines(self):
        self.assertEqual([p.name for p in k.runtime(Path("/compiler"))], ["kotlin-stdlib.jar", "annotations-13.0.jar"])
    def test_verify_rejects_modified_artifacts(self):
        with tempfile.TemporaryDirectory() as temp:
            p = Path(temp) / "artifact.jar"; p.write_bytes(b"123")
            digest = hashlib.sha256(b"123").hexdigest(); k.verify(p, digest, 3)
            for checksum, size in (("0" * 64, 3), (digest, 4)):
                with self.assertRaises(k.KotlinError): k.verify(p, checksum, size)
            alias = Path(temp) / "alias.jar"; alias.symlink_to(p)
            with self.assertRaises(k.KotlinError): k.verify(alias, digest, 3)
    def test_offline_missing_artifact_never_downloads(self):
        with tempfile.TemporaryDirectory() as temp, patch("urllib.request.urlopen") as url:
            with self.assertRaises(k.KotlinError): k.fetch("https://example.invalid", Path(temp) / "missing", "0" * 64, 3, True)
            url.assert_not_called()
    def test_compile_uses_absolute_tools_without_environment_changes(self):
        with patch.object(k, "validate"):
            args = k.compile_command(Path("/java home"), Path("/kotlin home"), [Path("/source file.kt")], Path("/classes"), [Path("/android.jar")], android=True)
            self.assertEqual(args[0], "/java home/bin/java")
            self.assertIn("-no-jdk", args); self.assertIn("-no-stdlib", args); self.assertIn("-no-reflect", args)
            self.assertEqual(args[-1], "/source file.kt")
            self.assertNotIn("-include-runtime", args)
            with self.assertRaises(k.KotlinError): k.compile_command(Path("relative"), Path("/kotlin"), [], Path("/classes"), [])
    def test_handwritten_java_is_refused_even_with_a_kotlin_replacement(self):
        import build
        with tempfile.TemporaryDirectory() as temp:
            root = Path(temp); (root / "Example.kt").write_text("class Example")
            self.assertEqual(build.preferred_sources(root), [root / "Example.kt"])
            (root / "Example.java").write_text("class Example {}")
            with self.assertRaises(build.BuildError): build.preferred_sources(root)
    def test_current_application_and_tests_are_kotlin_only(self):
        import build
        for directory in (build.APP / "src", build.APP / "tests"):
            sources = build.preferred_sources(directory)
            self.assertTrue(sources)
            self.assertTrue(all(p.suffix == ".kt" for p in sources))
        self.assertTrue((build.ROOT / "licenses/Kotlin-Apache-2.0.txt").is_file())
        self.assertTrue((build.ROOT / "licenses/Kotlin-runtime-NOTICE.txt").is_file())
    def test_host_uses_java8_api_surface(self):
        with patch.object(k, "validate"):
            args = k.compile_command(Path("/java"), Path("/kotlin"), [], Path("/classes"), [])
            self.assertIn("-Xjdk-release=8", args); self.assertNotIn("-no-jdk", args)

if __name__ == "__main__": unittest.main()
