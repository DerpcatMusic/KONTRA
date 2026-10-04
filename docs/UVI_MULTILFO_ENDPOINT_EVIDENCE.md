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

## Depth conversion and clock composition

Depth can be measured independently of the unavailable Freq range globals. Its original registration encodes bounds 0/1 directly and invokes descriptor routines `0x140fffcb0`, `0x1410024f0` and `0x141002450`, producing mapper `0x141ea3270`, multiplicative converter `0x141e37a50` and Smooth enabled. The fixture supplies the documented parameter name and those literal registration bounds; no Freq globals are replaced.

Original static converter `0x141429a80` matches 1,260 float32 cases covering five base depths, nine signed ratio values (including clipping beyond ±1), seven source values, both source polarities and inversion. For the measured scalar ABI, the first point buffer supplies ratio and the source node/state supplies its value. Bipolar values shift to unipolar before multiplicative Depth conversion; native Bipolar setter `0x141149650` explicitly updates source-node polarity at +0x238. This isolates static conversion, without constructing the complete connection graph.

Another 300 cases execute the original Depth getter/setter `0x141149540`/`0x141149530`, descriptor, parameter manager and generic source wrapper `0x1410e5340`. Physical Depth clocks and composed Rust source endpoints match all float32 bits at 32/44.1/48/96 kHz, base depths 0/.5/1, warm targets .125/.9/0, fragmented lengths 1/17/31/32/33/65/129/256 and additional 8192-frame cases. No DSP, math or SIMD routine is substituted; allocation/free adapters serve the private caller heap.

The existing leaf reproduces this composition when called once per real interval of at most 32 frames, with that interval's physical Depth point. The native wrapper selects each point before invoking the source callback; its initial output retains the previous interval's saved float. The normalized clock emits the prior future, advances persistent state by real frames, and pads the last exposed point separately. This proves caller-supplied scalar Depth edits and per-32-frame dispatch, not dynamic Ratio/source scheduling or actual graph wiring. The caller-authored parameter node/handler/vector, generation and fixed physical Freq are explicit fixture boundaries. Admission remains unchanged.

Private receipts are `depth-static-comparison-safe.json` and `depth-clock-comparison-safe.json`; reproducible fixtures are `depth_descriptor.py`, `depth_static_compare.py`, `depth_clock.py` and `depth_clock_compare.py` in the directory above. Freq registration instead reads +0x258b060/+0x258b064 in the official image's .data section; the current mapped/original on-file bytes produce invalid negative range values. No authenticated loaded .data capture is presently established. Those bytes do not establish a usable range, and no replacement range or loader retry is part of this evidence.

## Measured behavior and boundary

Phase advances in double precision by real interval length times `1/(double(rate)/double(Freq))`. Every endpoint precedes its interval. A partial interval updates persistent phase and smoothing only by real frames; its exposed final endpoint is extrapolated to 32 frames using float32 subtraction, multiply, division and addition.

Smoothing uses `min(1, interval/(double(Smooth)*double(rate)))` on the previous waveform value before evaluating the new waveform. The measured branch combines sine with one of two random values per cycle and normalizes by `1/(1+NoiseDepth)`. Native phase wrap refreshes both noise values. The constructor uses MT seed5489; explicit retrigger resets phase, noise and smoothing.

The candidate intentionally has no admission adapter. Native note/retrigger-mode dispatch, clone ownership, CRT thread/random scheduling, connected normalized Freq/Depth clocks, global source scheduling, bypass and full hosted lifecycle remain unproved. Existing unsupported-source gates must remain intact until those are measured. In particular, the four StepEnvelope inputs do not become a proven connected source merely because the StepEnvelope leaf is separately measured.

Official parameter reference: [UVI Lua MultiLFO](https://lua.uvi.net/_elements.html#multi-lfo). Documentation establishes exposed controls; it does not establish the endpoint/RNG behavior measured here.
