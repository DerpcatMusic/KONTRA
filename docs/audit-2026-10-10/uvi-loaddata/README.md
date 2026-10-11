# UVI loadData: explicit JSON async consumer

Status: SOURCE READY, Rust compilation/tests UNRUN, native runtime parity UNKNOWN.
No CPU/RAM comparison or overall Falcon async acceptance is claimed.

## Exclusive source and integration order

Checkout: `/home/derpcat/.t3/worktrees/KONTAKTO/uvi-loaddata-async-20261010`.
Branch: `pi/uvi-loaddata-async-20261010`.
Authorized frozen base: `9075da681fbca5019640619822a4540bfd2a57a1`,
tree `ee617e185aecada9d74e22b5e1c2ad499af23f28`.
Last validated combined revision remains `429e7dff3ecb0f53aef32c43201ab796ccc8c28c`.
Neither frozen source nor another lane checkout was changed.

Pick BOTH source commits, in order:

1. `4b0958c8afed3d0c4b7bde975476fed2e80b55d9`: binding, worker, task,
   JSON conversion, eight authored checks including the corrected audit leaf.
2. `1cdc8d4d5a82aa67d5a4a942b8501f8bf5bf1c41`: REQUIRED parked-owner wake,
   receipt backpressure, and additional actual ScriptThread regression.

Final source tree: `76e908e3bf57b91df5f70f57592a665907ebc85e`.
The first commit alone has a parked-owner completion gap and is NOT READY.
No Cargo.toml/Cargo.lock/dependency/public driver interface changed.
No core, KSP, zone, transport, plugin, or other UVI service slice is required.

## Authoritative reference evidence

Read-only vendor documentation is the authorized evidence equivalent here.
REA tool discovery found the live native inspection tools; opening/running an
installed reference host is unnecessary and prohibited for this slice.
These are documentation observations, NOT executed native measurements.

- https://lua.uvi.net/group___async.html, exact `loadData(path, callback)` section,
  coordinator archive
  `/home/derpcat/.t3/scratch/2026-10-10-stop-this-thread-2336e1f0-2f34-7f6415ff/handoff/official-docs/uvi-async.html`,
  verified SHA-256 `a646dafca693caf478d910a61818edce38a55d0e482fb3c5f8af2118474944c2`.
  It specifies AsyncTask return; optional callback receives decoded JSON value,
  not the task; missing/unreadable excludes the callback; success is true even
  if the file could not be read; invalid JSON errors in the callback's thread.
- https://lua.uvi.net/class_async_task.html, read-only retrieval 2026-10-10,
  archived here as `official-async-task.html`,
  SHA-256 `c2b6a09f0b5eda7f2ae6b3f12ff7c6f529980db2afe2f8c6b638ff2c2cbc9a4d`.
  Attributes: unique integer id, finished boolean (also after cancellation),
  progress float 0..1, success boolean, state pending/running/finished/cancelled.
  `cancel()` requests asynchronous cancellation, not immediate completion.
- https://lua.uvi.net/_async_intro.html, coordinator archive `uvi-async-guide.html`,
  SHA-256 `9ccbee50ce085b2e007bee3156973627b5e1510fecd184c93e33919ff511e2cb`.
  The guide distinguishes data callbacks from task callbacks and documents
  coroutine polling with `wait()`.
- The async page's `saveData` section specifies boolean/number/string/array/dict,
  nested tables, and stringified integer keys for mixed dictionary tables.
  No protected library/script/sample/key data was inspected or retained.

The exact archived pages describe the vendor's currently published contract;
matching an installed Falcon version has NOT been established. Native parity
is UNKNOWN. These receipts do not establish Kontakt behavior or performance.

## Implementing paths pinned to source SHA

All following locations refer to
`1cdc8d4d5a82aa67d5a4a942b8501f8bf5bf1c41`:

| Contract item | Production path/symbol | Authored runtime check (UNRUN) |
|---|---|---|
| Task return, fields, asynchronous cancel | `crates/sampler-uvi/src/script/async_data.rs:23-71` Status/Task/UserData | `tests/load_data.rs:60`, `:242` |
| Lazy bounded worker, parked-owner notification, receipt before next read | `async_data.rs:74-132` Request/Pending/Worker/DataLoads::worker | `tests/load_data.rs:292` actual ScriptThread chain32; drain-only, no host ticks/events |
| Per-host ownership, stale/reload isolation, no file-read join | `async_data.rs:135-142` DataLoads::stop; `src/script.rs:672-676` ScriptHost::drop | `tests/load_data.rs:242` |
| Successful unreadable task, no callback, one completion | `async_data.rs:144-183` DataLoads::poll | `tests/load_data.rs:104`; corrected `tests/uvi_audit.rs:187` |
| Bounded regular file read on IO worker | `async_data.rs:185-217` read_data | `tests/load_data.rs:104`, `:335` |
| Recursive JSON to Lua number/boolean/string/array/dictionary conversion | `async_data.rs:219-245` decoded | `tests/load_data.rs:60`, `:174` |
| Parse failure on completion coroutine, not calling pcall | `async_data.rs:247-292` install/completion wrapper | `tests/load_data.rs:132` malformed, binary, empty, trailing, overflow fixtures |
| Callback yield, clock context, no originating note | `async_data.rs:178-182` deferred tuple note=None; `src/script.rs:2299-2300` advance/poll/cycle; existing `resume` | `tests/load_data.rs:208` |
| Production registration, existing scheduler, sample/impulse untouched | `src/script.rs:1117-1118`, `:2273-2305` | all eight `tests/load_data.rs` tests |

