# Frozen v1 UVI CPU adapter

Source base: `4bffbb18`. This standalone audit binary leaves product source and
its root Cargo.lock unchanged. Build through `kontakto-heavy cargo build
--release --manifest-path tools/cpu-audit-uvi-v1/Cargo.toml`, then copy to W10's
own frozen binary directory with a digest and provenance receipt. Never replace
the shared frozen v1 artifacts.

Run: `cpu-audit-uvi-v1 BANK.ufs::MEMBER.uvip 32|64|256 strings|fx|piano`.
`--check` verifies quantiles, MIDI translation and argument boundaries without
loading a library. Set `KONTRA_UVI_READER` to the approved read-only reader.

The common schedule is byte-identical to `tools/cpu-audit-common.rs` on
fix-uvi `0d8b84c7`: 48 kHz, the same chords, controllers, sustain and four-second
paced stream, including the same 128-frame segmentation. Private failures emit
only their SHA-256; samples and script messages remain in RAM. Initialization
uses native graph preflight, bank sample preparation, resource capabilities,
modules and `Player::new`, exactly the worker's uncached preparation path.

`all.p50_us` / `all.p99_us` measure native Player Lua/DSP processing wall time,
including event collection and peak checking, excluding worker transport/UI.
Current v2 `cpu_audit` measures V2Core's callback, which offloads UVI processing.
Those seams must not be treated as comparable total engine CPU. Admission
rejection is an explicit `loads=no` record (exit 2), not a zero CPU result.
Measured cells need quiet receipts; contended runs are correctness only.
