"""Offline tests: fake distribution metadata and archives, no network or system writes."""
import hashlib
import io
from pathlib import Path
import tarfile
import tempfile
import unittest
from unittest.mock import patch
import rust_target as target

TRIPLE='aarch64-linux-android'
IDENTITY='1.97.1 (8bab26f4f 2026-07-14)'
PREFIX=f'rust-std-1.97.1-{TRIPLE}/rust-std-{TRIPLE}/lib/rustlib/{TRIPLE}/lib/'

def manifest(identity=IDENTITY, url='https://static.rust-lang.org/dist/sample.tar.xz', digest='a'*64):
    return (f'[pkg.rust-std]\nversion="{identity}"\n[pkg.rust-std.target.{TRIPLE}]\navailable=true\nxz_url="{url}"\nxz_hash="{digest}"\n').encode()

def archive(path, name, data=b'fake library', kind=tarfile.REGTYPE):
    with tarfile.open(path,'w:xz') as tar:
        entry=tarfile.TarInfo(name);entry.type=kind
        if kind==tarfile.REGTYPE:
            entry.size=len(data);tar.addfile(entry,io.BytesIO(data))
        else:
            entry.linkname='/etc/passwd';tar.addfile(entry)

class RustTargetTests(unittest.TestCase):
    def test_version_target_and_https_source_are_exact(self):
        self.assertEqual(target.select_component(manifest(),IDENTITY,TRIPLE)['xz_hash'],'a'*64)
        for data in [manifest(identity='1.96.0 (deadbeef 2026-06-01)'),manifest(url='https://example.invalid/dist/archive'),manifest(digest='bad')]:
            with self.assertRaises(target.TargetError):target.select_component(data,IDENTITY,TRIPLE)
        for url in ['http://static.rust-lang.org/dist/x','https://static.rust-lang.org.evil/dist/x','https://static.rust-lang.org/dist/x?redirect=y','file:///tmp/target']:
            with self.assertRaises(target.TargetError):target.official_url(url)
        with self.assertRaises(target.TargetError):target.destination(Path('/project'),'../../escape',TRIPLE)
        with self.assertRaises(target.TargetError):target.destination(Path('/project'),'1.97.1','not-a-target')

    def test_only_expected_regular_libraries_are_extracted_and_hashed(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);a=root/'test.tar.xz';stage=root/'sysroot'
            archive(a,PREFIX+'libstd-test.rlib')
            hashes=target.unpack(a,stage,'1.97.1',TRIPLE)
            name=f'lib/rustlib/{TRIPLE}/lib/libstd-test.rlib'
            self.assertEqual(hashes,{name:hashlib.sha256(b'fake library').hexdigest()})
            self.assertEqual((stage/name).read_bytes(),b'fake library')

    def test_unsafe_paths_links_and_wrong_component_are_rejected(self):
        for name,kind in [('../escape',tarfile.REGTYPE),('/absolute/file',tarfile.REGTYPE),(PREFIX+'libstd-test.rlib',tarfile.SYMTYPE),(PREFIX+'libstd-test.rlib',tarfile.LNKTYPE),(PREFIX+'nested/libstd-test.rlib',tarfile.REGTYPE),('other/libstd-test.rlib',tarfile.REGTYPE)]:
            with self.subTest(name=name,kind=kind),tempfile.TemporaryDirectory() as directory:
                root=Path(directory);a=root/'test.tar.xz';archive(a,name,kind=kind)
                with self.assertRaises(target.TargetError):target.unpack(a,root/'sysroot','1.97.1',TRIPLE)
                self.assertFalse((root/'escape').exists())

    def test_bad_manifest_checksum_fails_before_component_download(self):
        with tempfile.TemporaryDirectory() as directory,patch.object(target,'compiler',return_value=('1.97.1',IDENTITY)),patch.object(target,'fetch',side_effect=[manifest(),('0'*64).encode()]) as fetch:
            with self.assertRaisesRegex(target.TargetError,'checksum'):target.prepare(Path(directory),Path('/usr/bin/rustc'),TRIPLE)
            self.assertEqual(fetch.call_count,2)

    def test_existing_unrecognized_directory_is_not_overwritten(self):
        with tempfile.TemporaryDirectory() as directory,patch.object(target,'compiler',return_value=('1.97.1',IDENTITY)),patch.object(target,'fetch',side_effect=AssertionError('must not download')):
            root=Path(directory);output=target.destination(root,'1.97.1',TRIPLE);output.mkdir(parents=True);marker=output/'user-file';marker.write_text('preserve')
            with self.assertRaisesRegex(target.TargetError,'overwrite'):target.prepare(root,Path('/usr/bin/rustc'),TRIPLE)
            self.assertEqual(marker.read_text(),'preserve')

if __name__=='__main__':unittest.main()
