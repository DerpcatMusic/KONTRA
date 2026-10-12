# UVI setter consistency — bounded source handback

## Identity and status

- Private branch: `pi/uvi-setter-consistency-next`.
- Base: `366515c96b583747d1bd77c881b1f6909e86cf0d`, tree `a3a49bd4427b9b6941ebdaf14948bfc844197365`.
- Source/tests: **`0df29cdc63d66ea7ba6e98da64c81fae3e0201e0`**, tree **`4a5edb584954a5703ae0e0bcc8c2c2126169d808`**.
- Own changes: **SOURCE READY; all new/changed Rust regressions UNRUN and uncompiled**. Syntax/integrity checks are not runtime passes.
- Coordinator reports the inherited 3665 baseline validated: 46 direct runs, 320 passing invocations / 318 unique executed tests, zero failures, 43 ignored, 57/57 required names. FINAL-VERDICT SHA256 `624302e22df71c026cdb038de5bef13e3b8d13bf93101acb8bd9448e443cc3ce`; FINAL-CUSTODY `7494d0ca4443f8407039988176765ae791c0cb4bd49a5e5c1d6e36de28c88452`. These are **inherited coordinator results**, not executions of this patch. No own-code green result or comparative admission is inferred.
- Native execution/parity: **UNKNOWN**. Lower CPU AND RAM than both frozen v1 and Kontakt: **UNACHIEVED**. No performance measurement ran here.

## Small complete change within the existing acceptance seam

At the source SHA above:

1. `script_prelude.lua:365–404` silently ignores unknown names/IDs (including invalid numeric IDs), and preserves existing typed validation/clamping. It asks `native.setParam` before changing `__set` / `__touched`.
2. `script.rs:1395–1478` returns `true` for an admitted control command, `false` for an existing retained-XML local field, and `nil` for rejection. Catalog-known but unsupported controls no longer become successful-looking readback overlays. No parameter catalog or graph service was added.
3. `script.rs:656–666` reports actual admission to the **unchanged 65,536-command queue**. Native parameter state and Lua overlays do not change when the queue drops a setter command. This capacity policy is ours, not vendor evidence.
4. Imported `Custom` and connection `Destination` remain typed, script-visible **local model fields**. Their overlays do not prove DSP mutation. Existing unsupported findings remain; a type mismatch does not abort Lua. Unknown requests create neither definitions nor overlays nor commands.
5. Existing Gain/Pan, oscillator Pitch, Program Polyphony, OnePole.Freq and Gain.Volume routing/laws remain. Twelve non-setter command callsites keep their ignored-return semicolon/unit closure behavior. The exact census is **14 direct callsites**, including the two setters, not the earlier estimate of 17.

`script/async_data.rs` is byte-identical to base; `async_data::install` and everything outside the two Rust seams are byte-identical. No loader, reader, effect graph, oscillator implementation, dependency, frozen checkout, or integration freeze was changed.

## Vendor/reference evidence

`vendor-spec.json` preserves exact sections, URLs, archive metadata, hashes and selected verbatim parameter-table rows. Five supplied official archives were independently rehashed:

