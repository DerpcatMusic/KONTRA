# Kontakt FX round 2: evidence boundary

Research only, 2026-10-11. No DSP, reader, registry or diagnostic changed.

The mandatory protocol in `/home/derpcat/.cache/kontakto-gpt-handoff.md`
requires vendor documentation, then extraction through REA/Ghidra or radare2
MCP, then an independent implementation and confirmation. The second gate
failed; cached decompilation is a lead, not a successful new MCP extraction.

REA `open_binary` accepted the original read-only `Kontakt 8.exe` at
`/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe`.
It reported SHA-256
`0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
The string query timed out after 300 seconds. The targeted retry at
`0x140b00b80` failed with `execution_failure: Analysis could not complete`:
provider Ghidra 12.1.4, run `1828c499-6311-46f5-8d1c-5525b17923b0`, profile
`94a4ace23a4ffd2b6013f0f27a62d085be4c87e3d2bfe27fb8dd4aca4b0c4cbd`.
No function or table was served. Local `rea doctor` exited 1, reporting
missing CLI `GHIDRA_INSTALL_DIR`, unsupported host classification and stale
registrations; the live server nevertheless had started Ghidra. These are
distinct observations, not a diagnosis of the live timeout.

radare2 MCP `open_file` rejected the original path and a scratch symlink;
`show_info` confirmed no file was open. Its installed path validator
`r2mcp/src/path.inc.c:101` rejects hidden path components, including `.wine`
and `.cache`. No alternate-path workaround or binary patch was used.

The historical source cited by the module notes is the read-only
`t3code-80fe786b/artifacts/engine-analysis-2026-10-07/` cache, specifically
`kontakt-engine/vtable-methods.csv` and
`kontakt-engine/complete-export/pseudocode.c`. Addresses are image virtual
addresses, not offsets. Constant checks used read-only PE section mapping
against the original executable. They do not establish MCP provenance,
native audio parity, complete parameter mapping, or smoothing parity.

The existing v1 implementation at commit `0cb7a8a0` and round 1 receipt
`docs/audit-2026-10-11/kontakt-fx-handoff.md` were consulted. No decompiled
code was copied into the repository. No library scripts or PCM were logged.

Confirmation: one baseline workspace `kontakto-heavy cargo test --no-run`
passed in 54.69 seconds (exit 0). No focused implementation test was added
because no behavior was changed; failing-first verification is not applicable.
No reference recording was made. Resume with a working MCP analysis of the
same SHA before implementing any law below.
