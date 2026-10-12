# Kontakt and Falcon parity audit

This audit integrates scoped recoveries and correctness fixes on
`main`, and adds an offline source/conversion/UI workbench.
Native Kontakt and UVI Falcon/Workstation are the reference. Native equivalence
remains unknown until a versioned native comparison establishes it.

## Start here

- [Library results](library-results.md): installed scope, separate source,
  initialization and audio evidence, failures, and validation receipts.
- [Offline inspector](../library-inspector.md): run the workbench, inspect original
  code and lowering, and navigate ownership, UI omissions and implementation gaps.
- [Main consolidation](main-consolidation.md): final historical source resolutions,
  preservation and cleanup receipts.
- [Worktree recovery](worktree-recovery.md): every branch/worktree disposition,
  preserved dirty work, actual recovered changes and reasons for deferred merges.
- [Native loader design](loader-design.md): current transaction boundaries,
  documented native constraints, and the proposed atomic resource/audio/UI bundle.

## Native contracts

[Kontakt](kontakt-native-contract.md) and [Falcon](falcon-native-contract.md)
link official loading and scripting documentation. They distinguish documented
rules from local implementation behavior and native captures still required.
The restore-order correction follows UVI's documentation. The asynchronous
`loadSample`, callback-time, asset-identity and UI renderer gaps remain explicit.

## Recovery and implementation evidence

The initial inventory found 695 local branches, 284 registered worktrees,
39 pre-existing dirty checkouts and 13 missing directories. Squashed releases
make graph divergence unreliable as a missing-work detector. The audit compares
content and behavior, preserves unresolved operations, and ports reviewed hunks.
The recovered [W13 UI work](ui-recovery.md) has failing-before/passing-after
regressions. [Integration review](integration-review.md) explains superseded
donors, Falcon rack ownership, and headless script-effect hosting.

[Kontakt omissions](kontakt-parity.md), [Falcon conversion/effects](falcon-parity.md),
[runtime validation](runtime-validation.md), and [historical library evidence](library-evidence.md)
retain the detailed findings. A completed importer, available binding or audible
local render supplies evidence for its own stage. UI pixels, gestures, musical
callbacks and native behavior each need their own gate.
