#!/usr/bin/env python3
"""Hermetic ticket retry checks; no Apple service, ticket or credentials."""
import contextlib
import importlib.util
import io
from pathlib import Path
import subprocess
import unittest
from unittest.mock import patch

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('stapler', HERE / 'stapler.py')
helper = importlib.util.module_from_spec(spec)
spec.loader.exec_module(helper)
PRIVATE = 'private-password private-artifact-path private-server-url'
LOST = 'Error Domain=NSURLErrorDomain Code=-1005 "The network connection was lost." ' + PRIVATE


def result(code, stdout='', stderr=''):
    return subprocess.CompletedProcess([], code, stdout, stderr)


class TicketRecovery(unittest.TestCase):
    def check(self, operation, responses, expected, attempts, delays):
        output = io.StringIO()
        with patch.object(helper.subprocess, 'run', side_effect=responses) as run, \
             patch.object(helper.time, 'sleep') as sleep, \
             contextlib.redirect_stderr(output), contextlib.redirect_stdout(output):
            self.assertEqual(helper.stapler(operation, PRIVATE), expected)
        self.assertEqual(run.call_count, attempts)
        for call in run.call_args_list:
            self.assertEqual(call.args, (['xcrun', 'stapler', operation, PRIVATE],))
            self.assertEqual(call.kwargs, dict(stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                              text=True, timeout=120))
        self.assertEqual([call.args[0] for call in sleep.call_args_list], delays)
        for secret in PRIVATE.split():
            self.assertNotIn(secret, output.getvalue())

    def test_both_operations_recover_after_connection_loss_on_either_stream(self):
        for operation in ('staple', 'validate'):
            with self.subTest(operation=operation):
                self.check(operation, [result(68, LOST), result(68, stderr=LOST), result(0, PRIVATE)],
                           0, 3, [2, 4])

    def test_exhaustion_is_four_failed_attempts_for_both_operations(self):
        for operation in ('staple', 'validate'):
            with self.subTest(operation=operation):
                self.check(operation, [result(68, LOST)] * 4, 68, 4, [2, 4, 8])

    def test_non_network_and_invalid_ticket_failures_stop_immediately(self):
        for operation in ('staple', 'validate'):
            for response in (result(68, 'CloudKit response unknown ' + PRIVATE),
                             result(65, LOST), result(68, 'invalid ticket ' + PRIVATE),
                             result(68, LOST + ' signature is invalid'),
                             result(68, 'Error Domain=NSURLErrorDomain Code=-10050 ' + PRIVATE)):
                with self.subTest(operation=operation, response=response):
                    self.check(operation, [response], response.returncode, 1, [])

    def test_fatal_after_transient_stops_without_exhausting_retries(self):
        self.check('validate', [result(68, LOST), result(65, 'missing ticket ' + PRIVATE)],
                   65, 2, [2])

    def test_unknown_process_failure_or_timeout_does_not_retry(self):
        for error in (OSError(PRIVATE), subprocess.TimeoutExpired(PRIVATE, 120, output=PRIVATE)):
            with self.subTest(error=error):
                self.check('staple', [error], 1, 1, [])

    def test_success_never_retries_even_with_unreviewed_native_output(self):
        self.check('validate', [result(0, LOST)], 0, 1, [])

    def test_no_other_operation_can_invoke_xcrun(self):
        with patch.object(helper.subprocess, 'run') as run:
            with self.assertRaises(ValueError):
                helper.stapler('submit', PRIVATE)
            run.assert_not_called()


if __name__ == '__main__':
    unittest.main()
