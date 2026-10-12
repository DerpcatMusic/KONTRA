# Recover art preparation after decoder permit poison

Two data-free decode permits retained poison after an unwind. Later legacy art workers exited before publishing a result, while the common Native/legacy codec returned `None` for every subsequent image. Both now reuse the existing `MutexExt::lock_unpoisoned` helper, which clears poison and reports recovery. Image admission limits, cancellation and pixel decoding are unchanged.

The parent `97438d89` includes first-panic logging: it records a bounded/redacted original message, location and thread before unwind, chains the host hook, and uses nonblocking diagnostics with stderr fallback. No second panic hook was added.

The combined test-only baseline `09c08d86` failed both poison regressions; five neighboring art tests passed. Candidate `51f52e44` passed all seven. The isolated codec test verifies two subsequent decodes preserve exact RGBA pixels; the worker test verifies completion and cleared poison. The consolidated commit preserves that candidate's product source.

Exact commands, first-panic hook/root no-run outcomes and scanner receipts live in `/mnt/Windows11/DEV_WORKSPACE/kontra-runs/w3-representative-art-344/`. Scanner selection copies shared manifest rows 1 and 789: Conflux NKI and Big Screen NKM only. **Macr is not covered: no fixture exists in the shared manifest.** Scanner Original-OK is render/resource admission, not widget or host-compositor parity.

These tests establish recovery from synthetic poison. They do not identify the tester's original Amati panic or early native-exit cause. First-panic evidence from a new tester bundle is still needed for attribution. No plugin install, release, full gate or timed performance measurement is part of this check.
