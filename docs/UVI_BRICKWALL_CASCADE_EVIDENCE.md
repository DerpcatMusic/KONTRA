# Brickwall Filter IIR cascade boundary

Measured 2026-10-04 against the unchanged official UVI Workstation 4.0.9 x64
executable (SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`)
and its earlier loaded-text snapshot (SHA256
`684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`).

[Official element controls](https://lua.uvi.net/_elements.html#brickwall-filter)
identify Bypass, Slope 0..2, SoftBypass and Freq 20..20000 Hz. The
[official Falcon description](https://www.uvi.net/falcon) identifies a
Butterworth low-pass/high-pass filter with slopes 24..96 dB/octave. Native
preparation selects orders 4/8/16 for Slope 0/1/2, corresponding to 2/4/8
second-order sections. The helper's high-pass selector describes that measured
coefficient branch; it does not establish a hosted property not listed in the
element table.

Factory `0x140e84570` reaches effect constructor `0x14129b380`, which constructs
signal `0x14129adf0`. The original signal constructor and rate preparation
`0x14129b100` execute unchanged in an authored caller context. Channel-layout
and sample-rate getters execute unchanged too. The constructor allocates 12
independent channel chains and shared double coefficient records. Native
coefficient preparation `0x14129cf90` uses Butterworth poles and a bilinear
frequency warp; sections receive those records through `0x1416480c0`.
An attempt to execute the complete effect factory stops on a null runtime
registry access at `0x140ab94a4`. The fixture does not initialize or replace
that registry. The complete DSP signal constructor is verified independently;
the whole hosted effect factory is not claimed as completed.

The independent Rust model computes the same pole/bilinear mathematics rather
than retaining vendor coefficient tables. It preserves the native float32
sample-period reciprocal/frequency product before double tan/cos arithmetic,
then rounds the five coefficients to float32 at the processing boundary. Each
section retains two float32 history values. Stage processing `0x141646fa0`
uses a transposed second-order recurrence in native operation order.

The actual production Rust, independent mathematical model and original native
cascade match bit for bit in 576 authored cold/warm cases: six rates
8/32/44.1/48/96/192 kHz, 1/2/6/12 channels, slopes 0/1/2, low-pass/high-pass
branches, counts 1/17/65/129, seeded nonlinear input and direct physical edits.
Tested cutoffs include 20/100/1000/5000 and the smaller of 20000 or 0.45*rate.
The helper limits cutoff to that safe measured range and limits rate to those
six values. It does not infer native behavior above this cutoff bound.

Another 432 authored lifecycle cases match actual Rust to original native
stages, direct setters, dirty coefficient preparation, native clear
`0x14129b130` and outer bypass `0x140ecc2d0`. They cover initially bypassed and
warm-bypassed sequences, pending edits, slope shrink/regrow, low/high-pass
changes, a zero-frame call, counts through 4096, retained state, reset and a
long zero tail. Cutoff/order edits wait until the next active process call;
existing section history persists, and newly restored sections start cleared.
Bypass leaves the audio and all measured coefficient/history bytes unchanged.
The generic wrapper may set its initialization bit, which is distinguished
from audio-state freeze. Bypass metadata is caller-owned; hosted controls are
not inferred from this wrapper fixture.

The full native callback `0x14129c510` first copies the input into context
scratch through IPP entry `0x1416868c0`. Twenty-four original callback probes
reach that authentic entry after constructor and preparation, with the correct
frame count, channel-zero source and context scratch destination. Input is
unchanged at this passive stop. The copy implementation's runtime dispatch
is unavailable in the unchanged snapshot; it is neither replaced nor treated
as verified PCM. The full callback's copy, SoftBypass ramp/mixing, hosted and
connected frequency clocks, voice clone/cache behavior and whole-program audio
remain unverified. This leaf exposes only the measured IIR cascade and is not
registered with Program playback.

The original full signal clone `0x14129af30` also executes, including copying
the native section and coefficient vectors. Forty-eight clone-continuation
cases cover six rates, 1/2/6/12 channels, warmed state and a pending physical
edit. The clone owns independent coefficient/history storage; clearing the
origin leaves its 129-frame continuation unchanged. Actual Rust cloned state
matches native output bit for bit. The fixture supplies allocation and release
services for its bounded caller-owned arena at CRT boundaries; no DSP
arithmetic or protected audio-copy helper is substituted. Actual voice-cache
assignment, allocator/runtime behavior and interleaving remain unverified.

Six owned Starter program slots were decoded only in memory: 10/11/12/31/46/50.
They contain 18 BrickwallFilter nodes, all initially active with SoftBypass=0.
Three nodes own four connections; fifteen own none. Actual slopes cover 0/1/2
and cutoffs range from 25.646612 to 7603.7886 Hz. Initial physical attributes
do not establish the connected nodes' dynamic timeline or later script edits.
No vendor XML, bank audio, source, activation patch or account state is included.

The actual initial settings of all 18 nodes are additionally compared at
44.1/48/96 kHz, 1/2 channels and counts 17/65/129: 324 bit-identical native/Rust
cases with original authored audio. These are physical-attribute comparisons;
connected sources are not synthesized or admitted. Three focused Rust tests
retain authored native low/high-pass impulses, split-buffer continuity, pending
edits during bypass, native-cleared history, channel independence and rejected
unmeasured controls. The Rust helper starts with explicit 1000 Hz low-pass
settings; native comparisons apply those physical settings before first audio,
and do not equate the helper defaults to hosted constructor/property defaults.
