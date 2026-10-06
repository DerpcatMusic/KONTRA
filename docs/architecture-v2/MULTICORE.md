# Multicore voice rendering

Goal: render voices on a fixed pool of real-time worker threads with output
bit-identical to the single-threaded path.

## Rules

- Pool starts at activate (host setting `threads`: `auto`, or 1..N; 1 = today's path, no threads).
  `auto` = min(available cores - 1, 4) workers plus the audio thread.
- No allocation, no locks on the audio path. Idle workers are PARKED (`thread::park`, a futex),
  never spinning: the audio thread publishes a run with one atomic store and unparks only the
  workers a block needs. It renders tasks itself while they work (tasks are claimed from one
  atomic counter), then waits briefly for stragglers. Idle cost is zero.
- A block uses the pool only with at least 32 voices per thread; below that waking workers
  costs more than it saves and the audio thread renders alone.
- Unsafe lives in a separate crate (`sampler-pool`); `sampler-core` keeps `forbid(unsafe)`.
  The crate exposes one safe call: `Pool::run(&mut self, shards: &mut [S], f: Fn(&mut S))`
  that runs each shard exactly once and returns when all are done.

## Partition

- Voice slots are split into contiguous shards by active-voice count (not slot count).
  Shard i owns the disjoint slices of every per-voice array (`cells`, `delay_samples`,
  modulation state, voice arena). Splitting is done with `split_at_mut`, so the borrow
  checker proves disjointness.
- Shared transient state (the filter bank scratch) is cloned per shard at activate.
- Chunks are 64 frames (the existing CELL grid), so control points do not move.

## Determinism

- Each voice renders into its own zeroed 64-frame scratch (stereo), never directly into
  the mix. After the parallel phase the audio thread folds scratches into the output and
  bus inputs serially in slot order: the float summation order is the single-threaded
  order, so results are identical.
- `end_voice`, stream-underrun and nonfinite counters are recorded per shard and applied
  after the parallel phase in slot order.
- Fused (AVX2+FMA) and baseline paths are each deterministic; they are compared with a
  tolerance, as with the resampler.

## Verification

- Test: same instrument and notes rendered with threads 1, 2, 4 give equal samples.
- Heap guard (`support::without_heap`) stays green with the pool active.
- Benchmark: 1024 voices x 4 SVF at +7 st, 1/2/4 threads. Report thread-CPU time and
  cycles per voice, not wall time (machine load is noisy).

## Status

Implemented in `crates/sampler-pool` (pool, `Slab`, `Disjoint`) and
`crates/sampler-core/src/parallel.rs`. Setting: `Runtime::set_threads(Threads::Auto | Fixed(n))`
(control side; the host reads `KONTRA_THREADS`, default 1). Unmodulated, unscripted voices
only: a block with a modulated or script-layered voice renders on one thread. Moving the
modulation advance and mix into the serial prepare and fold phases lifts that.
Per-voice DSP state is claimed through checked slabs on both paths; claim conflicts panic.

Measured (1024 voices x 4 SVF at +7 st, 64-frame blocks, loaded machine, thread CPU):
median wall 1264 / 702 / 423 / 317 us at 1 / 2 / 4 / 8 threads; CPU per voice-block
1430 / 1545 / 1621 / 2136 ns. Idle process CPU at 4 threads: 0.14% (no notes), 0.37% (one note).
