# REA `open_binary` timeout on the soothe3 image

Measured on this machine (16 CPUs, load 40–49 during runs). Static analysis only.

## Root cause

- REA's Ghidra provider has a 330 s startup deadline (`DEFAULT_GHIDRA_STARTUP_TIMEOUT_MS`,
  rea-agents 6.7.0). It covers import, full default auto-analysis, and bridge readiness.
- Auto-analysis of the 138 MB image takes 833 s (headless baseline) and 885 s on repeat.
- `open_binary` only detects the format (~0.5 s). Ghidra starts on the first deep call
  (`binary_overview`), which fails at 331.6 s with `provider_timeout`.

## Measurements

| Run | Result | Wall |
|---|---|---|
| REA, default deadline, `binary_overview` | provider_timeout | 331.6 s |
| REA, deadline 1200000 ms, `binary_overview` | success | 729.4 s |
| Headless baseline, 2 GB heap | rc=0 | 833 s |
| Headless control repeat | rc=0 | 885 s |
| 4 GB heap (GC logged) | rc=0; 22 s GC pause, 0 full GCs | 844 s |
| 6 JIT/GC threads | rc=0 | 825 s |
| Disable 3 analyzers | rc=0 | 595 s |
| Disable 7 analyzers (incl. Decompiler Switch Analysis, Stack) | rc=0 | 215 s |

Largest analyzers in the baseline: Disassemble Entry Points 165 s, Decompiler Switch
Analysis 157 s, Stack 139 s, x86 Constant Reference Analyzer 126 s.

Heap size, GC, and thread counts are not the bottleneck.

## Applied workaround (user config, not in git)

- `/home/derpcat/.claude.json`, `mcpServers.rea.env`: added
  `"REA_GHIDRA_STARTUP_TIMEOUT_MS": "1200000"`.
- Backup: `/data/dev/workspace/RunScratch/smooth/rea-timeout/config-backup/claude.json.before-rea-timeout`.
- Revert: restore the backup, or remove only that key. The file was re-serialized with
  indent=2, so formatting differs; content is otherwise unchanged.

## Open

1. Restart the `rea` MCP server. Running servers keep the 330 s env. The live
   `open_binary` path is untested with the fix; a scratch client using the same env succeeded.
2. Claude Code's own per-call MCP timeout is separate and was not checked.
3. Headroom at 1200 s under load is about 470 s. 1800000 ms would be safer; untested.
4. Disabling analyzers gets under 330 s only by also dropping Switch and Stack recovery.
   Decompile quality with them off is unchecked, so this is not recommended.
5. Stack samples showed time in Ghidra's project DB buffer cache writing to disk. A larger
   cache or faster storage was not tested.

Scratch: `/data/dev/workspace/RunScratch/smooth/rea-timeout/`.
