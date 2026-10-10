#!/usr/bin/env python3
"""CPU driver admission regressions; no plugin, library, host or process runs."""
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('native_cpu', Path(__file__).with_name('cpu-audit-native.py'))
audit = importlib.util.module_from_spec(spec)
spec.loader.exec_module(audit)


class Admission(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.attempt = 0
        self.plugin, self.cli, self.host = [self.root / name for name in ('KONTRA.clap', 'cli', 'host')]
        for path in (self.plugin, self.cli, self.host):
            path.write_bytes(path.name.encode())
        self.build = dict(source_sha='a' * 40, profile='release', path=str(self.plugin),
                          sha256=audit.sha(self.plugin), cli_sha256=audit.sha(self.cli),
                          host_sha256=audit.sha(self.host),
                          host_source_sha256=audit.sha(Path(audit.__file__).resolve().parents[1] / 'vendor/moose-clap/tests/live_performance.cpp'))
        self.save_build(self.build)
        flags = self.root / '.cache'
        flags.mkdir()
        for name in ('request', 'granted'):
            (flags / ('kontra-quiet-' + name)).write_text(json.dumps(dict(owner='fixture')))

    def save_build(self, row):
        (self.root / 'BUILD.json').write_text(json.dumps(row))

    def run_driver(self, extra=(), cold=None, rejected=True):
        self.attempt += 1
        out = self.root / f'out-{self.attempt}'
        argv = ['cpu-audit-native', 'strings', '64', str(out),
                '--host', str(self.host), '--version', 'v2', '--plugin', str(self.plugin),
                '--cli', str(self.cli), '--quiet-owner', 'fixture', *extra]
        def run(command, **kwargs):
            if 'export-multi-state' in command:
                Path(command[-1]).write_bytes(b'fixture state')
            if hasattr(kwargs.get('stdout'), 'write') and cold is not None:
                kwargs['stdout'].write(json.dumps(cold))
        with patch.object(sys, 'argv', argv), patch.object(Path, 'home', return_value=self.root), \
                patch.dict(os.environ, KONTRA_QUIET_OWNER='1'), \
                patch.object(audit.subprocess, 'run', side_effect=run) as process, \
                patch.object(audit, 'observe', return_value={'status': 'MEASURED'}) as observe, \
                patch.object(Path, 'is_file', return_value=True), patch('builtins.print'):
            if rejected:
                with self.assertRaises(AssertionError):
                    audit.main()
                observe.assert_not_called()
                if cold is None:
                    self.assertFalse(any('export-multi-state' in call.args[0] for call in process.call_args_list))
            else:
                self.assertEqual(audit.main(), 0)
                observe.assert_called_once()
                row = json.loads((out / 'metrics.json').read_text())
                self.assertEqual(row['source_sha'], self.build['source_sha'])
                self.assertEqual(row['artifact_receipt'], self.build)
                self.assertEqual(row['cache'], cold)

    def test_v2_requires_selected_source(self):
        self.run_driver()
        self.run_driver(['--source-sha', 'b' * 40])

    def test_v2_rejects_changed_binaries_before_state_export(self):
        for path in (self.plugin, self.cli, self.host):
            original = path.read_bytes()
            path.write_bytes(b'changed')
            self.run_driver(['--source-sha', 'a' * 40])
            path.write_bytes(original)

    def test_v2_rejects_nonrelease_profile_before_state_export(self):
        for profile in ('ci', 'debug', None):
            for extra in ([], ['--profile']):
                with self.subTest(profile=profile, extra=extra):
                    self.save_build(dict(self.build, profile=profile))
                    self.run_driver(['--source-sha', 'a' * 40, *extra])

    def test_v2_rejects_unverified_host_source(self):
        for value in (None, 'b' * 64):
            self.save_build(dict(self.build, host_source_sha256=value))
            self.run_driver(['--source-sha', 'a' * 40])

    def test_residual_cold_pages_never_reach_audition(self):
        self.run_driver(['--source-sha', 'a' * 40, '--cold'],
                        dict(files=1, pages_total=10, pages_before=10, pages_after=1))

    def test_verified_cold_run_retains_build_and_cache_receipts(self):
        self.run_driver(['--source-sha', 'a' * 40, '--cold'],
                        dict(files=1, pages_total=10, pages_before=10, pages_after=0), rejected=False)

    def test_cold_receipt_numeric_boundary(self):
        path = self.root / 'cache.json'
        row = dict(files=1, pages_total=10, pages_before=10, pages_after=0)
        path.write_text(json.dumps(row))
        self.assertEqual(audit.cold_receipt(path), row)
        for key, value in [('files', 0), ('pages_total', 0), ('pages_before', -1),
                           ('pages_before', 11), ('pages_after', 1), ('pages_after', False),
                           ('pages_total', '10'), ('pages_before', None)]:
            path.write_text(json.dumps(dict(row, **{key: value})))
            with self.assertRaises(AssertionError):
                audit.cold_receipt(path)
        path.write_text('{}')
        with self.assertRaises(AssertionError):
            audit.cold_receipt(path)


if __name__ == '__main__':
    unittest.main()
