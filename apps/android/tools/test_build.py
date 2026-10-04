"""Host-only build tests. Never installs an SDK or changes an ADB device."""
import argparse
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest
import xml.etree.ElementTree as ET
spec=importlib.util.spec_from_file_location('android_build',Path(__file__).with_name('build.py'))
build=importlib.util.module_from_spec(spec)
spec.loader.exec_module(build)

class BuildTests(unittest.TestCase):
    def test_absolute_paths(self):
        self.assertEqual(build.absolute('/opt/android-sdk'),Path('/opt/android-sdk'))
        for value in ['android-sdk','$ANDROID_HOME','./sdk']:
            with self.assertRaises(argparse.ArgumentTypeError): build.absolute(value)
        with self.assertRaises(build.BuildError): build.run(['cargo','--version'])

    def test_explicit_cargo_config_without_environment_variables(self):
        args=argparse.Namespace(rustc=Path('/opt/rust/bin/rustc'),offline=True,vendor=Path('/tmp/vendor'))
        options=build.cargo_options(args)
        self.assertIn('build.rustc="/opt/rust/bin/rustc"',options)
        self.assertIn('--offline',options)
        self.assertFalse(any('env.' in value or 'ANDROID_HOME' in value or 'JAVA_HOME' in value for value in options))

    def test_installed_numeric_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            for name in ['9.0.0','28.2.13676358','30.0.0-preview']: (root/name).mkdir()
            self.assertEqual(build.latest(root,'test').name,'28.2.13676358')

    def test_elf_machine_and_all_load_segments(self):
        with tempfile.TemporaryDirectory() as directory:
            library=Path(directory)/'libtest.so';data=bytearray(176)
            data[:6]=b'\x7fELF\x02\x01'
            struct.pack_into('<H',data,18,183)
            struct.pack_into('<Q',data,32,64)
            struct.pack_into('<HH',data,54,56,2)
            for at in [64,120]:
                struct.pack_into('<I',data,at,1)
                struct.pack_into('<QQ',data,at+8,0x4000,0x8000)
                struct.pack_into('<Q',data,at+48,0x4000)
            library.write_bytes(data)
            self.assertEqual(build.elf_alignment(library,183),[16384,16384])
            with self.assertRaises(build.BuildError): build.elf_alignment(library,62)
            struct.pack_into('<Q',data,168,0x1000);library.write_bytes(data)
            with self.assertRaises(build.BuildError): build.elf_alignment(library,183)

    def test_manifest_permissions_and_packaging(self):
        root=ET.parse(build.APP/'AndroidManifest.xml').getroot();ns='{http://schemas.android.com/apk/res/android}'
        self.assertEqual(root.findall('uses-permission'),[])
        self.assertEqual(root.find('application').attrib[ns+'extractNativeLibs'],'false')
        self.assertEqual(root.find('uses-sdk').attrib[ns+'minSdkVersion'],'26')

    def test_requested_sdk_values_only_change_generated_manifest(self):
        original=(build.APP/'AndroidManifest.xml').read_bytes()
        with tempfile.TemporaryDirectory() as directory:
            output=build.configured_manifest(Path(directory)/'AndroidManifest.xml',28,35)
            root=ET.parse(output).getroot();ns='{http://schemas.android.com/apk/res/android}'
            self.assertEqual(root.find('uses-sdk').attrib[ns+'minSdkVersion'],'28')
            self.assertEqual(root.find('uses-sdk').attrib[ns+'targetSdkVersion'],'35')
            self.assertEqual(root.findall('uses-permission'),[])
        self.assertEqual((build.APP/'AndroidManifest.xml').read_bytes(),original)

if __name__=='__main__': unittest.main()
