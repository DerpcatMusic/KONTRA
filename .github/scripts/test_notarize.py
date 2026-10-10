#!/usr/bin/env python3
"""Offline notarization orchestration checks; no Apple credentials or acceptance."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HERE = Path(__file__).resolve().parent
ID = '12345678-1234-1234-1234-123456789abc'
MOCK = '''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
log = pathlib.Path(os.environ['CALLS'])
calls = log.read_text().splitlines() if log.exists() else []
with log.open('a') as f: f.write(json.dumps(args[:3]) + '\\n')
case = os.environ['CASE']
if args[:2] == ['notarytool', 'submit']:
    assert '--wait' not in args
    if case == 'upload-error': sys.exit(1)
    print(json.dumps({'id': 'bad' if case == 'bad-id' else '12345678-1234-1234-1234-123456789abc', 'status': 'Uploaded'}))
else:
    assert args[:3] == ['notarytool', 'wait', '12345678-1234-1234-1234-123456789abc']
    assert '--timeout' in args
    if case == 'persistent' or (case == 'resume' and len(calls) == 1): sys.exit(1)
    if case == 'malformed': print('bad-json'); sys.exit(0)
    print(json.dumps({'id': '00000000-0000-4000-8000-000000000000' if case == 'wrong-id' else args[2], 'status': 'Invalid' if case == 'rejected' else 'Accepted'}))
    if case == 'accepted-error': sys.exit(1)
'''


class Notarization(unittest.TestCase):
    def check_case(self, case, accepted, waits):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            mock = root / 'xcrun'
            mock.write_text(MOCK)
            mock.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + os.pathsep + os.environ['PATH'],
                       CASE=case, CALLS=str(root / 'calls'), APPLE_ID='private-id',
                       APPLE_APP_SPECIFIC_PASSWORD='private-password', APPLE_TEAM_ID='private-team')
            result = subprocess.run(['python3', str(HERE / 'notarize.py'), str(root / 'artifact.dmg')],
                                    env=env, capture_output=True, text=True)
            self.assertEqual(result.returncode == 0, accepted, result.stderr)
            calls = [json.loads(row) for row in (root / 'calls').read_text().splitlines()]
            self.assertEqual(sum(c[:2] == ['notarytool', 'submit'] for c in calls), 1)
            self.assertEqual(sum(c[:2] == ['notarytool', 'wait'] for c in calls), waits)
            for secret in ('private-id', 'private-password', 'private-team'):
                self.assertNotIn(secret, result.stdout + result.stderr)
            if accepted:
                self.assertEqual(json.loads(result.stdout), {'id': ID, 'status': 'Accepted'})

    def test_success(self): self.check_case('accepted', True, 1)
    def test_timeout_resumes_same_upload_once(self): self.check_case('resume', True, 2)
    def test_persistent_error_is_bounded(self): self.check_case('persistent', False, 2)
    def test_upload_error_never_resubmits(self): self.check_case('upload-error', False, 0)
    def test_bad_submission_id_never_waits(self): self.check_case('bad-id', False, 0)
    def test_rejection_never_retries(self): self.check_case('rejected', False, 1)
    def test_malformed_success_fails_closed(self): self.check_case('malformed', False, 1)
    def test_wrong_submission_fails_closed(self): self.check_case('wrong-id', False, 1)
    def test_nonzero_accepted_result_fails_closed(self): self.check_case('accepted-error', False, 1)


if __name__ == '__main__': unittest.main()
