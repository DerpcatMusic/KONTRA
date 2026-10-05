"""Fail closed on incomplete Rust Doctor reports; retain its diagnostic gate."""
import json
import os
from pathlib import Path
import sys


def check(report):
    complete = report.get("complete") is True and report.get("errors") == []
    gate = report.get("gate", {})
    passed = complete and gate.get("status") == "passed"
    summary = report.get("summary", {})
    text = (
        f"Rust Doctor: complete={complete}, gate={gate.get('status', 'missing')}, "
        f"errors={summary.get('errors', '?')}, warnings={summary.get('warnings', '?')}\n"
    )
    return passed, text


if __name__ == "__main__":
    passed, text = check(json.loads(Path(sys.argv[1]).read_text()))
    print(text, end="")
    if summary_path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary_path, "a") as output:
            output.write(text)
    sys.exit(0 if passed else 1)
