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
(`0x140567d5b..0x140567e5f`), flip 11 (`0x140567e64`), mirror 12
(`0x140567ed5`), seesaw 17
(`0x140567f83`), and second bend 31/32/33 (`0x140567bf7..0x140567c84`).
Constants read with radare2/PE mapping include .9375 at `0x144714920`, .9 at
`0x144714914`, .98 at `0x144714930`, 1.96 at `0x144714990`, - .9 at
`0x144714a50`, 4 at `0x1447149e4` and 1.8 at `0x144714980`.

The asymmetric law changes the two half-cycle slopes while preserving the
endpoints. PWM rescales a prefix and returns zero beyond its threshold. Flip
conditionally reverses phase. Seesaw traverses the forward and backward parts
with amount selecting their boundary. Second bend uses a rational curve around
the midpoint. Mirror first folds phase at the midpoint, then applies asymmetric
slopes. Its native width coefficients are the exact f32 values with bits
`0x3f51758e` (`0x144714910`) and `0x3da7913f` (`0x1447148cc`); its upper-branch
seam subtraction is `0x3f000011` (`0x1447148fc`), deliberately not idealized .5.
Our Rust code is written from these understood laws, not copied
pseudocode. Existing ASYM2MP and cubic table readout port our v1
`0cb7a8a0:src/engine/wavetable.rs` and `src/engine/voice.rs::hermite`.

`tools/dsp/verify_wavetable_forms.py` follows the existing original-byte probe
workflow and executes 490 cases: 14 modes, five amounts and seven phases. It
substitutes no helpers. The committed numeric fixture includes SHA/entry and
all results; a repeated byte probe reproduces it exactly. The focused Rust test
failed before implementation at mode 7. The held-voice mode-write regression
failed before binding with `InvalidInput`. Both are now GREEN. Mirror's added
original-byte cases failed first at phase .125 / amount 0 (identity .125 versus
native .59158224), then passed within the same absolute tolerance, 2e-7.
The held switch preserves exact next-sample readout after 37 frames and performs
no heap allocation. The one-based saved-source regression failed with the former
0/16 admission, then passed for every admitted ordinal. Fractional/unproved host
mode inputs fail atomically; their regression also verified RED then GREEN.

The scalar routine alone does not prove amplitude-form processing, sync/log
forms, modulation-oscillator FM/PM/RM, anti-aliasing, random phase, inharmonic
clocks, native control smoothing or whole-instrument sound. Those states remain
outside this measured admission. A direct held mode edit must use the shared
physical engine address and retain oscillator phase. Unsupported form writes
remain explicit failures rather than falling back to identity.

Native KSP symbol-registration identities are distinct from public enum values:
`0x140463abb..0x140463e08` registers WT form names, using registry identities
1201 onward; `0x140775ef0` stores the name/identity pair. These registry numbers
must not be used as DSP ordinals. V1's 51 valued WT form, quality, modulation-type
and tuning-unit constants were ported to the frontend. The focused initializer
test failed with an opaque identity (33554433 instead of ASYM2MP's 16), then
passed. Other WT enum families still need numeric admission; the registration inspection alone
is not an execution test of the KSP constant evaluator.

The native jump table sends amplitude forms 18..23 and wrap 27 to identity in
this phase leaf. That is evidence of another consumer, not permission to admit
those forms as identity-only sound. Sync, first bend, log/exp and quantization
branches call math helpers/imports; they remain outside this helper-free probe.
The native phase leaf cannot settle the complete oscillator's form ordering,
audio modulation, smoothing or quality laws.
