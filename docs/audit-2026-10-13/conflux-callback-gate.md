# Conflux callback gate: outbox servicing

The original `ui::loop_audit::loop_audit_conflux` run failed at the assertion
`Conflux callback emits its knob label`. The exact frozen test executable was
`kontakto-150cba8117694641`; the same executable reproduced the failure. The
preserved checkpoint was `/dev/shm/kontra-conflux-callback-20261013/loop/conflux.json`.

The two originally sampled controls were:

| Source slot | Control ID | Callback outcome | Effects drained | Outbox stalls |
| --- | --- | --- | --- | --- |
| 2 | `f9cb86703f54062171bc1173069ae697` | Pending (`None`) | 256 | 15 cumulative |
| 3 | `9a3696ab722ff5f5ef0ef67bc8be9030` | Pending (`None`) | 256 | 9 cumulative |

Both callbacks were admitted, and neither checkpoint reported a runtime fault.
The test rendered eight 64-frame blocks without servicing effects, drained once,
and asserted immediately. The core outbox holds 256 effects. When it fills,
`Op::Emit` rewinds the instruction and suspends the callback until a later render
block can retry after the host drains. Draining alone does not run that tail.
Runtime construction also starts plan-owned callbacks; the old test never
settled that startup work before invoking the sampled controls.

The audit correction leaves render sampling, selected IDs, and midpoint writes
unchanged. After sampling, it settles finite startup by interleaving 64-frame
renders and the existing `ScriptUi::apply` effect servicing. It requires finite
bootstrap completion and a drained outbox. Startup has a 3,000-block bound
(192,000 frames, four seconds at the library's 48 kHz rate), and a pending finite
bootstrap or fault fails after its checkpoint is written. Startup label effects
are excluded from the callback label assertion.

The first corrected run compiled but failed this readiness check: after 3,000
blocks it had drained 1,483 effects, changed 927 properties, and reported no
faults, while three timer drivers remained waiting. Its checkpoint was
`/dev/shm/kontra-conflux-callback-final-20261013/loop/conflux.json`. Requiring
every plan-owned behavior to finish is incorrect for an animated library.

The compiler contract distinguishes those recurring programs without a
library-specific program-number list. `sampler-ksp/src/lib.rs` lowers each
registered `TIMER_MS` or `TIMER_BEAT` signal into a `Listener` entry followed
immediately by a non-entry driver. `lower.rs::listener_driver` intentionally
loops through wait, start-listener-body, and jump. `ScriptView::callback` and
`program_count` preserve the entries and each module's contribution; the audit
uses the same cumulative program bases as `bind_modules`. Listener entries and
their generated timer drivers remain in progress diagnostics but do not block
finite-bootstrap readiness. All other plan-owned programs, including scheduled
persistence callbacks, unclassified finite activation programs, and generated
engine initializers, still must finish. Faulted recurring programs still fail.
A quiet-frame requirement is unnecessary because legitimate timers may emit
continuously. An authored classifier regression includes both registered timers
and an unregistered listener immediately followed by a finite engine initializer.

Each selected callback then receives the same interleaved servicing until its
actual outcome is `Finished`, or the same bound is exhausted. Rejection,
missing callback, fault, cancellation, fuel exhaustion, stale outcome handle,
and a pending outcome remain failures. Checkpoints distinguish pending work
from completion and record render blocks, frames, per-service effect counts,
changed applications, ignored/no-op effects, faults, and per-phase stalls.

The original requirement that some callback emit exactly three effects with
exactly three changed applications is retained. The required `set_knob_label`
effect is also retained, and its actual target's `$CONTROL_PAR_LABEL` property
must read back the emitted text before the existing publication probe runs.
No library script, label payload, library asset, or protected source is added
to this document or the repository.

This source correction alone does not establish a passing gate or native
equivalence. The parent runner must compile and rerun the exact test. Its
existing scope remains owned IR layout and software painting; this test cannot
establish Conflux NativeUI-to-Vector layout support, native window presentation,
or GPU presentation. A production callback fix is not justified by the original
unfinished-callback checkpoint.

Coordinator validation: all 666 root tests passed serially, including the new timer-driver readiness regression. The exact installed-library gate passed after compilation, retained both originally sampled IDs, finished both callbacks without faults or new outbox stalls, verified three emitted/changed UI effects and the knob-label readback for one callback, and verified one changed string-property effect for the other callback, and completed publication checks. Startup serviced 1481 effects in 15 blocks. The subsequent plugin and standalone release build passed. These results remain scoped KONTRA evidence; native parity and Vector authored layout are not established. The external `conflux-passing-callback-receipt.json` records the final checkpoint and commands.
