#!/usr/bin/env python3
"""Privacy and read-only boundary tests for the memory-only inspector."""
import importlib.util
import json
from pathlib import Path
import threading
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
from urllib.request import Request, urlopen

SPEC = importlib.util.spec_from_file_location("inspector", Path(__file__).with_name("library-inspector.py"))
inspector = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(inspector)


def authored_report():
    return {"format": "kontra-library-inspector", "schema": 1,
            "item": {"path": "authored.nki", "kind": "kontakt", "program": None},
            "stages": [{"name": "script frontend", "status": "ok"}],
            "scripts": [{"id": "ksp:slot0", "source": 'on init\nmessage("authored-only")\nend on'}]}


class InspectorTests(unittest.TestCase):
    def test_initialize_has_only_owned_ram_writable(self):
        with patch.object(inspector.shutil, "which", return_value="/usr/bin/bwrap"):
            args = inspector.inspect_command(Path("/bin/inspect"), "/library/Piano.nki",
                initialize=True, source=True, program="2", ram=Path("/dev/shm/owned"))
        self.assertIn("--initialize", args)
        self.assertIn("--ephemeral-source", args)
        self.assertEqual(args[args.index("--ro-bind")+1:args.index("--ro-bind")+3], ["/", "/"])
        i = args.index("--bind")
        self.assertEqual(args[i+1:i+3], ["/dev/shm/owned", "/dev/shm/owned"])
        self.assertEqual(args.count("--bind"), 1)

    def test_unsandboxed_initialization_is_refused(self):
        with patch.object(inspector.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "requires bubblewrap"):
                inspector.inspect_command(Path("/bin/inspect"), "fixture.nki",
                    initialize=True, source=False, program=None, ram=Path("/dev/shm/owned"))

    def test_profile_requires_the_same_read_only_initializer(self):
        with patch.object(inspector.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "requires bubblewrap"):
                inspector.inspect_command(Path("/bin/inspect"), "Authored.uvip",
                    initialize=False, source=False, program=None,
                    ram=Path("/dev/shm/owned"), profile_init=True)
        with patch.object(inspector.shutil, "which", return_value="/usr/bin/bwrap"):
            args = inspector.inspect_command(Path("/bin/inspect"), "Authored.uvip",
                initialize=False, source=False, program=None,
                ram=Path("/dev/shm/owned"), profile_init=True, ui_only=True)
        self.assertIn("--initialize", args)
        self.assertIn("--profile-init", args)
        self.assertIn("--ui-only", args)

    def test_render_requires_sandbox_and_preserves_snapshot_base(self):
        with patch.object(inspector.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "requires bubblewrap"):
                inspector.inspect_command(Path("/bin/inspect"), "Authored.nksn",
                    initialize=False, render=True, source=False, program=None,
                    ram=Path("/dev/shm/owned"), base="Original.nki")
        with patch.object(inspector.shutil, "which", return_value="/usr/bin/bwrap"):
            args = inspector.inspect_command(Path("/bin/inspect"), "Authored.nksn",
                initialize=False, render=True, source=False, program=None,
                ram=Path("/dev/shm/owned"), base="Original.nki")
        self.assertIn("--initialize", args)
        self.assertIn("--ephemeral-render", args)
        self.assertEqual(args[args.index("--base")+1], "Original.nki")

    def test_lifecycle_requires_read_only_sandbox_and_preserves_selected_program(self):
        with patch.object(inspector.shutil, "which", return_value=None):
            with self.assertRaisesRegex(RuntimeError, "bubblewrap"):
                inspector.inspect_command(Path("/bin/inspect"), "Authored.nksn",
                                          initialize=False, source=False, program="2",
                                          ram=Path("/dev/shm/owned"), settle_ui=True)
        with patch.object(inspector.shutil, "which", return_value="/bin/bwrap"):
            args=inspector.inspect_command(Path("/bin/inspect"), "Authored.nksn",
                                           initialize=False, source=False, program="2",
                                           ram=Path("/dev/shm/owned"), settle_ui=True,
                                           base="Original.nki")
        self.assertIn("--settle-ui",args)
        self.assertIn("--initialize",args)
        self.assertEqual(args[args.index("--program")+1],"2")
        self.assertEqual(args[args.index("--base")+1],"Original.nki")

    def test_native_pixels_have_only_private_memory_routes(self):
        report = authored_report()
        pixels = b"\x89PNG\r\n\x1a\nAUTHORED_TEST_PIXELS"
        report["ui"] = {"interfaces": [{"render": {"shots": [{"png_hex": pixels.hex()}]}}]}
        server, url = inspector.make_server([report])
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with urlopen(url + "report") as response:
                result = json.load(response)
            shot = result[0]["ui"]["interfaces"][0]["render"]["shots"][0]
            self.assertNotIn("png_hex", shot)
            self.assertEqual(shot["image_url"], "image/0/0/0")
            with urlopen(url + shot["image_url"]) as response:
                self.assertEqual(response.headers["Content-Type"], "image/png")
                self.assertEqual(response.headers["Cache-Control"], "no-store")
                self.assertEqual(response.read(), pixels)
            with self.assertRaises(HTTPError) as failure:
                urlopen(url + "image/0/0/../etc/passwd")
            failure.exception.close()
            self.assertIn("png_hex", report["ui"]["interfaces"][0]["render"]["shots"][0])
        finally:
            server.shutdown()
            server.server_close()
            thread.join()

    def test_manifest_bank_member_is_preserved(self):
        with patch.object(inspector.shutil, "which", return_value=None):
            args = inspector.inspect_command(Path("/bin/inspect"), "fixture.ufs::Presets/Alto Flute.uvip",
                initialize=False, source=False, program=None, ram=Path("/dev/shm/owned"))
        self.assertEqual(args[-3:], ["fixture.ufs", "--program", "Presets/Alto Flute.uvip"])
        self.assertNotIn("--initialize", args)

    def test_old_or_unrelated_reports_are_not_silently_accepted(self):
        with self.assertRaises(ValueError):
            inspector.validate_reports({"load": {"ok": True}})
        with self.assertRaises(ValueError):
            inspector.validate_reports([authored_report(), {"format": "other"}])

    def test_timeout_progress_never_repeats_raw_library_diagnostics(self):
        stderr = b'PRIVATE_SOURCE_PAYLOAD\nINSPECT_PROGRESS {"phase":"container","elapsed_ms":2}\nINSPECT_PROGRESS {"phase":"source_frontend","status":"started"}\n'
        self.assertEqual(inspector.last_progress(stderr), "source_frontend (started)")
        self.assertIsNone(inspector.last_progress(b'PRIVATE_SOURCE_PAYLOAD\nINSPECT_PROGRESS {"phase":"<script>vendor text</script>"}\n'))

    def test_memory_server_does_not_embed_source_in_html_or_serve_files(self):
        report = authored_report()
        server, url = inspector.make_server([report])
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            with urlopen(url) as response:
                self.assertEqual(response.headers["Cache-Control"], "no-store")
                self.assertNotIn(b"authored-only", response.read())
            with urlopen(url + "report") as response:
                self.assertEqual(json.load(response), [report])
                self.assertEqual(response.headers["Cache-Control"], "no-store")
                self.assertIsNone(response.headers.get("Access-Control-Allow-Origin"))
            with self.assertRaises(HTTPError) as failure:
                urlopen(url + "../../etc/passwd")
            self.assertEqual(failure.exception.code, 404)
            failure.exception.close()
            with self.assertRaises(HTTPError) as failure:
                urlopen(Request(url + "report", headers={"Host": "foreign.example"}))
            self.assertEqual(failure.exception.code, 403)
            failure.exception.close()
        finally:
            server.shutdown()
            server.server_close()
            thread.join()


if __name__ == "__main__":
    unittest.main()
