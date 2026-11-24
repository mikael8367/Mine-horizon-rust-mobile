import importlib.util
import pathlib
import tempfile
import unittest
import zipfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('check_apk_ci', ROOT / 'check-apk-ci.py')
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)


class CheckApkTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.dir = pathlib.Path(self.temp.name)
        self.lib = self.dir / 'lib.so'
        self.lib.write_bytes(b'test-library')
        self.checker = self.dir / 'fake_checker.py'
        self.checker.write_text('import sys\nsys.exit(0)\n')
        self.apk = self.dir / 'test.apk'

    def write_apk(self, library=b'test-library', extra=None):
        with zipfile.ZipFile(self.apk, 'w') as z:
            z.writestr('lib/arm64-v8a/libhorizon_gpui_prova.so', library)
            if extra:
                z.writestr(extra, b'extra')

    def test_matching_library_passes(self):
        self.write_apk()
        check.check_apk(self.apk, self.lib, 'fake-readelf', self.checker)

    def test_different_library_rejected(self):
        self.write_apk(b'wrong')
        with self.assertRaises(ValueError):
            check.check_apk(self.apk, self.lib, 'fake-readelf', self.checker)

    def test_unexpected_abi_rejected(self):
        self.write_apk(extra='lib/x86/libother.so')
        with self.assertRaises(ValueError):
            check.check_apk(self.apk, self.lib, 'fake-readelf', self.checker)

    def test_jni_symbols_required(self):
        kt = self.dir / 'GpuiActivity.kt'
        nm = self.dir / 'nm.txt'
        kt.write_text('@JvmStatic external fun start()\n@JvmStatic external fun frame()\n@JvmStatic external fun touch()')
        nm.write_text('0 T Java_org_holool_gpui_GpuiNative_start\n0 T Java_org_holool_gpui_GpuiNative_frame\n')
        with self.assertRaisesRegex(ValueError, 'touch'):
            check.check_jni(kt, nm)
        with nm.open('a') as f:
            f.write('0 T Java_org_holool_gpui_GpuiNative_touch\n')
        check.check_jni(kt, nm)
