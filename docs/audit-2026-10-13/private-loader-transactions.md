# Private replacement and recalled-state ownership

Source baseline: `da17ace28897a9dd1e70e81ef145833b8a43abd4`.

Replacement preparation keeps the current audio epoch, instrument model,
controls, script ingress, and streaming owner active. Failure or cancellation
publishes no empty audio replacement. A successful privately prepared candidate
acquires a new epoch; explicit empty selection still clears the slot.

Persistence capture uses the committed source, so a different pending source
cannot acquire the old instrument's live controls or script state. A host recall
of the same source needs a distinct state identity even while the previous
instrument remains audible. The post-load hook records that identity before
background preparation starts, and retains its capture fence after cancellation
or failure. Successful commit or explicit clearing removes the fence.

A per-slot recall revision distinguishes repeated recalls of the same source.
Preparation checks that revision along with source/settings and audio epoch,
including a final check under the view lock. An older job cannot commit or clear
a newer recalled-state fence merely because both recalls use the same path.

Authored production regressions cover failed and canceled replacement retaining
held audio and live edits; different-source capture ownership; same-source save
immediately after the post-load hook, during preparation, and after cancellation
or failure; deterministic recall during preparation rejecting the stale job;
and successful commit resuming live capture. The final review lane performed
source inspection, Rustfmt parsing without writes, and `git diff --check` only.
Coordinator-owned Rust execution and product rebuilding remain separate checks.
These transactions do not establish native Kontakt or UVI resource-lifetime,
asynchronous sample-loading, or host audio parity.
