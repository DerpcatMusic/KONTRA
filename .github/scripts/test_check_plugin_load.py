from pathlib import Path
import unittest
from unittest.mock import patch

from check_plugin_load import binaries, check_binary, check_windows_imports


class PluginLoadTests(unittest.TestCase):
    def test_rejects_normal_and_delay_loaded_redistributable(self):
        for dll in (
            "VCRUNTIME140.dll",
            "MSVCP140_1.dll",
            "concrt140.dll",
        ):
            for table in ("imports", "delay load imports"):
                with self.subTest(dll=dll, table=table):
                    with self.assertRaisesRegex(ValueError, f"dynamic MSVC runtime imports: {dll.lower()}"):
                        check_windows_imports(f"  Section contains the following {table}:\n\n    {dll}\n")

    def test_accepts_windows_system_libraries(self):
        check_windows_imports("Image has the following dependencies:\n    KERNEL32.dll\n    ucrtbase.dll\n")
        for table in ("imports", "delay load imports"):
            with self.subTest(table=table):
                check_windows_imports(
                    f"  Section contains the following {table}:\n\n    api-ms-win-crt-runtime-l1-1-0.dll\n"
                )
        with self.assertRaisesRegex(ValueError, "no DLL imports"):
            check_windows_imports("unrecognized output")

    def test_checks_the_architecture_binary_inside_each_vst3_bundle(self):
        for platform, vst3 in (
            ("linux-x86_64", "Contents/x86_64-linux/KONTRA.so"),
            ("windows-x86_64", "Contents/x86_64-win/KONTRA.vst3"),
        ):
            with self.subTest(platform=platform):
                root = Path("bundles")
                self.assertEqual(binaries(platform, root), [
                    (root / "KONTRA.clap", "clap_entry"),
                    (root / "KONTRA.vst3" / vst3, "GetPluginFactory"),
                ])
        self.assertEqual(binaries("windows-x86_64", root, installed=True), [
            (root / "CLAP/KONTRA.clap", "clap_entry"),
            (root / "VST3/KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3", "GetPluginFactory"),
        ])

    def test_load_reports_a_missing_export(self):
        path = Path(__file__)
        with patch("check_plugin_load.subprocess.check_output", return_value=""), \
                patch("check_plugin_load.ctypes.CDLL", return_value=object()):
            with self.assertRaisesRegex(AttributeError, "clap_entry"):
                check_binary(path, "clap_entry")
