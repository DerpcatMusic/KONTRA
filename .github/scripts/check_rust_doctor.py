"""Fail closed on incomplete Rust Doctor reports; retain its diagnostic gate."""
import json
import os
from pathlib import Path
import sys


def check(report, minimum_score=None):
    complete = report.get("complete") is True and report.get("errors") == []
    gate = report.get("gate", {})
    passed = complete and gate.get("status") == "passed"
    score = report.get("audit", {}).get("score") or {}
    if minimum_score is not None:
        passed = passed and score.get("authoritative") is True and score.get("value", -1) >= minimum_score
    summary = report.get("summary", {})
    text = (
        f"Rust Doctor: complete={complete}, gate={gate.get('status', 'missing')}, "
        f"errors={summary.get('errors', '?')}, warnings={summary.get('warnings', '?')}\n"
    )
    if minimum_score is not None:
        text += f"New-core score={score.get('value', 'missing')}, authoritative={score.get('authoritative')}, required>={minimum_score}\n"
    return passed, text


if __name__ == "__main__":
    passed, text = check(json.loads(Path(sys.argv[1]).read_text()))
    print(text, end="")
    if summary_path := os.environ.get("GITHUB_STEP_SUMMARY"):
        with open(summary_path, "a") as output:
            output.write(text)
    sys.exit(0 if passed else 1)
