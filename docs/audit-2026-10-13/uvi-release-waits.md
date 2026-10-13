# UVI release waits and independent coroutine ownership

Reviewed 2026-10-13 against the official UVIScript reference. No Falcon or UVI
Workstation instance, installed library, compiler/build, or Rust test was executed
for this change. These are authored regressions of documented rules, with parent
verification pending; they are not native playback or timing receipts.

## Documented contract

The [UVI Time and Threading reference](https://lua.uvi.net/group___time.html)
states that `run` starts an independent coroutine immediately, while `spawn`
starts one at the end of the current instant. Both preserve the current virtual
time. `waitForRelease` suspends the coroutine created by NoteOn until its own
NoteOff; sustain does not prolong that wait. In callbacks not started by NoteOn,
including controller callbacks and functions launched with `run` or `spawn`,
`waitForRelease` returns immediately. These independent coroutines do not inherit
the initiating note's `isNoteHeld` context.

The [UVI UI guide](https://lua.uvi.net/_u_i_page.html) separately prohibits direct
waits inside synchronous widget `changed` callbacks. Starting a separate
coroutine with `spawn` from the callback remains allowed. Returning immediately
outside a note coroutine must not silently admit a direct widget wait.

## Production correction

`ScriptHost` previously passed its current note identity into `run`, although
`spawn` already had independent ownership. The prelude's `waitForRelease` always
yielded a release marker. As a result, `run` could wait on the calling note, while
spawn/controller coroutines could wait forever without a note to release them.
The promoted authored spawn test expected that permanent suspension, and the
unit release test incorrectly used `run` to assert inherited note ownership.

`run` now resumes with no note owner. Event dispatch assigns a release-wait owner
only to NoteOn callbacks, including their `onEvent` handler. The prelude rejects
synchronous use, and yields a release marker only while its owning note remains
held. Otherwise it returns immediately. A coroutine resuming after its NoteOff
therefore cannot install a release wait that has already missed its completion.

Regressions exercise the actual production `ScriptHost` scheduler: immediate
`run` order and argument identity, restored caller ownership, deferred spawn
continuation without note inheritance, unrelated NoteOff exclusion, direct-note
resume before `onRelease`, controller continuation, waiting after NoteOff, and
synchronous widget rejection with an allowed spawned alternative. The old two
test expectations were corrected against the official rule, not preserved as
implementation agreement.

## Evidence boundary

These ownership and callback corrections do not establish native sample-accurate
timing. Production `ScriptThread` still runs off the audio thread and transfers
commands through its delayed live queue. Wait APIs retain their `Approximate`
parity ledger status. Exact Falcon/Workstation callback timestamps, versioned
native event traces, and live block-size comparisons remain unverified.

UVI `loadSample` remains unsupported. Its documented callback-dependent task
return, sample replacement, asynchronous completion, cancellation, and old/new
voice resource lifetimes require a separate resource transaction; this patch
does not fabricate task success.
