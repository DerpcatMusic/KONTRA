# Repo navigation

If `graft/` exists, query before source search/read: `graft ask "question" --source`, `graft grep "literal"`, `graft skeleton file`; `graft callers symbol` before renames or multi-file edits. Otherwise use `rg`.

# Rust work

Always read the installed `rust-best-practices/SKILL.md` and its relevant reference chapters before writing or reviewing Rust. On this workstation it is under `/home/derpcat/.agents/skills/`. Delegated agents must do the same; use GPT-6.1-Sol with high reasoning when available.

Preserve sample-accurate MIDI identities, DSP parameter laws and format-specific semantics. Prepare and retire retained owners outside audio callbacks; callbacks must not allocate, free heap owners, lock or perform I/O. Validate behavioral changes with independent PCM or lifecycle checks, and measure performance in optimized builds before claiming an improvement.

# Available analysis tools

- Native: `r2`/`rabin2` + Ghidra `pdg`, `analyzeHeadless`, `retdec-decompiler`; MCP: `~/.local/bin/r2mcp -d pdg` (new sessions).
- Triage: `floss`, `capa`, `binwalk`, `ksc`/`ksdump`/`ksv`, `yara`, `upx`; Python: `r2pipe`, `capstone`, `unicorn`, `pefile`, `elftools`, `kaitaistruct`.
- Bytecode: `jadx`, `apktool`, `ilspycmd`, `wasm-decompile`/`wasm2wat`.
- Runtime: `gdb`, `lldb`, `frida`, `strace`, `ltrace`, `perf`, `bpftrace`, QEMU, `adb`, `rr` (AMD recording blocked; see guide).

Guide: `/home/derpcat/Documents/Codex/2026-10-04/sudo/outputs/reverse-toolkit.txt`. Shell tools work now; tracing permissions vary. Do not change system settings just to enable tracing.
