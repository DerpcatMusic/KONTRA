# Opt-in UVI script-owner attribution

`KONTRA_UVI_OWNER_AUDIT=1` enables fixed numeric owner-thread clocks in a `scan`
build. Absent or `0` leaves them disabled; other values are rejected. Ordinary
builds contain neither this environment option nor these clocks. The disabled
scan path takes no phase-clock reads and performs no extra binding-graph scan.

`UiBridge::owner_audit()` reads aggregate atomics directly. It never queues a
request behind a busy Lua owner or acquires the interface mutex. It returns
`None` when disabled. No script bodies, arguments, file names, task paths or UI
strings enter this report. Storage is fixed at 32 stages and 16 counters; report
construction allocates only on the caller outside audio/owner work.

## Timing boundaries

The report names nine disjoint owner-turn groups: events, UI requests, advance,
UI publication, finding publication, scan snapshot, command collection, command
push and park. Loading and the initial UI snapshot are separate startup groups.

Nested groups must not be added to those outer totals:

| Nested group | Included work |
| --- | --- |
| Scheduled advance | Due coroutine lookup, resume and its deferred callback cycles |
| Data/sample advance polls | Each existing resource service, including owner bookkeeping |
| Deferred callback cycle | Each existing batch of spawned/completion callbacks; may nest in an event, UI request or advance |
| Interface export | Construction of the complete authored `Interface`, including hidden widgets |
| Control mirror | Control extraction and atomic value publication |
| Face mutex/equality/replacement | Lock wait, full-interface equality, replacement and retirement measured separately |
| Command backpressure | Existing full-queue retry, UI-request handling and 1 ms sleep; nested in command push |
| Backpressure UI requests | Request handling inside that retry; nested again |
| Seeded barrier events/advance | Barrier work nested in UI requests, separate from ordinary owner-turn stages |
| Export registry/topology/widgets/page/validation | Five disjoint substages nested in interface export (or the initial snapshot) |
| Sampled widget/value/inventory | Rotating declaration-index residue, stride 128; value and inventory are disjoint inside the sampled full-widget interval |

Each completed interval records calls, total/max monotonic wall nanoseconds, a
16-bin wall histogram and caller-thread CPU nanoseconds/samples. Histogram upper
bounds in microseconds are `4^i`, `i=0..14`; bin 15 overflows. Linux uses the safe
thread CPU clock through `rustix`. Unsupported CPU measurements remain `None`;
wall time is never substituted. CPU excludes decode workers and other threads.
Both clocks include their instrumentation overhead, and wall includes scheduling
delay. An interval still in flight is named but excluded from completed totals.

Snapshots read independent atomics concurrently; they are observations, not an
atomic transaction or a completion barrier. A before/after subtraction gives
cumulative sums, calls and histogram deltas; lifetime maxima cannot be
subtracted into interval maxima. Park duration is waiting, not compute. The
completed phase intervals may also straddle capture boundaries: their deltas
must not be forced to equal the caller's gesture elapsed time. Retain both
active-stage observations and the timing scope when interpreting them. The
existing owner progress marker now identifies ordinary advance and park phases
correctly; it remains a phase marker rather than a cost measurement.

Counters retain owner turns, popped events/requests, scheduled/deferred/all
coroutine resumes, exported widget/binding totals, changed faces, commands
collected/pushed, queue-full retries and backlog high-water. They establish work
frequency, not cost per widget or proof that a callback completed successfully.

## Validation and use

On 2026-10-15 three focused fixtures passed: optional off/on observation preserves
the complete final authored interface, commands and fault state; a delayed
callback is observed; a 1,100-command burst exercises the real 1,024-entry queue
and direct snapshots while the owner retries; nested clocks and bounded numeric
histograms remain separate. Ordinary all-target compilation and scan compilation
passed. These fixtures do not establish real-library timing or native pixels.
The existing seeded-audit integration suite passed 7/7 and UI integration passed
17/17 on the same source cohort.

The optional finer probe selects one declaration-index residue out of 128 per
export and rotates that residue across exports. Stage call counts retain the
actual sample size; counters retain all bound-value reads and sampled bound-value
reads. It performs no extra Lua property/getter reads. Sampled totals describe
the observed subset and must not be scaled into exact whole-loop costs. An
authored 257-widget fixture verifies the rotation, exact complete-interface
equality, custom getter mutation ordering, raw data edits and unchanged custom
`__index` side-effect counts with the probe enabled/disabled.

