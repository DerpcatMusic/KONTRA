# Conflux phase-form evidence (2026-10-13)

Reference contract: NI KSP Reference, [Engine Parameters / Instrument, Source
and Amplifier Module](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameters#instrument--source-and-amplifier-module),
WT_FORM/WT_FORM2 and WT_FORM_MODE/WT_FORM2_MODE. The [Conflux source and shaper
controls](https://docs.native-instruments.com/ni-tech-manuals/conflux-manual/en/edit-page.html)
make these editable synthesis parameters.

Read-only native target: `/mnt/Windows11/DEV_WORKSPACE/re-binaries/Kontakt8.exe`,
SHA256 `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
The old Ghidra cache path in the handoff is absent. REA opens and hashes the
binary, but deep analysis reports `provider_unavailable`: Ghidra is not
configured, Hopper is absent and IDA is not configured. Installed radare2 was
used directly on the same read-only PE; no host or Wine process was launched.

`0x140567aa0` takes phase and amount in XMM0/XMM1 and the phase-form ordinal in
R8D. Its jump table is at `0x140567fe0`. The checked branches are identity 0,
PWM 7 (`0x140567d2d`), asymmetric 8/9/10/14/15/16
(`0x140567d5b..0x140567e5f`), flip 11 (`0x140567e64`), seesaw 17
(`0x140567f83`), and second bend 31/32/33 (`0x140567bf7..0x140567c84`).
Constants read with radare2/PE mapping include .9375 at `0x144714920`, .9 at
`0x144714914`, .98 at `0x144714930`, 1.96 at `0x144714990`, - .9 at
`0x144714a50`, 4 at `0x1447149e4` and 1.8 at `0x144714980`.

The asymmetric law changes the two half-cycle slopes while preserving the
endpoints. PWM rescales a prefix and returns zero beyond its threshold. Flip
conditionally reverses phase. Seesaw traverses the forward and backward parts
with amount selecting their boundary. Second bend uses a rational curve around
the midpoint. Our Rust code is written from these understood laws, not copied
pseudocode. Existing ASYM2MP and cubic table readout port our v1
`0cb7a8a0:src/engine/wavetable.rs` and `src/engine/voice.rs::hermite`.

`tools/dsp/verify_wavetable_forms.py` follows the existing original-byte probe
workflow and executes 455 cases: 13 modes, five amounts and seven phases. It
substitutes no helpers. The committed numeric fixture includes SHA/entry and
all results; a repeated byte probe reproduces it exactly. The focused Rust test
failed before implementation at mode 7. The held-voice mode-write regression
failed before binding with `InvalidInput`. GREEN validation was still pending
at the WIP checkpoint.

The scalar routine alone does not prove amplitude-form processing, sync/log
forms, modulation-oscillator FM/PM/RM, anti-aliasing, random phase, inharmonic
clocks, native control smoothing or whole-instrument sound. Those states remain
outside this measured admission. A direct held mode edit must use the shared
physical engine address and retain oscillator phase. Unsupported form writes
remain explicit failures rather than falling back to identity.

Native KSP symbol-registration identities are distinct from public enum values:
`0x140463abb..0x140463e08` registers WT form names, using registry identities
1201 onward; `0x140775ef0` stores the name/identity pair. These registry numbers
must not be used as DSP ordinals. V1's valued WT constants are a separate
reference for the eventual frontend port; the registration inspection alone
is not an execution test of the KSP constant evaluator.
