# One representative patch per installed library

Scope: one selected instrument/program from each installed Kontakt and UVI library. A fresh production V2Loader preparation is followed by four seconds of bounded plan activation, CC1=100/CC11=127, one source-covered note at velocity 100, one second held and one second released. PCM is measured and discarded. This is an offline production-engine exercise; it does not establish native Kontakt/Falcon equivalence, DAW behavior, UI gestures, or whole-library compatibility.

Each patch runs in an isolated process. Structured load and runtime diagnostics use the same actionable report pipeline as the product. UVI program identities are enumerated from current bank metadata. Script array-file reads are serviced read-only and MIDI-save requests complete as failed; the probe never writes library state. This diagnostic-host policy must be distinguished from an engine omission. A failed probe or operational timeout is incomplete evidence, not a library incompatibility verdict.

Private source-bound artifacts and the explicit selection are retained under `/mnt/Windows11/DEV_WORKSPACE/PluginArtifacts/kontra-one-per-library-20261019/`. Full scripts, artwork and PCM are not exported. Build/source identity, compiler profile and executable hashes must be retained with comparisons.

## Before-change contracts

| Entry point / formats | Thread and ownership | Bounds and recovery | Distinguishing evidence |
|---|---|---|---|
| `offline_library_diagnostics` / diagnostic helper | Runs on a 32 MiB worker; owns prepared part, stream, source metadata and diagnostics; retires all state off audio | One patch/process, fixed activation/note/release frame budgets, external 180-second process timeout | Actual production preparation, finite audio measurements and located runtime faults |
| Effect drain / helper only | Between render calls; runtime owns job admission, helper owns temporary completion payloads | Existing bounded outbox and job storage; saves refuse without modifying source files | Read-only completion and explicit diagnostic-host scope |
| Fault capture / helper only | Reads retained continuations before completion retirement, then formats off audio | Existing fixed-size fault snapshots and bounded report history | Reports retain source slot/line, opcode and operand evidence |

The audit results, ranked gaps and implementation evidence are added after the sweep completes. Priority is playback/load impact followed by the number of distinct libraries affected, not repeated warning count.
