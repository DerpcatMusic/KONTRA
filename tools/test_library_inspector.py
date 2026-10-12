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
