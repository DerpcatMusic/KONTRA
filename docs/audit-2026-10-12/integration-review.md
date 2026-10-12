# Independent integration review — 2026-10-12

Reviewed `f5442739`, `f7b89517`, `ae8ef69d` and `f6acb34f` against
`11ca186d`, plus the uncommitted `fix-widget-gate` donor. Other worktrees
and installed libraries were read only. The coordinator owns broad tests
and the installed-library scan; this review ran only the authored Falcon
fixture target. No licensed scripts, sample audio or host state were
published.

## Additional routing defect reproduced and repaired

`crates/sampler-uvi/src/inserts.rs:306` removes zero-gain and bypassed
branches from a static EffectRack. When every branch was removed, the
translator emitted no processor, preserving the input. The equivalent
scripted rack retained its branches and applied zero gain. An empty rack
also passed the input through instead of summing to zero.

This is a pre-existing routing error; the direct-child ownership correction
in `f5442739` did not introduce it. The original nested regression always
retained a nonzero outer sibling and therefore did not cover an empty sum.

Retained v1 `4bffbb18:src/uvi/playback.rs:2701` zeros an unbypassed rack's
output, then adds each nonbypassed chain. Its bus path applies chain Gain.
Bypassing the rack itself skips that processing and preserves the input.
The [Falcon effects manual](https://manual.uvi.net/falcon/en/effects/#effect-rack)
describes discrete parallel chains with their own level and power controls.
The fix follows that retained executable graph contract; it does not claim
a new Falcon recording or recover an unknown DSP coefficient law.

The translator now emits one zero-gain stage when a rack has no active
branches. `effect_racks_with_no_active_chains_are_silent` renders authored
constant PCM through single and multiple zero-gain chains, all disabled
chains, an empty rack and a nested silent rack, with and without a script.
Each case also checks that bypassing the parent rack restores unity.

Failing-first validation on the unfixed code returned **0.48828125 →
0.48828125** for the static single-zero-gain rack, and exited 101. After the
fix, the complete fixture target passes **24/24**, 0.55 seconds after
compilation, including nested ownership and live gain/bypass regressions.
Logs: `/tmp/kontra-parity-audit-20261012/uvi-silent-rack-red.log` and
`uvi-silent-rack-green.log`. Commands used the serialized heavy wrapper:

```sh
/home/derpcat/.cache/kontakto-heavy cargo test --locked --profile ci -p sampler-uvi --test fixture effect_racks_with_no_active_chains_are_silent -- --exact --nocapture
/home/derpcat/.cache/kontakto-heavy cargo test --locked --profile ci -p sampler-uvi --test fixture
```

## Widget-gate donor is superseded

The donor's 64-line patch adds binding and recall-mismatch counts, but still
computes persistence from every value changed during a gesture window. That
older gate could attribute an unrelated timer update to the edited widget.
Copying its helpers would also restore obsolete `typed-source-widget` keys.

Current `src/ui/widget_gate.rs:19` already records actual admitted UI
submissions. `changed` intersects backend changes with submitted owners;
array and XY values use individual typed-cell keys. The UI hooks in
`src/ui/part.rs` record admitted native and IR edits. Existing authored
regressions reject unrelated listener changes, unchanged edited owners and
untouched array cells, and still reject a real edited-owner recall mismatch.
The current recall loop checks those attributed keys. No source correction
or donor import is justified by this review. Keep the donor unchanged as
historical evidence and classify it as superseded.

## Other changes accepted with their stated limits

- `CachedInit.folders` is captured and restored in `init_cache.rs`. The
  Kontakt build hash includes `sampler-ksp/src`; old product entries miss
  their importer stamp. The required new field additionally rejects old
  JSON state rather than silently reconstructing empty folders. The authored
  cold/warm callback regression covers literal, computed and unknown folder
  selectors.
- Installed Kontakt discovery parses settings as JSON, handles the player's
  umbrella and single-library roots, and deduplicates instruments. Its
  first-per-library translation check explicitly permits access failures;
  this is metadata smoke coverage and cannot substitute for full preset
  load/render scanning or native host comparisons.
- Comparing workspace `Cargo.toml` package names with CI's `CRATES` list
  found exactly all **14** first-party workspace packages, with no missing
  or unexpected entries. This was a read-only Python/TOML check, not another
  Cargo run.
- Enabling the 31 existing authored UVI probes adds useful regular regression
  coverage. Their success establishes their asserted local contracts,
  including command emission and readback, without proving whole-library
  or calibrated Falcon sound parity. The updated documents retain that
  distinction and label historical corpus counts.

No additional correctness regression was verified in the reviewed changes.
Broad test results, installed-bank failures and the recovered UI work remain
in their separate coordinator and lane receipts.