Paths abbreviated as `async_data.rs` are under `crates/sampler-uvi/src/script/`.
Test paths in this table are under `crates/sampler-uvi/`.

Thread placement was inspected before implementation:
`src/scripted/thread.rs:97-122` processes incoming events on the Lua owner;
`:183-287` constructs ScriptHost on `uvi-script`, calls advance, publishes,
then parks if no timed coroutine exists. Existing callbacks already use
`resume`/`cycle` and preserve their note context. Only the loadData completion
uses note=None, the existing independent-coroutine context. No Lua value,
callback, ScriptHost, or VM crosses to `uvi-data-io`. Its std thread wake handle
is not an mlua coroutine. The wake target comes from `std::thread::current()`
inside the first loadData invocation, not the launcher thread. Production creates
and invokes ScriptHost on `uvi-script`; its Rc<Shared> prevents cross-OS-thread
Send moves. Reload creates a fresh worker/channel pair, not a reused global wake
handle. The audio-facing tick/drain/event queues are unchanged.

## Safety policies and limitations, not native parity assertions

- Only authored explicit absolute filesystem paths are accepted (4096 bytes,
  no NUL). Relative paths, bank members, URI/token paths, and implicit library
  resolution remain outside this slice. No official reader is used.
- Regular-file metadata is checked before and after open; read length is capped
  at 8 MiB plus one detection byte. Oversized payloads raise a completion-thread
  policy error with task.success=false. These limits are ours, not vendor limits.
  A concurrent path replacement between metadata and open is not made atomic;
  filesystem stalls can delay detached IO-worker exit. No IO join blocks reload.
- At most64 pending requests hold paths/status/coroutines, not64 file buffers.
  Completion capacity1 plus a capacity1 receipt channel gates the next read.
  After receipt, one next worker read can overlap the previous owner decode.
  Thus at most two raw byte payload lengths, each <=8 MiB+sentinel, can overlap;
  allocator capacity, Lua string copies, serde JSON node expansion, and retained
  callback data are additional memory. This is a logical bound, not measured RSS.
- serde_json's existing default nesting limit applies. Conversion consumes the
  existing deterministic callback work allowance per JSON node, and Lua retains
  its existing VM memory budget. JSON parsing allocates on the control owner,
  never on the audio thread. The parser is byte/depth bounded, not a new parser
  work scheduler. A per-host lazy IO thread is created only on the first request.
  Worker stack/count, physical peak memory, CPU and scheduling cost are UNMEASURED.
- Progress currently reports0 until completion,1 at finish/cancel. Vendor gives
  a range but no exact intermediate cadence. Task fields are read-only here;
  native mutation behavior, initial success timing, callback vs finish ordering,
  and invalid-JSON task.success details are NOT independently native-verified.
- JSON null maps to Lua nil (array holes retain their numeric positions), and
  all JSON numbers become Lua numbers via f64. Tests specify these owned-parser
  choices. The retrieved loadData section does not explicitly specify null
  holes, duplicate object keys, Unicode edge cases, or large-number rounding;
  installed-native equivalence for those details remains UNKNOWN.
- Unseeded production owners are explicitly unparked after publication. Seeded
  scan/audit owners keep their existing explicit barrier semantics; external
  file completion timing is not certified as deterministic audit output.
- Existing loadSample/loadImpulse remain unavailable stubs; loadMidi has no
  production binding. Their return/task/callback contracts are NOT proven by
  loadData tests. They require separate implementations and native evidence.
- This source slice does not certify sandbox authorization for arbitrary third-
  party authored scripts, installed-bank compatibility, complete Falcon/UVI
  services, or significantly lower CPU AND RAM than frozenv1 and Kontakt.

## Validation request: integration is the sole build owner

No cargo/rustc/clippy command was run here. Do not set CARGO_TARGET_DIR or
RUSTC_WRAPPER; use the current shared cargo/sccache configuration, one build
process globally, no background builds. Do not run generic gate.py/UVI probes,
installed tests, official readers, Wine, or a native host.

After BOTH source commits (and this evidence commit), suggested serialized gates:

```sh
cargo test -p sampler-uvi --no-default-features --test load_data
cargo test -p sampler-uvi --no-default-features --test uvi_audit missing_load_data_does_not_call_callback -- --exact
cargo test -p sampler-uvi --no-default-features --test load_data --features scan
cargo test -p sampler-uvi --no-default-features --test modules --test lua_compatibility --test ui
```

The no-default-features target excludes protected-bank reader support; fixtures
are only temporary owned JSON, nonexistent paths, and an owned directory.
`load_data` has8 active tests; the corrected audit leaf adds1. Repeat the
parked-owner and cancellation/drop regressions if the integration gate supports
repetition. Record the exact integrated SHA/tree, commands, exit codes and logs.
Do not promote source inspection or these owned tests to native parity.

Completed non-build checks: rustfmt on new module/test and corrected audit file,
`git diff --check`, exact vendor digest verification, static worker/channel/
callback ownership assertions. `source-checks.json` pins source/test hashes.
These checks do not execute the Rust binding, worker, scheduler or callback.

NEXT: obtain serialized integration receipts; fix only actual compile/runtime
failures reported there. Then separate remaining async service contracts and
memory/CPU acceptance measurements. Overall resource goal remains UNACHIEVED.