The dynamic-source witness can capture `owner_audit_before` after service binding
and `owner_audit_after` after task settlement, before independent PCM checking.
Its worker delivery measurement separately brackets worker completion to owner
receipt. Submission-to-owner-ack includes core application, transport and owner
scheduling; it is not pure core commit time.

For a useful 1,746-task run, retain before/after snapshots, task stage histograms,
full interface/geometry hashes, asset outcomes, PCM verification and external
workload observations. Compare export/publication frequency, command retry time,
sample-poll cadence and CPU versus wall before proposing batching or caching.
No scheduler, queue, publication policy or live UI-value cache is changed here.

Existing seeded `ScriptThread` audits use a synchronous barrier clock. That is
different from the genuine real-clock production owner. An unseeded real-bank
run can attribute its observed work, but cannot provide a deterministic full-UI
off/on oracle when authored initialization is random. Keep deterministic authored
off/on checks and seeded single-thread inspector oracles separate until an
explicit diagnostic real-clock/seed contract is introduced. Do not report an
overhead percentage or speed improvement from unmatched scheduling or UI seeds.

## Actual normal-owner observation

The [numeric 2026-10-15 receipt](receipts/ao-owner-phases-20261015.json) pins helper
SHA `ec30d253dfdc4e2cbc00b417455abdbca78d7ed1b27a9120e6b492bbb9e45d83`
and source revision `cfaea34fb0130c2f4d364145293fdb617c972752`. A fresh unseeded
Augmented Orchestra Pizzicato 2 owner accepted the actual visible LayerNext
control through its normal callback. The run used production translation,
attachment, streaming, source transactions and silent core commits; it loaded
no plugin binary, DAW or audio device.

The before/after owner window was 25,307.429 ms. These are measured completed
interval deltas, not a causal before/after speed comparison:

| Disjoint owner group | Calls | Wall ms | Owner CPU ms |
| --- | ---: | ---: | ---: |
| UI publication | 104 | 24,019.444 | 23,733.579 |
| Park | 2,026 | 1,229.484 | 7.844 |
| Advance | 2,027 | 41.065 | 39.372 |
| Events | 2,026 | 3.342 | 2.271 |
| UI requests | 2,026 | 3.326 | 2.260 |
| Scan snapshot | 2,027 | 2.484 | 1.372 |
| Command collection | 2,027 | 2.197 | 1.171 |
| Command push | 2,027 | 2.085 | 1.061 |
| Findings publication | 0 | 0 | unavailable |

Within those 104 UI publications:

| Nested group | Wall ms | Owner CPU ms |
| --- | ---: | ---: |
| Complete `Interface` export | 23,851.482 | 23,567.815 |
| Face replacement/retirement | 133.463 | 131.547 |
| Control-value mirror | 28.148 | 27.754 |
| Face equality | 1.927 | 1.833 |
| Face mutex wait | 0.126 | 0.051 |

Export covered 1,055,080 widgets: 104 complete graphs of 10,145 widgets, including
hidden widgets. There were 578,136 exported bindings, 103 changed faces, 1,746
deferred resumes, 201 scheduled resumes and 1,948 total coroutine resumes.
The gesture admitted one control event; 110 UI requests include the witness's
status observations. Exactly one command was collected/pushed in this window,
with zero queue-full retries or command-backpressure intervals. The reported
backlog high-water of 83 is lifetime state, not an interval-delta maximum.
The 1,746 source transactions use their dedicated producer/reply channel; they
are not counted as commands in that script-to-audio command ring.

Sample polling itself took 16.235 ms wall / 14.756 ms owner CPU. Worker delivery
wait totaled 22,926.051 ms across 1,746 responses (maximum 287.031 ms), while
source resolution/headers totaled 593.669 ms and preparation/decode 299.581 ms.
These are overlapping task-clock aggregates, not phases to add to the owner
window. They are consistent with completed worker responses waiting while the
owner exports UI, but no alternate scheduling implementation was tested here.

All 1,746 tasks succeeded after owner acknowledgments, with no failed, pending
or canceled task. Independent reads compared 4,656 windows / 1,191,936 frames
across 1,746 changed assets with zero registry/header/frame mismatch and matching
aggregate fingerprints. Eight meter requests resolved to exact owners; native
display laws remain unavailable. No voice sounded, and no script/core error or
stream underrun was observed. These facts do not establish native sound/pixels,
all-sample equality or note-on deadline margin.

