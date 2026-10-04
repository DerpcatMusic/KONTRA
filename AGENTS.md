# Repo navigation

If `graft/` exists, query before source search/read: `graft ask "question" --source`, `graft grep "literal"`, `graft skeleton file`; `graft callers symbol` before renames or multi-file edits. Otherwise use `rg`.

# Available analysis tools

- Native: `r2`/`rabin2` + Ghidra `pdg`, `analyzeHeadless`, `retdec-decompiler`; MCP: `~/.local/bin/r2mcp -d pdg` (new sessions).
- Triage: `floss`, `capa`, `binwalk`, `ksc`/`ksdump`/`ksv`, `yara`, `upx`; Python: `r2pipe`, `capstone`, `unicorn`, `pefile`, `elftools`, `kaitaistruct`.
- Bytecode: `jadx`, `apktool`, `ilspycmd`, `wasm-decompile`/`wasm2wat`.
- Runtime: `gdb`, `lldb`, `frida`, `strace`, `ltrace`, `perf`, `bpftrace`, QEMU, `adb`, `rr` (AMD recording blocked; see guide).

Guide: `/home/derpcat/Documents/Codex/2026-10-04/sudo/outputs/reverse-toolkit.txt`. Shell tools work now; tracing permissions vary. Do not change system settings just to enable tracing.
