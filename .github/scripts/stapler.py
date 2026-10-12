#!/usr/bin/env python3
"""Retry proven Apple ticket connection loss, without bypassing stapler success."""
import argparse
import re
import subprocess
import sys
import time


BACKOFF = (2, 4, 8)  # Four attempts, with fourteen seconds of total backoff.
CONNECTION_LOST = re.compile(r'\bError Domain=NSURLErrorDomain Code=-1005\b')
INVALID_TICKET = re.compile(
    r'\b(?:invalid|revoked|missing|expired) (?:ticket|signature)\b|'
    r'\b(?:ticket|signature) (?:is )?(?:invalid|revoked|missing|expired)\b', re.I)


def stapler(operation, artifact):
    if operation not in ('staple', 'validate'):
        raise ValueError('Unsupported ticket operation')
    for attempt in range(len(BACKOFF) + 1):
        try:
            child = subprocess.run(['xcrun', 'stapler', operation, artifact],
                                   stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                   text=True, timeout=120)
        except (OSError, subprocess.TimeoutExpired):
            # An unknown failure is not evidence of the reviewed network error.
            print(f'Apple ticket {operation} failed without a completed result', file=sys.stderr)
            return 1
        if child.returncode == 0:
            print(f'Apple ticket {operation} succeeded', file=sys.stderr)
            return 0
        output = child.stdout + '\n' + child.stderr
        transient = (child.returncode == 68 and CONNECTION_LOST.search(output)
                     and not INVALID_TICKET.search(output))
        if not transient or attempt == len(BACKOFF):
            # Paths, URLs and arbitrary native-tool text can contain private data.
            print(f'Apple ticket {operation} failed (exit {child.returncode}; attempts {attempt + 1})',
                  file=sys.stderr)
            return child.returncode if child.returncode > 0 else 1
        delay = BACKOFF[attempt]
        print(f'Apple ticket {operation} connection lost; retry {attempt + 2}/4 in {delay}s',
              file=sys.stderr)
        time.sleep(delay)
    raise AssertionError('Unreachable ticket retry state')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('operation', choices=('staple', 'validate'))
    parser.add_argument('artifact')
    args = parser.parse_args()
    raise SystemExit(stapler(args.operation, args.artifact))
