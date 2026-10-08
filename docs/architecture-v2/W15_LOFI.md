# W15: FX 0x20 LoFi v1 port

Ported the frozen v1 `0cb7a8a0:src/fx/blocks.rs` LoFi processor into a
separate scalar kernel. Bits quantization, fractional sample hold, seeded
noise and noise-color state use prepared coefficients and existing state
storage. Voice and bus insert chains and signal traces include the block.

Public payload decoding follows DSP_FORMAT_SPECIFICATION.md: Bits, Frequency,
NoiseLevel float32 values, one flag byte, then NoiseColor float32. Unsupported
true flag state retains UnknownLaw. Controls validate finite normalized values.
Native rate reduction/interpolation and noise calibration are still unverified.

Failing-first typed-slot test failed before the correct 17-byte decoder.
Two kernel tests pass (quantization/hold and fragmented noise/state reset).
`lofi_render_and_recycle_use_prepared_state_without_heap_work` passes with
zero audio-thread allocations/deallocations. Analog's saved slot is pristine;
exercising its Bits control at 0.1, Frequency at 1 changed the gate sample with
residual -11.843862949 dB relative to dry. PCM/output stayed in memory.
Receipts under `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/`:
`w15-lofi-typed-red.log`, `w15-lofi-kernel.log`, `w15-lofi-rt-heap.log`,
`w15-offline-ab.log`; touched-crate no-run passed.

Native calibration, dynamic processor targets, W12 recount and quiet CPU
acceptance versus frozen v1 remain required before full DSP READY.
NEXT: Constant loop boundaries and source/processor-target routing.
