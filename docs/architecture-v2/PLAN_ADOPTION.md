# Prepared-plan generations and retirement

This is partial V2-10 evidence: bounded live replacement of resident assets and
native programs. It uses the new ownership kernel exclusively. Streaming jobs,
async request cancellation, imported resource IDs, parameter migration and host
lifecycle integration remain open.

## Ownership and admission

`Runtime::with_plan_updates` constructs the runtime and a separate `PlanControl`
on the control thread. It declares two independent bounds: live/retained generation
slots (including the active generation), and capacity in each transfer direction.
`Runtime::new` remains the single-generation construction path.

Each logical note counts one reference to a generational `PlanId`. New physical
inputs use the active generation; children inherit their parent's generation.
Families, voices, scheduled work and callbacks retain it transitively through note
ownership. Source selection, PCM reads, program instructions, wait policy and local
bounds all resolve through that generation. An old callback therefore cannot resume
against a replacement program table or generate from replacement sample indices.
Expression inheritance remains a separate decision.

A control-side submission moves a boxed immutable `Prepared` into the bounded
pending queue. Rejection returns the exact box and a reason: queue capacity,
sample-rate mismatch, callback-local capacity, disconnected endpoint or exhausted
request sequence. Accepted request IDs start at one; initial preparation is request
zero. Queue acceptance does not imply activation.

`poll_plan_update` first executes already-due musical work and returns unused
inactive generations, then adopts at most one pending plan at the current sample
boundary. Hosts explicitly choose that boundary. Adoption requires both a vacant,
non-quarantined generation slot and available return-queue capacity. Backpressure
preserves the queued plan and current active plan. No latest-wins discard or implicit
voice cancellation occurs. A returned request ID reports activation to the caller.

## Destruction domain

`Pcm::new` validates nonzero rate, nonempty frames and finite samples once, taking
the original boxed buffer without copying it. Its opaque immutable handle uses
`Arc` to share that buffer between independently prepared plans. Construction,
cloning and destruction belong on control; rendering borrows a slice and performs
no reference-count operation. There is no global cache, path deduplication or
streaming implied by sharing. Region and source constraints are still validated
for every prepared plan.

An inactive generation becomes eligible only after its last logical note retires.
Rejected terminal notifications, physical keys, tails, descendants, manual pins and
unaccepted behavior outcomes can all delay that retirement. `collect_retired_plans`
moves eligible boxes into the return queue; a full or abandoned queue leaves the
box in its existing slot. It never drops prepared assets on the audio thread.
`PlanControl::retired` consumes them on the control thread, where destruction or
reuse is allowed. The active plan remains resident even without notes.

Both endpoints and the runtime are destroyed off audio after processing stops.
If the control endpoint disappears during processing, polling reports disconnection
and retained storage survives until that explicit shutdown. Queue ownership is not
used to run a destructor in an error branch. Returned stale/cross-runtime `PlanId`s
cannot resolve a recycled slot.

Transfers use the already-resolved **rtrb 0.4.0** dependency through its safe SPSC
API; no custom atomics or unsafe code were added. Its documented push/pop operations
are wait-free and do not allocate after construction. One producer per direction
also makes a successful free-slot preflight stable against concurrent consumption.
See [crate contract](https://docs.rs/rtrb/0.4.0/rtrb/),
[producer capacity](https://docs.rs/rtrb/0.4.0/rtrb/struct.Producer.html#method.is_full),
and [consumer ownership](https://docs.rs/rtrb/0.4.0/rtrb/struct.Consumer.html#method.pop).
Arena scans remain bounded by declared capacities; no unbounded queue-drain loop
runs during adoption.

## Callback memory

Runtime preparation now reserves callback cells from the declared total budget:
`floor(behavior_cells / behaviors)` cells per continuation, zero when no
continuations are reserved. Initial and submitted program widths must fit that
fixed stride. Plan replacement never reallocates or reshapes callback storage.
Existing callbacks keep their values and original program-local bounds; new slots
are cleared on admission. Sample-rate changes still require a new runtime.

## Executable evidence

`cargo test --locked -p sampler-core --test plans` covers:

- Old/new PCM overlap, release tails and an old delayed callback whose replacement
  has no program table. Independent exact audio agrees across blocks 1, 3, 7 and 20.
- Separate note/behavior/terminal retention and no early old-plan return.
- Full generation, pending and retirement storage, lossless retry, unchanged active
  generation under backpressure, and exact ownership of a rejected box.
- Rate/local-width rejection before publication, callback-local state in a newly
  adopted program, stale/cross-runtime handles, endpoint disconnection and invalid
  configuration.
- Sixty-four transfers across an actual control thread. That thread destroys each
  returned plan; guarded audio operations adopt, render and retire without heap
  allocation or deallocation. Test synchronization is outside those operations.
- Shared PCM keeps its original buffer address across handle cloning, plan adoption
  and control-side destruction of the old generation. The surviving plan renders
  exact audio after caller handles and the retired plan have been dropped.

`cargo run --release --locked -p sampler-core --example prepare_workload` measures
one-time PCM validation separately from repeated one-region preparation. On the
local Ryzen 7800X3D pinned to CPU 2, median preparation was 0.22–0.28 microseconds
for buffers from one frame to 1,048,576 frames (8 MiB); validation of the largest
buffer took 541 microseconds. Preparation includes handle cloning and metadata
allocation, with plan destruction outside timing. Three paired audible render
runs against the preceding implementation measured a configuration-median speed
ratio of 0.972 (range 0.903–1.036). The extra ownership indirection is not claimed
to be free. Local CSV evidence is `artifacts/shared-pcm-*`; these are microbenchmarks,
not host latency guarantees.

The authoritative complete new-core score is 90, zero errors and 138 warnings,
with all rules retained. Local evidence uses `artifacts/architecture-v2/plans-*`.
This is ownership/correctness evidence, not a worst-case latency certification or
asset-streaming implementation.

## Native executable integration

`sampler-native replace FIRST.wav SECOND.wav OUTPUT.wav` now drives this ownership
path at a half-second boundary inside normal 256-frame processing. The old note
and tail continue while a same-key input uses the new generation. The executable
accepts both terminals and destroys the returned old plan outside rendering.
See the [audition timeline and independent audio checks](NATIVE_ENTRY.md#resident-plan-replacement-audition).
