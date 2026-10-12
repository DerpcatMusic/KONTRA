# Basic LFO smoothing and native control-clock evidence

The original basic LFO smoothing recurrence is measured for an authored normal-coefficient matrix. Its tiny serialized nonzero Smooth default remains gated. A small triangle table-interpolation change removes a measured float32 operation-order difference; this does not admit deterministic Smooth or claim complete instrument audio parity.

## Original code and initialization provenance

Reference: unchanged locally owned UVI Workstation 4.0.9 executable, SHA256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`, image base `0x140000000`. Both PE FileVersion and ProductVersion identify 4.0.9. Original loaded text is retained separately from file-backed data. File-backed `.data` is not an authenticated initialized engine-state capture.

The factory registry's LFO factory is `0x140e869f0`; constructor `0x1410f72b0`, source-state constructor `0x1410f58f0`, and source-generator vtable slot67 `0x1410f8ca0`. A bounded genuine factory call stops at `0x140ab94a4`: its original thread-local static guard reads an absent `GS:[0x58]` TLS vector. No TLS guard, epoch or initialization flag was invented.

The basic waveform pointer vector at `0x14283abf8` starts null in retained data. Original RIP-relative writes identify table constructor `0x14163c3a0..0x14163cbbb`. Its observed sole direct rel32 caller is the original static-init thunk `0x140917320`: load the original object address, then jump to that constructor. Invoking this genuine thunk succeeds. The constructor executes original vector/scalar sine code, computes the deterministic waveform tables and publishes its own pointers. No generated table was substituted, no protected math dispatch was replaced, and no original code was patched. This is an executed native initializer in a declared fixture environment, not a capture of the complete initialized application.

After this initializer, the complete original generator returns in the measured normal domain using declared caller-owned source descriptors, numeric parameters/context, allocated buffers and reset flag. Original nested oscillator constructor `0x14163c300` and sample-rate setter `0x14163c370` execute. Full source factory, graph setters, voice scheduling, target interpolation and final PCM are outside this fixture.

## Exact smoothing law

The original source block `0x1410f99e3..0x1410f9a0c` forms these float32 operations:

```
product = Smooth * sample_rate
q = powf(float32(1/3), float32(1 / product))
```

For positive product, native float `powf` at `0x1419ef640` executes unchanged. The shared suffix `0x1410f9a29..0x1410f9b08` computes `q32` by five float32 squarings. For each real control point `x`, using the previous committed step duration:

```
retention = previous_step == 32 ? q32 : powf(q, previous_step)
previous = float32(x + float32(float32(previous - x) * retention))
x = previous
previous_step = current_step
```

The final endpoint uses `powf(q, last_step)` even for32, and is a lookahead only: it does not replace the committed previous value. The last partial step is committed as its actual frame count. Bipolar/depth transformation follows smoothing in the native source generator; those transforms are not implied by a raw-kernel test.

UVI's [official element API](https://lua.uvi.net/_elements.html) documents Smooth as a modulable0..1-second parameter; it does not specify this algorithm. No epsilon-to-zero rule is inferred from the tiny serialized value.

## Successful normal-domain measurements

Retained private receipts identify each separately scoped comparison:

- `native-regular-smoothing-safe.json`:720 complete original coefficient-plus-suffix cases; rates32/48/384k, Smooth.001/.1/1 seconds,10 sizes1..256, previous steps1/17/31/32, FPCW037f/027f and masked MXCSR1f80. All native coefficients and scalar-recurrence outputs match float32 scalar calculations bit-for-bit; no CRT error path is reached.
- `native-generator-paired-safe.json`:162 configurations and2592 complete original generator calls. Waveforms0/1/2, the same rates/Smooth values, authored frequencies5.5014190673828125/3/.6666666865348816, both control words, reset at initial Phase0, then blocks256/256/17/31/33/64/1/129. Paired native raw versus smoothed points match the recurrence bit-for-bit; raw/smoothed native phase and frame clocks remain equal. Committed partial-step state and uncommitted lookahead are checked.
- `native-generator-corners-safe.json`:36 further configurations/576 complete calls at48k, Wave1/2, Smooth.001, those frequencies, initial Phase.249/.499/.999, both control words, and the same blocks. These exercise initial corners and wrapping rather than only constant square intervals.
- `actual-rust-kernel-comparison-safe.json`: the actual unchanged Rust `smooth_controls` helper matches all7128 captured native paired control values bit-for-bit. Raw inputs are supplied from this authored native fixture; this does not assert the current Rust basic-LFO producer supports Smooth.

All fixture outputs originate in authored numeric inputs; no owned preset/script/asset payload is exported. Original native instructions and data files stay private.

## Tiny Smooth: observed result before an unresolved return

The observed serialized scalar is float32 `5.2776863e-09`. Twelve original caller-block cases (rates32/44.1/48/96/192/384k, both FPCW037f/027f, MXCSR1f80) reach underflow. The actual native startup-selected handler address `0x14068b890` executes `xor eax,eax; ret`, writes nothing, and cannot change an exception result.

The handler was selected by invoking the original encoded math-error setter `0x1419fe38c`, matching the original startup call `0x14199d60a`. A direct rel32 census finds that one setter caller; this is not an exhaustive proof of indirect callers or the application's actual initialized handler policy. The recovered identity is not an exported PE symbol.

At the handler, underflow type4 carries result bits0. After the original error path, XMM0 float result bits remain0 immediately before the native security epilogue. Execution stops there without a simulated return. Original `__security_init_cookie` at `0x14199ea68` was invoked first, but legitimately skips entropy when the retained cookie differs from the VC default. The retained noncanonical high bits remain invalid. Its exact original IAT references resolve to GetSystemTimeAsFileTime, GetCurrentThreadId, GetCurrentProcessId and QueryPerformanceCounter; no cookie or flag was overwritten. Original direct caller `0x14199d7f8` is in the CRT entry wrapper. See Microsoft's [initialization contract](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/security-init-cookie?view=msvc-170).

The earlier target `0x25549ee` is the retained Hint/Name RVA for the real original RaiseException IAT cell `0x141c64858`, called at `0x141a04564` in an authored unmasked FP environment. It is an unresolved standard CRT/Win32 exception path, not an unknown protected DSP dispatch. The current packed import directory does not enumerate that original cell. A no-op exception adapter was not used.

Receipts: `native-lfo-coefficient-before-return-safe.json`, `native-crt-boundary-safe.json`, `native-cookie-imports-safe.json`, `lfo-pow-import-provenance-safe.json`. Separate `native-zero-retention-suffix-safe.json` proves40 original suffix cases have bit-identical finite points and correct partial/committed state when q0 is an explicitly authored scalar. Those40 successful suffix cases cannot turn the12 incomplete coefficient calls into complete native returns or justify admitting the tiny preset default.

## Current Rust producer comparison and bounded change

The actual frozen `bb60218` Program parser and `ModulationGraph::deltas` were compiled directly with cached dependencies; no Cargo/full application build was run. The real path-lookup family and parser limits were copied unchanged into the focused wrapper. The complete actual modulation source was used, not a copied scalar-only producer.

`current-rust-native-clock-comparison-safe.json` compares27 Phase0 configurations. Current sine uses analytic double phase/sine and differs from native table/float clock already at frame32; maximum first512-frame error is2.822006932834409e-5. Square raw control points are bit-exact. Triangle's initial straight segments agree; corner tests expose float32 operation-order differences.

`current-rust-native-corners-comparison-safe.json` compares18 corner/wrap configurations. Triangle's maximum aligned error is1.1920928955078125e-7. Native table construction stores exact i/64 quarter segments; source generation interpolates adjacent points using two weighted float32 products and an addition. The candidate preserves that order in `triangle_lfo_value`, replacing direct piecewise evaluation of the entire uint32 phase.

`candidate-rust-native-corners-comparison-safe.json`: with the candidate, every first512-frame aligned control-point comparison is bit-exact. Mixed partial source-call comparisons retain at most5.960464477539063e-8 from the current graph's fixed-block interpolation; complete native source-call boundaries are not automatically the same as renderer/host boundaries. These remaining differences are reported, not hidden.

The authored regression uses actual `ModulationGraph::deltas` at rate48k, Freq5.5014190673828125, Phase.499 and frame64. Original generator result is `0xbccf97c0`; baseline computes `0xbccf9800`. The regression fails baseline and passes the candidate. Deterministic Smooth admission, stochastic seed policy, StepEnvelope behavior and sine producer clocks remain unchanged.

## Additional host-sync and transformed-control scope

`native-generator-sync-safe.json` adds 36 configurations / 576 complete calls at tempo 84 with SyncToHost1, Wave 1/2, the three scalar beat periods and corner/wrap phases. The candidate actual graph matches all first 512 aligned control points bit-for-bit; mixed-partial error remains at most 5.960464477539063e-8. The native float32 tempo/60/beat-period law is measured in this caller context.

A transformed full-generator attempt with Wave 2, Phase .499, Bipolar0 and Depth .37 reaches original helper `0x1416869b0`, then its unresolved retained indirect math dispatch at `0x1416869b9`. All legacy oracle vector/math adapters were removed for that attempt. No replacement was installed. Receipt `native-generator-transformed-boundary-safe.json` records the exact boundary. The completed generator scope is raw bipolar Depth1; no broader transform or normal-Smooth production admission follows.

## Actual Augmented scalar inventory and remaining production state

`augmented-lfo-scalar-inventory-safe.json` retains a completed read-only decode of all 620 owned Augmented declarations, using the current actual Program parser and ModulationGraph baseline. All 620 decode successfully. There are 620 positive Smooth nodes, each exactly float32 bits `0x31b55705` (`5.2776863e-09`), plus one zero-Smooth node. There are no normal Smooth nodes at or above .001 seconds. The baseline reports 364 first LFO Smooth failures and admits no complete graphs. Consequently a normal-Smooth-only gate would provide no additional admission in this bank while tiny Smooth stays gated; this is a static inference from the measured inventory, not a candidate after-census result.

Static disassembly identifies source reset callback `0x1410f8180`: Retrigger1 sets the selected state's pending-reset byte at +0x108 and increments its reference count. The original generator's reset branch clears its elapsed frame clock, committed smoothing previous value and rise progress, sets previous step to32, and restores integer phase from its Phase parameter. The already completed authored generator calls execute this reset branch. The source callback itself and its real voice/context scheduling have not been executed in the fixture.

Original state clone `0x1410f5af0` copies oscillator/frame state, coefficient, committed previous, previous step and the pending-reset byte. It does not independently reset the filter. It also copies the control-buffer pointer; independent buffer ownership is not established by those instructions. These are static observations, not successful full clone-call evidence.

A private normal-domain implementation draft retains the committed filter value/step independently from the final lookahead, and caches nine control points per full256-frame source block. It uses the already measured actual Rust smoothing helper and integer square/triangle clocks, scoped by the existing source/voice/instance key. Tiny values, sine/custom/random waveforms, depth/polarity transforms, delay/rise, live parameter changes, partial/skipped blocks and unmeasured source-clock contexts remain excluded. This draft has not been compiled, tested or admitted; new verification is pending under the CPU constraint. Existing completed native/kernel measurements do not substitute for that verification.

The genuine transformed-math initializer chain is `0x140917460` → `0x140001000` → CPU feature helpers → `0x141aca000`. Its bounded unchanged attempt stops at invalid instruction `0x141aca02f` in IPPCODE, outside the captured original loaded text whose exclusive end is `0x141ac9438`. Retained file-backed IPPCODE bytes do not establish initialized plaintext. A public Win32 synchronization/environment adapter cannot supply those protected arithmetic instructions. No math dispatch, CPU flag, cookie, table or return substitute was installed. Receipts: `native-math-init-boundary-safe.json`, `native-math-init-capture-boundary-safe.json`, `native-lfo-static-reset-clone-safe.json`.