| Official page | SHA256 |
|---|---|
| [Element](https://lua.uvi.net/class_element.html) | `b4516f54b9fd2061c5ba134afc30930b4c7922a4ed78b3badd022a395e5b5200` |
| [Program](https://lua.uvi.net/class_program.html) | `6737fb73889b3a5eef1cf4eb61fa74b0d8925fc6aca9c67ad948f8aa79e986bf` |
| [Layer](https://lua.uvi.net/class_layer.html) | `0dcd5af14f8b525c4d11ef886c99583c08420a6f111acb364142c4f5fa01ef3f` |
| [Keygroup](https://lua.uvi.net/class_keygroup.html) | `7de0d5a623a3cb4d1251023ba333c2319db95d11ed9775655ddc550a984198c0` |
| [Oscillator](https://lua.uvi.net/class_oscillator.html) | `a170da27d5829f709798ff25b320ed4ccd47859ff8f5c0c677cf813e2cd19cc0` |

[Element::setParameter](https://lua.uvi.net/class_element.html#a876941566822f111c98750e3c954a94c) explicitly says an unknown name/ID or a mismatched Lua type is silently ignored, without an error. `getParameter` names/IDs return values according to the definition. The exact Program/Layer/Keygroup descriptions establish containment; Oscillator::Parameters names Pitch/Gain/BaseNote. This is a **primary-specification equivalent for requirements only**, not observation of a native host.

A read-only HTTPS fetch of [Elements & Parameters](https://lua.uvi.net/_elements.html) returned 200, SHA256 `ddca396e24ff7a05dbd3be9b2b073cb5a4a7adfb67fbd00d75e597f6c0a08d0f`, 753,534 bytes. The receipt retains 19 exact rows in Program/Layer/Keygroup/SamplePlayer/OnePole/Gain sections, including the supported fields and rejected test controls. The full archive is `/tmp/uvi-setter-next-elements.html`; this temporary archive path is not portable. Selected verbatim rows and the full-document hash are committed. Current unversioned docs do not establish an installed Falcon/Workstation version.

REA `current_document({})` returned **`target_unavailable`** (no app open); exact result retained in `rea-current-document.json`. No native process was opened and no decompiled code was copied.

## Prepared production regressions — NOT EXECUTED

`evidence.json` lists the exact 11 required names/targets and pins 24 implementing/consumer/test spans by commit, Git blob and span hash.

- Four new `host_parameters` regressions use **production ScriptHost**, not a replacement setter model: cold/cached unknown names and IDs; catalog-known unsupported writes preserving authored values; typed accepted names/IDs and exact queue commands across all admitted scopes; saturation with final-slot acceptance, overflow rejection, unchanged readback/overrides, and successful writes after draining (including unchanged native authored value).
- The existing Custom lookup and Destination retained-field tests remain. Destination now also rejects the wrong type and asserts no DSP commands/insert overrides.
- The production cutoff fixture now checks rejected names/IDs/types/unsupported Mode inside the controller callback, then accepted numeric-ID cutoff readback. Its existing peer-lane checks and non-vacuous PCM power thresholds are unchanged.
- The streamed Gain fixture has a **MIDI completion fence emitted only after rejection/readback assertions**. After that fence, it checks the production engine control remains 0.25 and an exact constant-input PCM sample remains unchanged. The later accepted numeric-ID Volume write still must reach the real control and change PCM by the existing ratio check. No tolerance was widened.

**FOLLOWING combined batch only**, after source composition with NCKP `8bbbbb978e5ad09b62373f88cee447b71f70ba3d` → `1b6280a2b64b5d8097f10cdc31df7feaa97116c3`. The sole integration owner should compile the composed source once, then execute `sampler-uvi` `host_parameters` (all seven required names there), the three exact `fixture` names, and `script::tests::set_parameter_on_program_and_layers_becomes_commands` from `evidence.json`. Default library-access plus the owner's existing scan union is sufficient. Use only the owned clear XML/generated WAV fixtures. Do not invoke a gate/reader/installed-library harness. Do not append to the finalized 3665 freeze. No validation build request was made for this lane alone.

## Remaining admission blocker and exact ordering for the next owner

All paths/lines here are pinned to source SHA `0df29cdc63d66ea7ba6e98da64c81fae3e0201e0`; the cited consumers are unchanged from base.

**Queue admission is not proof of actual lowered DSP-lane application.**

- `engine_parameters.rs:10–49` recognizes OnePole.Freq/Gain.Volume by type. `register:69–117` only adds controls for an admitted `InsertNode.count == 1`, excludes OnePole nonzero KeyTracking and invalid authored defaults. The setter cannot see that inventory.
- Ordinary translated path: `lib.rs:301` calls `register` during translation. The translated instrument already contains the admitted control keys when `attach_script_with_ui_state:1087–1108` calls `ScriptThread::spawn_with_ui_state`. `scripted/thread.rs:188–209` constructs ScriptHost, runs initialization and publishes `Loaded`/overrides. Only afterward does `lib.rs:1109` call `initialize` to edit existing control defaults.
- `engine_parameters::bindings:51–65` can convert those **already-registered IR control keys to EngineParameterAddress** before the spawn. This is the earliest small admission-inventory seam for the ordinary translated path; it is not yet a validated Prepared plan. Passing that existing inventory before initialization is a smaller candidate than a new runtime ACK framework. It has NOT been implemented here.
- Assembly is later: `lib.rs:1439–1445` (streamed) or `1708–1710` (resident) collects bindings, calls the Kontakt core loader, then installs them with `Prepared::with_engine_parameters`. `sampler-kontakt/src/load.rs:330–369` forwards through `finish_kept`; `prepare_inner:916–1010` invokes `sampler_core::lower::lower_with` at 945 or 1005. `sampler-core/src/engine_parameters.rs:379–397` validates that each binding has a real plan control index and valid law.
- **Legacy scripted bank paths have different ordering**: `lib.rs:1515–1518` and `1610–1613` initialize ScriptHost **before** `translate_full`, using its insert overrides to patch XML. They do not have an admitted translated inventory at initialization. They require a separate ordering decision; the ordinary-path candidate must not be claimed complete for these paths. No bank/runtime reader ran here.
- Delivery: `scripted/thread.rs:262–284` drains ScriptHost commands into the ring, retaining/retrying full-ring commands. Its `drain:439–448` feeds `Driver::wake/apply:324–358`; `apply_all:370–379` calls `Runtime::set_engine_parameter` and records an unmodeled finding on failure, with **no acknowledgement back to ScriptHost**. Core `engine_parameters.rs:632–672` finds the binding, edits the actual control, or rejects an unbound module. A healthy queue/write test is not universal downstream acceptance proof.

**NEXT:** central execution of the prepared 11 names, then a separate admitted-inventory follow-up with explicit ordinary/legacy initialization ordering. Full setter/native parity remains unknown until those gaps and native measurements are addressed.

## Checks actually executed here

`check-source.py` is a read-only reproducer; `source-checks.json` retains its output. It checks source-commit bytes, unchanged async_data/install/outside-seam source, the 14/12 caller census, `git diff --check`, Rust parser/format checks (rustfmt only), prelude syntax and seven embedded Lua-script syntax checks (`luac -p`, no execution). Private `graft grep`/`callers` explicitly failed with **no wiring graph**; exact-span source fallback was used. An earlier explicit main-graph query auto-refreshed generated graph metadata, was reported immediately, and was not repeated. No main product source was edited.

No Cargo/rustc/clippy/build, plugin/native host/audio job, reader, protected fixture, install/publication, nested agent, schedule, or dev server ran here. The remaining risk is the explicitly separate queue-versus-lowered-lane seam and unexecuted Rust regressions.
