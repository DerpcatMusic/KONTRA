# Finite startup group residency replay

The Evolved settled-owner diagnostic established a lost-admission problem: 1,872 captured requests produced only 1,024 accepted jobs, 848 queue refusals, and no pending work. Of 933 groups, only 32 remained loaded. Waiting ten seconds did not restore the refused requests. This document describes the general startup correction; a post-change installed-library witness remains a separate requirement.

## Execution and ownership

KSP initialization reserves each captured `purge_group` return ID in the existing plan-wide MIDI/NKA async domain. Compiler-owned records retain the physical source group, source slot, instrument/snapshot persistence pass, source offset and ID. A rolled-back evaluation pass rolls back its records and ID reservations together. These records are absent from `Model` and `ScriptView`, so UI snapshots do not repeatedly clone execution metadata.

Binding validates the aggregate roster before copying it, resolves physical groups through the prepared source mapping, and rejects missing/ambiguous source authority or colliding IDs. It does not renumber captured integer cells. Modules using async operations must share their initialization ID domain; independently compiled conflicting domains are rejected.

Both documented control-side consumers automatically prepare the roster: `Runtime::new` and `PlanControl::submit`, including their cancelable variants. They build a fresh residency owner and feed the existing physical ring with backpressure. Every captured request is submitted once, and every worker result is retained in a preallocated completion spool. The physical queue does not need an async callback continuation to free a slot. No request is silently consolidated or dropped.

The candidate's previous group owner stays unchanged during preparation. Only a completed candidate replaces that owner. Failure/cancellation joins and destroys candidate work off audio; it cannot alter an active owner's group revisions or eligibility. Shared Pcm retention protects old data and reader guards. A retired Prepared admitted again gets a fresh owner and completion spool instead of reusing stale completion results.

Worker reload failures remain false async results and unloaded groups. Preparation does not turn a failed source into success. Already retained, valid initial heads can be reused while the previous preparation owner still holds them. This is distinct from a later cold live reload, whose registered physical loader and memory quota remain authoritative.

On a runtime service boundary, at most 64 startup completions per generation enter the existing async callback path. Insufficient callback capacity retains the next result for a later boundary. Results always target their retained PlanId and ScriptInstanceId. Panic cancels undelivered results without emitting late success, and buffers remain owned until control-side retirement. Plan-program admission also retains its next-start cursor instead of silently losing a persistence/listener start on temporary capacity pressure.

`Runtime::startup_work_pending(plan)` is an O(1), non-advancing query for unadmitted startup programs or completion records. `ResidencyProgress.startup_completions_pending` reports the callback spool separately from physical queued/in-flight/ready jobs. Readiness must account for both; intentionally purged groups are not pending work.

## Bounds and cache

The shared physical queue remains 1,024 jobs. The finite roster is limited to 262,144 descriptors across all modules, with checked 32 MiB descriptor/spool/validation metadata admission. The compiler also charges the existing instruction budget for the replay cost. These are execution-metadata bounds, not a total process-RAM claim; sample payloads retain their separate physical budget authority.

The initializer cache has a required residency schema and explicit input/output async high-water provenance. Missing/corrupt identities reject before restoring values. Pure residency-ID reservations can be cached. Explicit-path NKA calls and other unrepresented async activity continue to require fresh initialization; allowing a nonzero async high-water must not cache external-file effects. The KSP profile is `ksp-8.12-v3`.

## Verified scope and open contracts

The full core/KSP package run passed 1,460 tests with 89 ignored before the final aggregate preflight adjustment. The final focused run passed nine core and seven KSP tests, including:

- 2,050 requests with a two-slot callback arena, every stored returned ID delivered once, and fresh nonzero PCM from the final selected group;
- exact successful/failed async status, physical slots 0/4 mapped to dense instances 0/1, and conflicting-domain rejection;
- instrument/snapshot pass provenance, failed-pass rollback, cache round trips with real callbacks and corrupt-cache rejection;
- constructor/update cancellation, blocked-worker cancellation, retained old eligibility/revisions/data, retired-plan re-admission, panic, and runtime heap guards;
- the aggregate descriptor limit and a later startup program retried after temporary callback pressure.

The earlier test-harness failures are retained privately: one assumed due resumes occurred before the next start-admission attempt, and another incorrectly used integer division by zero to force evaluator rollback. The full run also caught and corrected a real explicit-NKA cache regression; its original assertion is retained unchanged.

This fixes finite captured startup admission. Ordinary live `Op::Purge` still has its existing bounded queue/refusal behavior. Initialization-time `get_purge_state` has no physical source authority and is explicitly reported unsupported. The evaluator does not invent `$ALL_GROUPS` purge semantics or undocumented invalid-input behavior. Native cross-callback completion timing is not recovered; the existing final-engine-write replay already does not preserve cross-command interleaving with captured requests.

The [official NI group-command contract](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/group-commands) documents purge modes 0/1, UI/persistence callback availability and async IDs. It does not specify the native queue capacity, invalid-index sentinel, or this implementation's preparation/service scheduling.
