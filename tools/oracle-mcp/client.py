#!/usr/bin/env python3
"""Minimal stdio MCP client for oracle-mcp (JSON-RPC 2.0, newline-delimited).

    tools/oracle-mcp/client.py --script steps.jsonl
    tools/oracle-mcp/client.py '{"tool":"oracle_status","arguments":{}}' ...

steps.jsonl: one object per line:
    {"tool": "oracle_up", "arguments": {}, "expect": "ok", "comment": "launch"}
`expect` is "ok" (default) or "error"; a step whose result does not match fails the
run and the exit status is 1. Each step prints the tool's JSON text verbatim, so a
smoke run pastes as evidence.

One client process = one server process = one long-lived Kontakt, so a sequence of
steps shares the session (that is the whole point of the session model).
"""
from __future__ import annotations

import json
import subprocess
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
SERVER = HERE / "server.py"


class Client:
    def __init__(self, server=SERVER, timeout=900):
        self.timeout = timeout
        self.proc = subprocess.Popen([sys.executable, str(server)], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True, bufsize=1)
        self.next_id = 0

    def _send(self, obj):
        self.proc.stdin.write(json.dumps(obj) + "\n")
        self.proc.stdin.flush()

    def _read(self, want_id, deadline):
        while True:
            if time.monotonic() > deadline:
                raise TimeoutError(f"no response for id {want_id} within {self.timeout}s")
            line = self.proc.stdout.readline()
            if not line:
                raise RuntimeError("server closed stdout")
            line = line.strip()
            if not line:
                continue
            msg = json.loads(line)
            if msg.get("id") == want_id:
                return msg

    def call(self, method, params=None, timeout=None):
        self.next_id += 1
        self._send({"jsonrpc": "2.0", "id": self.next_id, "method": method,
                    **({"params": params} if params is not None else {})})
        return self._read(self.next_id, time.monotonic() + (timeout or self.timeout))

    def notify(self, method, params=None):
        self._send({"jsonrpc": "2.0", "method": method,
                    **({"params": params} if params is not None else {})})

    def handshake(self):
        init = self.call("initialize", {"protocolVersion": "2024-11-05", "capabilities": {},
                                        "clientInfo": {"name": "oracle-mcp-client",
                                                       "version": "1"}})
        self.notify("notifications/initialized")
        return init

    def tool(self, name, arguments=None, timeout=None):
        return self.call("tools/call", {"name": name, "arguments": arguments or {}},
                         timeout=timeout)

    def close(self):
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        try:
            self.proc.wait(timeout=60)
        except subprocess.TimeoutExpired:
            self.proc.kill()


def text_of(response):
    res = response.get("result") or {}
    parts = res.get("content") or []
    return "\n".join(p.get("text", "") for p in parts if p.get("type") == "text")


def main(argv):
    steps = []
    if argv[:1] == ["--script"]:
        if len(argv) < 2:
            print(__doc__, file=sys.stderr)
            return 2
        for line in Path(argv[1]).read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#"):
                steps.append(json.loads(line))
    elif argv and argv[0] in ("--list", "--tools"):
        client = Client()
        try:
            print(json.dumps(client.handshake(), indent=2))
            tools = client.call("tools/list")["result"]["tools"]
            print(json.dumps({"tools": [t["name"] for t in tools]}, indent=2))
            return 0
        finally:
            client.close()
    else:
        for arg in argv:
            obj = json.loads(arg)
            steps.append({"tool": obj.get("tool") or obj["name"],
                          "arguments": obj.get("arguments") or {},
                          "expect": obj.get("expect", "ok")})
    if not steps:
        print(__doc__, file=sys.stderr)
        return 2

    client = Client()
    failures = 0
    try:
        client.handshake()
        for i, step in enumerate(steps, 1):
            name = step["tool"]
            args = step.get("arguments") or {}
            expect = step.get("expect", "ok")
            comment = step.get("comment")
            print(f"\n=== [{i}/{len(steps)}] {name} "
                  f"{json.dumps(args)[:160]}{'  # ' + comment if comment else ''}")
            t0 = time.monotonic()
            try:
                resp = client.tool(name, args)
            except Exception as e:  # noqa: BLE001 - transport failure is a result too
                print(f"TRANSPORT ERROR: {e}")
                failures += 1
                continue
            dt = time.monotonic() - t0
            if "error" in resp:
                print(f"RPC ERROR ({dt:.1f}s): {json.dumps(resp['error'])}")
                failures += 1
                continue
            body = text_of(resp)
            is_error = bool((resp.get("result") or {}).get("isError"))
            print(body)
            print(f"-- isError={is_error} in {dt:.1f}s (expected {expect})")
            if (expect == "error") != is_error:
                failures += 1
                print("   ^^ UNEXPECTED OUTCOME")
            checks = step.get("checks") or {}
            if checks and not is_error:
                try:
                    parsed = json.loads(body)
                except json.JSONDecodeError:
                    print("   ^^ CHECKS SKIPPED: response is not JSON")
                    failures += 1
                    parsed = {}
                for field, want in checks.items():
                    got = parsed
                    for part in field.split("."):
                        got = got.get(part) if isinstance(got, dict) else None
                    if isinstance(want, dict) and "min" in want:
                        value = len(got) if isinstance(got, (list, dict)) else got
                        ok = isinstance(value, (int, float)) and value >= want["min"]
                    else:
                        ok = got == want
                    if not ok:
                        failures += 1
                        print(f"   ^^ CHECK FAILED {field}: got {got!r}, wanted {want!r}")
                    else:
                        print(f"   check ok {field} = {got!r}")
    finally:
        client.close()
    print(f"\nsteps={len(steps)} unexpected={failures}")
    return 0 if failures == 0 else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
