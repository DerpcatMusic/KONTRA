# Falcon UI loading attribution

The measured production initializer and post-initialization overlay/translation
are the dominant CPU work in this offline UI path. The current data does not
resolve a positive profiler overhead or identify function self CPU. It does
separate initialization from model export, resource preparation and painting.

## Same-executable profile-on/off comparison

[Numeric measurements](receipts/ao-ui-profile-pairs-20261015.json) retain three
runs per mode, ordered off/on/on/off/off/on. All six use the same opt-level-3,
no-LTO `ci` executable, SHA-256
`9fa1b7edc7276abd6654191dc6488f2fb69cbb7f93a080b0eae10a5cf8786344`, the same
Pizzicato 2 member, read-only seeded UFS cache, production initializer and shared
UI renderer. Source classification/coverage, Runtime/PCM/audio hosting and
musical/gesture callbacks are absent in both modes. Protected report pixels
and metadata stayed in process memory; only the numeric receipt is retained.

| Phase | Off wall, ms | On wall, ms | Off CPU, ms | On CPU, ms |
| --- | ---: | ---: | ---: | ---: |
| Container, selected program and scripts | 463.6 | 451.2 | 2,260 | 2,260 |
| XML parse and metadata | 58.5 | 54.1 | 50 | 60 |
| Production translate and initialize | 5,402.1 | 5,406.2 | 5,330 | 5,330 |
| UI model export | 253.0 | 229.7 | 250 | 220 |
| UI render/assets/PNG | 328.6 | 329.6 | 190 | 180 |

Values are medians; phase CPU is aggregate thread-group user plus system CPU,
so parallel work can exceed elapsed wall time. The observer resolves only the
owned descendant `corpus-health` process and checks its start-time identity.
It reads [Linux `/proc/pid/stat` CPU ticks](https://man7.org/linux/man-pages/man5/proc_pid_stat.5.html)
at existing stderr phase markers. This host has 100 ticks/second: each counter
has 10 ms granularity, and differences also carry pipe/scheduling observation
uncertainty. Sub-tick phases are not proved to cost zero CPU. Independent total
child user/system CPU uses [RUSAGE_CHILDREN](https://man7.org/linux/man-pages/man2/getrusage.2.html)
before/after the one child; the runner launches no other simultaneous children
and does not double-reap an already waited process.

Process-wall medians were 7,126 ms off and 7,065 ms on. Total child user CPU was
8,470.1/8,470.8 ms; system CPU was 370.7/367.5 ms. Wall ranges were
6,999–7,662 ms off and 6,865–7,119 ms on. The observed 61 ms median difference
is inside the variation and is not negative profiler overhead or a speed win.
All 213 monitoring observations were compiler-free; an external CPU workload was
present in 209. These are **contended** shared-machine observations, with no
clean causal overhead estimate or universal startup-time claim.

The inspector reached its render-begin boundary at median 6,549 ms off and
6,479 ms on after process launch. This means the initialized model and inspector
projection were exported. It does not establish plugin interactive publication,
which this standalone path does not perform. Report serialization and launcher
transfer also remain distinct from model export and CPU painting.

## What the profile attributes

The profile-on constructor clock has a median 3,198 ms, including a 636 ms
program object graph and 2,426 ms script body/require. Post-initialization
overlay/IR translation takes another 2,082 ms. These inclusive clocks overlap
with child phases; do not sum nested rows as exclusive work.

| Native API | Calls per run | Median inclusive wall, ms |
| --- | ---: | ---: |
| Parameter definitions | 60,034 | 629.3 |
| Read parameter | 575,045 | 358.2 |
| Write parameter | 108,953 | 64.0 |
| Single definition | 39,937 | 12.0 |
| Has parameter | 25,228 | 5.6 |

The sampler collected 1,186–1,214 VM safepoint stacks; stack collection itself
recorded 3.50–3.61 ms. That measured substep excludes the cost of all API clocks,
checks and report construction. The frozen sampler stores up to four inclusive
frames and does not retain which was the top frame. Its frame counts cannot be
converted into function self CPU or used to subtract caller costs.

## Resources and invariants

All six runs keep 10,145 widgets, 101 visible widgets, 53 validated hierarchical
menus and the same 28 explicit UI omissions. All 89 requested assets resolve
(84 image decodes and five fonts), with no pending asset or missing-file outcome.
The cache is a directory-metadata hit in every run, not cached vendor pixels.
After the first observation, aggregate resource reads take 3.9–6.5 ms and codec
decoding 40.9–48.4 ms. These are nested worker clocks; neither their sum nor
queue waits are interchangeable with complete renderer elapsed time.

Every run executes 14,053,215 VM checkpoints, leaves the existing work limit
unchanged, initializes without script faults and reaches rendering without
work exhaustion or wall expiry. The observed model counts, resource outcomes and work ledger agree; this
receipt does not establish full pixel or widget-state hash equivalence. The
separate native meter laws, unresolved
element tap bindings and Table fill-style gaps remain diagnosed.

## Bounded top-frame metadata

After the frozen on/off receipt, the opt-in profiler gained `self_samples` per
frame. This counts the topmost observed Lua/C stack frame alongside the existing
inclusive visits. The same 512-frame, 240-character source-name and 120-character
function-name bounds remain. Dropped top frames and unavailable top frames have
separate counters; retained self visits plus those two counters equal the number
of sampling observations. No function body, original script string or stack
memory is retained. Ordinary constructors keep the profiler disabled.

An authored nested-function/loop/menu fixture passes: ordinary and profiled
interfaces and computed values agree, coverage stays disabled, every observation
is accounted for, and the private body literal is absent from the report.
Top-frame visits are still safepoint-biased sample counts rather than exclusive
CPU time. The profiler's stack-gather clock includes the new counter updates;
the preceding frozen receipt predates this extension and measures its earlier
instrumentation only.

For subsequent deterministic A/B oracles, the inspector honors the existing
scanner-only `KONTRA_UVI_AUDIT_SEED` and records it in `mode.audit_seed` for
initialized UVI inputs. The CLI passes that explicit configuration through the
same production initialization helper. Legacy inspection and plugin entry points
keep `Config::default`; absent seeds preserve the prior behavior, and malformed
seeds are rejected before initialization. Seeded on/off CLI fixtures produce
identical complete interface records, while a no-seed configured inspection
matches the legacy default wrapper. These are authored fixtures; the preceding
six-run frozen receipt did not fix the Lua RNG seed and claims count invariants.
