# MultiLFO endpoint candidate

This independent leaf reproduces one measured MultiLFO waveform branch and is not admitted by `ModGraph`. It does not establish Bartok playback parity.

The branch is SineDepth=1, TriangleDepth=SawDepth=SquareDepth=0, NormalizeOutput=Bipolar=true, SyncToHost=Invert=false and RiseTime=0. The caller supplies physical Freq/Depth, rate, Smooth and nonnegative NoiseDepth. Rates are limited to 32/44.1/48/96 kHz, Freq to .01..20, and the other exposed controls to 0..1. Processing accepts only 1..8192 real frames per call and rejects larger counts before touching state. Zero frequency and other waveforms, polarity, sync, rise and bypass behavior are outside the candidate.

Actual Bartok contains one MultiLFO under ControlSignalSources, using SineDepth=1, NoiseDepth≈.65, Smooth≈.1, Freq≈6.0631, Depth=1, Phase=0 and Retrigger=1. Its six direct incoming routes are Mode0 without mapper/invert/bypass: three Freq and three Depth, resolving to four StepEnvelope and two builtin sources. These counts/settings were inspected in memory; no bank payload or access key was saved in source.

## Native evidence

The unchanged UVI Workstation 4.0.9 reader is identified by SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`. The previously loaded original text snapshot is identified by SHA256 `684a5557efed9e426a62cbba75c15cc727d764680e1660164b7c5ae88698fda1`. Private Unicorn fixtures execute original MultiLFO constructor `0x141146350`, callback `0x1411497f0`, uniform/MT routine `0x14114a030`, normalization `0x14114a7a0`, waveform `0x14114a5d0`, sine and remainder instructions. Optional unrelated scalar/math adapters are forbidden on this path.

The retrigger fixture supplies authored CRT thread storage through the runtime TLS getter `0x141a012c4`; the original CRT random routine `0x1419f515c` remains unchanged. That routine updates its uint32 state by `state*214013+2531011`, returning `(state>>16)&32767`. This returned value becomes the native MT19937 seed. This proves an explicit seeded retrigger, not native note dispatch, thread-state initialization or ordering among other random users. The Rust API consequently accepts an explicit MT seed; it does not invent a hosted seed policy.

The Rust candidate matched all native float32 endpoint bits in 1,392 comparisons:

- 432 rate/frequency/smoothing/fragmented-block cases: rates 32/44.1/48/96 kHz; Freq .01/.5/actual≈6.0631/20; Smooth 0/actual≈.1/1; sequential blocks 1/17/31/32/33/65/129/256/8192 frames.
- 864 retrigger cases: four rates, three Smooth values, CRT states 1/42/5489/uint32-max, six phase values including half-cycle and 1, and 1/33/8192-frame blocks.
- 96 warm physical-control/storage-offset cases: Freq/Depth/Smooth/Noise transitions and offsets 0/17/32/65. These inspect the endpoint callback, not a normalized connection manager.

A separate 1,400-value noise comparison (2,800 uint32 draws) matches every native double bit across multiple MT state generations. The leaf has one native-vector/invalid-input preservation test. A counting allocator fixture measured zero heap allocations during processing and explicit retrigger. Checks use source-direct `rustc`; no application rebuild was required.

Receipts and fixtures remain private in `/home/derpcat/.cache/kontakto-uvi-multilfo-private/`: `rust-comparison-safe.json`, `random-comparison-safe.json`, `receipt-safe.json`, `measure_multilfo.py`, `rust_compare.py` and `random_compare.py`. They contain authored inputs and sanitized comparisons, not commercial preset data or executable code.

A further 64 global-context fixtures execute original note-on `0x141149320`, generic source processing `0x1410e5340` and note-off `0x141146540` with float32-exact Rust endpoints. Repeated processing at the same generation preserves native phase, and note-off restores its active-note count. The context, notes, generation and TLS storage are authored; production dispatch and RNG ordering remain outside this evidence.

The module export and owned preflight diagnostic do not admit the source. A source-direct gate harness checks exactly one MultiLFO blocker at its source node, the owned diagnostic containing `not executable`, and continued ModulationGraph rejection. The native-vector preservation test rejects 8193 frames and `usize::MAX` before reproducing the unchanged measured vector.

## Measured behavior and boundary

Phase advances in double precision by real interval length times `1/(double(rate)/double(Freq))`. Every endpoint precedes its interval. A partial interval updates persistent phase and smoothing only by real frames; its exposed final endpoint is extrapolated to 32 frames using float32 subtraction, multiply, division and addition.

Smoothing uses `min(1, interval/(double(Smooth)*double(rate)))` on the previous waveform value before evaluating the new waveform. The measured branch combines sine with one of two random values per cycle and normalizes by `1/(1+NoiseDepth)`. Native phase wrap refreshes both noise values. The constructor uses MT seed5489; explicit retrigger resets phase, noise and smoothing.

The candidate intentionally has no admission adapter. Native note/retrigger-mode dispatch, clone ownership, CRT thread/random scheduling, connected normalized Freq/Depth clocks, global source scheduling, bypass and full hosted lifecycle remain unproved. Existing unsupported-source gates must remain intact until those are measured. In particular, the four StepEnvelope inputs do not become a proven connected source merely because the StepEnvelope leaf is separately measured.

Official parameter reference: [UVI Lua MultiLFO](https://lua.uvi.net/_elements.html#multi-lfo). Documentation establishes exposed controls; it does not establish the endpoint/RNG behavior measured here.