The complete process took 33.145 s, including setup and independent PCM checking.
Monitoring observed 65 compiler samples and up to 98.2% compiler CPU. Cache state
was uncontrolled. There is no isolated latency claim or opt-in overhead estimate.
The timed owner CPU establishes full export as this run's dominant measured
consumer; it does not identify which export substage is expensive. The
[incremental export plan](UVI_UI_EXPORT_PLAN.md) therefore measures those parts
before introducing a cache or changing publication policy.

## Coarse export substages

The [follow-up numeric receipt](receipts/ao-export-substages-20261015.json) uses
clock commit `84beb030d` and immutable helper SHA
`0651288702df0cc39fdff8fa18d5f3047b99e4bfbed114601fb5e768d9f3859d`.
Five optional brackets split registry collection, topology, the complete widget
loop, page construction and validation. No field reads, getters or publication
policy were changed. The authored full-interface/command/fault off/on fixtures
passed 3/3 and ordinary all-target compilation passed before this run.

One normal unseeded LayerNext gesture produced 77 full exports over a
17,438.136 ms owner window:

| Disjoint substage inside export | Wall ms | Owner CPU ms |
| --- | ---: | ---: |
| Registry collection | 93.095 | 92.009 |
| Topology/paint traversal | 1,087.193 | 1,072.774 |
| Widget conversion/property inventory | 15,161.213 | 14,958.815 |
| Page extents/construction | 14.421 | 14.011 |
| Final validation | 31.099 | 30.533 |
| Complete export, including ungrouped work | 16,397.543 | 16,178.703 |

The widget loop consumed 92.46% of measured export CPU; topology consumed 6.63%.
Repeated page/validation ancestor walks have a deep-graph complexity risk, but
they are not the dominant consumer in this AO observation. The loop still
contains multiple operations: these coarse clocks do not distinguish raw field
readout, live parameter getters, conversion, assets/styles and unsupported-key
inventory. Further sampling or an exact authored micro-oracle must establish
which smaller change merits optimization.

The 77 exports visited 781,165 widgets and 428,043 bindings; 76 faces changed.
All 1,746 tasks succeeded with no pending/failed/canceled task, script/core error
or stream underrun. Independent PCM checking again compared 4,656 windows /
1,191,936 frames across 1,746 changed assets with zero mismatch and aggregate
fingerprint `2777464607603900197` on both sides. There was one command, zero
command-ring backpressure and 1,746 deferred completion resumes.

The process took 25.768 s. The monitor observed zero compiler samples and zero
named external `pure_va` workload; other desktop work and OS cache effects were
not excluded. The publication count differs from the earlier 104-export run,
and static assembly/cache conditions differ. Neither the lower total nor the
per-export difference establishes a clock-induced gain, an overhead estimate or
an optimization speedup. No export optimization or new full real-bank UI/pixel
parity claim accompanies this diagnostic. Further changes were deferred for the
requested production build and installed-binary replacement.

## Rotating widget sample

The [sampled numeric receipt](receipts/ao-export-sampled-20261015.json) uses
probe commit `fae3f896e` and immutable helper SHA
`847d6a1d21527d472b36890fbf2fb4a82c7e3aa8b0fd826309ebc5188b797075`.
A fresh normal LayerNext gesture completed 107 full exports / 1,085,515 widgets
over a 26,092.845 ms owner window. The rotating stride-128 subset contained
8,485 widgets. Its completed caller-CPU intervals were:

| Sampled work | CPU ms | Share of sampled widget CPU |
| --- | ---: | ---: |
| Current value read | 9.225 | 4.59% |
| Documented-property inventory | 68.518 | 34.08% |
| Remaining conversion, including probe overhead | 123.319 | 61.33% |
| Full sampled widgets | 201.063 | 100% |

These percentages describe the observed subset, not an extrapolated exact
whole-loop cost. The value/inventory intervals are disjoint and nested in the
sampled-widget interval. Both non-nil-element counters were zero in this AO run;
it provides no evidence of expensive element-bound value reads. Inventory is a
measurable next target; static classification and mlua raw iteration alternatives
remain experiments until exact malformed-key, mutation, full-model and pixel
oracles pass. No export optimization is enabled.

All 1,746 tasks succeeded with zero pending/failed/canceled tasks, script/core
error or stream underrun. Independent verification again compared 4,656 windows
/ 1,191,936 frames with matching aggregate fingerprint `2777464607603900197` and
zero mismatch. The process took 33.662 s with 66 compiler observations (maximum
98.8% compiler CPU). Publication frequency and scheduling differ from earlier
receipts. This is contended attribution, with no overhead or speedup claim.
Further optimization was deferred to prioritize reported Kontakt snapshot/UI
state and menu correctness.
