#!/usr/bin/env python3
"""Upload once; resume a failed Apple status wait once, without bypassing acceptance."""
import json
import os
import subprocess
import sys
import uuid


def notarize(artifact):
    auth = ['--apple-id', os.environ['APPLE_ID'], '--password',
            os.environ['APPLE_APP_SPECIFIC_PASSWORD'], '--team-id', os.environ['APPLE_TEAM_ID']]
    result = {}

    def run(command, expected_id=None):
        nonlocal result
        # Apple transport errors contain unreviewed server text. Log only safe result fields.
        child = subprocess.run(['xcrun', 'notarytool', *command, *auth, '--output-format', 'json'],
                               stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)
        try:
            data = json.loads(child.stdout)
        except ValueError:
            # A failed wait can return no JSON. Keep the known upload identity for one retry.
            return child.returncode if child.returncode and expected_id is not None else None
        try:
            submission = str(uuid.UUID(data['id']))
            if expected_id is not None and submission != expected_id:
                raise ValueError('Unexpected submission')
            # A submit without --wait acknowledges the upload with id/path,
            # not a final status. Only a wait for that ID can prove acceptance.
            # https://developer.apple.com/documentation/security/customizing-the-notarization-workflow
            state = data.get('status', 'Uploaded') if expected_id is None else data['status']
            if state not in ('Uploaded', 'In Progress', 'Accepted', 'Invalid', 'Rejected'):
                raise ValueError('Unexpected status')
            if 'statusCode' in data and type(data['statusCode']) is not int:
                raise ValueError('Unexpected status code')
            if state == 'Accepted' and child.returncode != 0:
                raise ValueError('Failed acceptance command')
            safe = dict(id=submission, status=state)
            if 'statusCode' in data:
                safe['statusCode'] = data['statusCode']
            result = safe
        except (ValueError, KeyError, TypeError, AttributeError):
            return None
        return child.returncode

    status = run(['submit', artifact])
    if status != 0:
        return False, result
    submission = result['id']
    if result['status'] not in ('Uploaded', 'In Progress', 'Accepted'):
        return False, result
    # Split submit/wait so a transport timeout cannot lose the upload's submission ID.
    for timeout in ('30m', '5m'):
        result = dict(id=submission, status='In Progress')
        status = run(['wait', submission, '--timeout', timeout], submission)
        if status == 0:
            return result['status'] == 'Accepted', result
        if status is None or result['status'] in ('Accepted', 'Invalid', 'Rejected'):
            return False, result
        print('Apple status wait failed; upload identity retained', file=sys.stderr)
    return False, result


if __name__ == '__main__':
    accepted, result = notarize(sys.argv[1])
    if result:
        print(json.dumps(result))
    raise SystemExit(0 if accepted else 1)
