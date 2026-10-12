# Native Kontakt controller and group-condition evidence

Static analysis only. No plugin or executable was activated, and no system settings were changed. This report separates proved record mappings from remaining playback work. The binary is not included in the repository.

## Binary identity

- File: `/home/derpcat/.wine/drive_c/Program Files/Common Files/VST3/Portapotty/Kontakt 8/x64/Kontakt 8.exe`.
- Native Instruments version resource: Kontakt 8.13.1; PE32+ x86-64; 178,386,944 bytes.
- SHA-256: `0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8`.
- Image base `0x140000000`; PE timestamp 2026-09-09 13:02:39 UTC.
- Tools: radare2 6.2.2 with Ghidra `pdg`, `rabin2`, Python `pefile`. Bounded local `af`/`pdg` calls; no whole-binary `aaa`.

## Program private prefix: high confidence

Actual BProgram secondary vtable `0x144e98350` points to archive private reader `0x140d0a2c0` (file offset `0xd096c0`). The vtable belongs to the secondary object at BProgram+0x10; its reader-relative field offsets are consequently 16 bytes smaller than the complete BProgram offsets used by the named XML reader and system-script generator.

Every explicitly supported reader branch 0x80, 0x82, 0x90–0x92, and 0xa0–0xb5 reads this same prefix. Do not infer support for omitted versions.

| Private-payload offset | Wire field | Complete BProgram field | Independent name/branch evidence |
| --- | --- | --- | --- |
| 0..4 | little-endian u32 pedal mode | +0x21b98 | `cc64Mode`, XML reader `0x14092e8b0` |
| 4 | byte CC7/10 enabled | +0x1fb25 | `useStdCC_7_10`, string `0x144e76b70` |
| 5 | byte CC120/123 enabled | +0x1fb26 | `useStdCC_120_123`, string `0x144e76b80` |
| 6..10 | little-endian signed i32 CC7 maximum dB | +0x1fb28 | `cc7MaxVolume`, string `0x144f1a350` |

The leading u32 was incorrectly called a version in the older research parser. It is a pedal enum. Reader helper `0x140cf03c0` (file `0xcef7c0`) maps serialized 0→runtime0, 1→runtime1, 2→runtime3 and rejects other values. The XML reader names runtime0 SustainPlusController, runtime1 SustainWithoutController, runtime3 ControllerOnly. Writer `0x140d161c0` (file `0xd155c0`) confirms runtime3 serializes as 2.

System-script generator `0x140876bd0` (file `0x875fd0`) tests +0x1fb25 at `0x140878397` (file `0x877797`) before adding its CC7/10 actions, and +0x1fb26 at `0x1408783fe` (file `0x8777fe`) before CC120/123 actions. These are actual execution templates, not merely UI labels. The latter performs a 10 ms fade plus NoteOff for CC120 and NoteOff for CC123. Production scopes that 10 ms fade to physical CC120 with validated Native policy; Compatibility physical CC120, explicit logical stops, and MPE reassignment cleanup retain their existing 5 ms fades. It supplies no evidence that the enable option also disables MIDI channel-mode 124–127 implicit NoteOff.

The generator maps CC7 maximum -12/-6/0/+6/+12 dB to integer multipliers 3113/3937/4956/6243/7874 in its engine-volume parameter write; unrecognized values use the 0 dB multiplier. The controller byte is multiplied by this coefficient.

### Instrument Volume scalar: verified setter, scoped playback policy

The `_set_engine_par` builtin entry `0x140789c30` resolves through the mapped symbol table at `0x14a5ec800` and common setter call at `0x14078a0c2`. Integer normalization at `0x14078a9ce` multiplies the signed parameter value as a double by `1e-6` (literal `0x1447148f0`), then converts it once to float at `0x14078a9df`. Generic setter `0x1408bdd60`, Volume row 660, reaches `0x140958a90` → `0x140514930` → `0x1407ba020`. That converter multiplies the float `x*x*x`, clamps to `[0,1]`, then multiplies by float `4` (literal `0x1447149e4`). The instrument path stores the resulting linear scalar at BProgram+`0x1fb14`.

Together with the generator coefficients, this proves the instrument scalar for the standard MIDI1 CC7 action. Production selects this policy only for supported, validated native private controller metadata. Compatibility instruments retain the existing squared physical CC7 output gain. Native physical CC7 replaces the authored instrument Volume; a later authored Volume replaces that override. Production uses the Native program-output amplifier described below, with no additional physical CC7 multiplier in the authored output-controller namespace. Authored `set_controller(7, ...)` retains its explicit output-gain namespace and remains separate from authored instrument Volume.

The proved system-layer-before-user-script order places native physical standard actions at original ingress, before user controller callbacks. Production queues the scalar in the existing parameter timeline and carries an explicit handled marker through controller callbacks and waits. Accepted forwarding still updates canonical controller/modulation caches, while it cannot repeat the scalar at a later callback time. A user callback's authored Volume or Pan therefore replaces the earlier native action; ignoring controller delivery or waiting does not erase or defer that system action. This scope applies to verified CC7/CC10 behavior and does not infer pedal delivery semantics.

Higher-resolution paired/native controls retain canonical precision by extending the proved byte law continuously: `normalized * (127 * coefficient)` in double precision followed by the same native converter. This is KONTRA's precision policy, not proof of Kontakt's MIDI2 controller behavior. Native authored Volume/Pan writes retain their accepted, bounded integer value for readback and replay. Imported linear gain and physical Volume overrides use the native row-660 inverse: `0x1407ba0d0` multiplies gain by float `0.25` at `0x1407ba0d6`, clamps to `[0,1]`, loads float `1/3` at `0x1407ba0ee`, then jumps through `0x1443d10b4` to the PE-imported Windows CRT `powf`. Builtin `get_engine_par` at `0x140785620` resolves the same symbol map at `0x14078565b`, calls the generic getter at `0x1407857ca`, and formats this result as float `*1000000 +0.5`, truncating to i32 at `0x140785953`–`0x14078596b`. This establishes the arithmetic chain; bit parity between the host's math implementation and Windows CRT `powf` remains unproved.

### Instrument Pan scalar and CC10: verified setter/getter

System-script generator `0x1408783eb` uses the fixed CC10 coefficient `7874` and instrument PAN address `(-1,-1,-1)` (template `0x144f18260`). The normalized double-to-float path above is followed by float `2*x-1` at `0x140958b09`/`0x140958b27`. Generic setter row 661 at `0x1408c489d` reaches `0x140958a90` → `0x140514930` → converter table `0x149e628c0` → `0x1407b6d60`. The converter clamps the bipolar scalar to `[-1,1]`, computes float `0.5*z+0.5`, then float `2*y-1`; instrument storage at `0x1408c48ac` is BProgram+`0x1fb18`. That round trip's float rounding is part of the implemented law.

Getter `0x14089eef7` reads the same field through `0x140879000` → `0x140888210` type-2 row getter `0x1407b6d90`, which clamps to `[-1,1]`. Wrapper inverse `0x14087903a` adds float 1 and `0x140879048` multiplies float 0.5 before the common million-unit formatting above. Independent float replay gives CC10 byte 63 → parameter 496062 → pan `-0.0078759789467` → getter 496062; byte 64 → parameter 503936 → pan `0.0078719854355` → getter 503936; byte 127 → parameter 999998 → pan `0.9999959468842` → getter 999998. Byte 0 reaches pan -1 and getter 0.

Validated native instruments retain this scalar in the authored instrument Pan namespace and apply it through the Native program-output stage described below; compatibility instruments retain exact-centered `ControlValue::pan()` on the output. The scalar conversion traces alone do not establish insert/send ordering or smoothing; the separate bounded output trace below supports that production placement and recurrence. Continuous higher-resolution CC10 uses the same precision-extension policy stated for CC7.

### Group Volume/Pan units: verified shared converters

The common setter's nonnegative group scope calls `0x1407046a0` at `0x1408be006`; that resolver indexes the program's group array at +`0x11370` (`0x1407046c9`–`0x1407046d0`). Group dispatch at `0x1408be4c8` uses the native ID-minus-two table `0x1408cb888`. Row 660 case `0x1408c135c` calls the same `0x140958a90` converter at `0x1408c1373` and stores Volume at group+`0x48`; row 661 case `0x1408c1382` calls it at `0x1408c138c` and stores Pan at group+`0x4c`. Both retain the parameter ID and normalized value descriptor, with no external converter callback. Group Volume therefore uses the same float cubic-times-four law, and Group Pan the same bipolar float round trip, as their instrument counterparts.

The generic getter's group dispatch (`0x14089879e`–`0x1408987c6`, table `0x1408a42f0`) reads those exact fields: Volume case `0x14089c483` calls `0x140879000` at `0x14089c493`; Pan case `0x14089c49d` calls it at `0x14089c4ad`. Both use the shared inverse converters and common integer formatting described above. Production applies these scalar units to authored instrument and group parameters under validated Native policy, including setup/live display. Compatibility retains its existing conversion law. Physical CC7/CC10 ownership remains instrument-scoped. Saved group gain/pan remain physical scalars; this evidence establishes units, without additional claims about native group amplifier placement, insert/send ordering, or smoothing.

ControllerOnly runtime3 causes the generator to set NO_SYS_SCRIPT_PEDAL; its release/pedal templates are guarded by that condition. SustainWithoutController runtime1 shares the sustain templates. The [Kontakt manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view) describes unavailable normal controller modulation in this mode; it does not establish suppression of KSP controller callbacks. The modulation-availability boundary remains unresolved.

### Mode1 delivery trace: unresolved execution boundary

The generic engine-parameter getter `0x140898570` resolves parameter ID `0x2e8` to the pedal enum at case `0x14089f09f`; the setter `0x1408bdd60` writes that field at `0x1408c4a80`. Its changed-option epilogue calls `0x1409723a0` at `0x1408cb3c8`. This proves the setting is available through the parameter interface and triggers configuration rebuilding; it does not prove a separate controller delivery filter.

The compiled `cc_delivery_request()` builtin is ID `0x73`. Interpreter call `0x14079ab26` reaches `0x140955ac0`, which inserts the requesting part into a per-controller linked list at engine+`0x12c20 + CC*0x28` (head at +8). Controller dispatch calls `0x14086c670` at `0x1408ed3ce` with the original UMP event, then services requested parts at `0x1408ed42c`. The enqueue helper removes the matching request. This is a concrete one-shot delivery mechanism used by the system sustain templates, which read raw `%CC[64]` and `%CC_TOUCHED[64]`; it contains no direct pedal-mode test in these inspected spans.

The bounded controller path does establish raw capture before user callbacks: `0x14086bf00` stores the incoming controller at engine+`0x17314`, then `0x14086c2fc`–`0x14086c35f` walks active callback slots without a CC64 or pedal-mode test. Callback helper `0x140798190`, kind 3, dispatches the existing callback block without either test. Surviving delivery calls `0x140cee5b0` at `0x14086c3f1`; its stores `0x140cee625`–`0x140cee651` retain CC64 value and receipt timestamps like other valid controllers. Live modulation traversal `0x140865440` matches controller source kind 4 against the incoming CC without an explicit CC64/mode filter in that function.

The external-controller source preparation path `0x1409766d1`–`0x1409766fb` tests source kind 4 and the saved signed default at raw record+`0x558`. A nonnegative default initializes the source via `0x140cebb70`; a negative default marks source+`0xa8` inactive. This is authored pre-receipt availability, with no pedal-mode test in the inspected path. `BExtMod` vtable `0x144f17268` onset method +`0x78` (`0x140ceba40`) clears only local +`0xa1`/+`0x1bc`. Preparation method +8 (`0x140ceb830`) calls base `0x1408dcd90`, which writes the source rate at +`0x14` and clears count +`0x58`, then clears local +`0x1b0`/+`0x1bc`/+`0xa8`; these methods contain no CC64 or pedal-mode branch.

The assignment creator `BEngine::IO_AddExtModulator` is identified at `0x14088e100` through its lambda type descriptor `0x14a2b2f10`, type getter `0x140a70050`, vtable `0x144f38100`, and label callback `0x140996900`. The complete inspected creator writes the supplied external-source kind at `0x14088e5db` and controller field at `0x14088e60a`, then calls group preparation `0x140973970` at `0x14088e72a`. It has no CC64 or pedal-mode filter. This closes the inspected source creation, initialization, accepted-cache, and live binding paths, without excluding a restriction in the creator's callers.

These spans do not locate the documented modulation-availability gate. Copied settings, dynamic parameter consumers, or caller-side route-assignment restrictions remain possible; the trace is not a claim that mode1 has no native effect. Blanket raw, callback, or accepted-controller suppression would contradict this bounded delivery path. No additional CC121 raw reset or authored-init suppression is established. Production retains existing physical/raw/pedal behavior and identifies normal CC64 modulation availability as unverified in its import warning. Fresh task-owned captures and their bounded closure are retained under `artifacts/core-native/sustain-mode1-*`; no native runtime experiment was performed.

## Group-start record IDs: high confidence

BParStartCriteria vtable `0x144e96fc8` archive reader `0x140d04400` (file `0xd03800`) requires record version 0x70 and accepts only mode0..5/operator0..2. Writer `0x140d12ec0` (file `0xd122c0`) uses the same numeric values.

StartCritRouterForm init `0x14061c020` (file `0x61b420`) binds literal names to integer menu values. Dispatch `0x140600d80` (file `0x600180`) separately chooses the named condition forms.

| Mode | Meaning | Named form |
| --- | --- | --- |
| 0 | Always | Default |
| 1 | Start On Key | Key |
| 2 | Start On Controller | CC |
| 3 | Cycle Round Robin | RoundRobin |
| 4 | Cycle Random | Default |
| 5 | Slice Trigger | Slice |

Operators: 0 AND; 1 AND NOT; 2 OR. Menu labels are at `0x144727848`, `0x144727850`, and `0x14472784c` respectively.

The archive field order is mode, operator, five 16-bit key_min/key_max/controller/cc_min/cc_max fields, three 32-bit cycle_class/slice_zone_idx/slice_zone_slice_idx fields, then sequencer_only byte. In native group memory the first record base is +0x790 and its mode at +0x798; four slots are 0x30 bytes apart.

## Group playback: concrete static evidence and remaining boundaries

The same system-script generator builds the actual group predicates. Key conditions are latched selections, not a test that the sounding note itself lies in key_min..key_max: distinct saved key ranges are collected globally, initialized natively from the program's default keyswitch, and changed only by incoming notes in the combined keyswitch span. Within that span all range latches are cleared, then every matching inclusive range is selected; notes outside it retain selection. Overlapping ranges can select multiple conditions. KONTRA preserves the saved default-keyswitch value; the native initialization and update boundaries are traced below.

KONTRA now prepares the known Key/CC/RR/Random predicates off-thread and evaluates them at final surviving note admission, after user callbacks. The stored event mask is updated before queuing playback; direct notes and native release generation use the same selector without repeating clocks for multiple zone or attack/release queries. A dedicated native-release provenance field carries the private SYS3 meaning without exposing or overloading public custom parameters. The raw library receiver bank is separate from accepted physical MIDI state, includes virtual bend/pressure indices, and updates on original physical or scheduled authored delivery before ignore/wait. Forwarded old work cannot overwrite newer raw input; synthetic canonical CC121 notifications do not invent a raw-bank reset.

Preparation conservatively disallows affected groups, with explicit diagnostics, for Slice, unknown mode/operator, inconsistent or opaque records, leading Always before an active row, invalid controller indices, or negative RR classes. Disabling Group Start skips that conservative predicate policy along with its clocks; the separate Release Trigger guard still controls the release-mask prelude. The selector's deterministic RNG is local policy; shared native CRT random-sequence equivalence is unproved. Playback validation is pending; no native smoothing, Slice sequencing, or counted-loop physics is claimed by this integration.

CC conditions use inclusive ranges on the raw script CC array. Round-robin count is the maximum saved cycle_class+1 across group rows; the global index starts -1 and its advance/modulo template checks `get_event_par($EVENT_ID,$NI_SYS_EVENT_PAR_3) = 0` (template string `0x144f17ea4`). The release template writes `$NI_SYS_EVENT_PAR_3` as 1 (`0x144f17983`). This private symbol compiles to runtime event parameter 20, separate from public `$EVENT_PAR_3` and `$EVENT_PAR_SOURCE`; the ordinary ingress and fresh `play_note` paths initialize it to zero through the shared event reset below. Random count is the number of random rows; generated indices are assigned in group/row iteration order, and selection is a uniform random integer over those rows. This evidence does not authorize assuming per-group, per-key, or per-channel cycle counters.

The native symbol constructor `0x140775ef0` stores the following name-associated IDs at symbol-record `+0x20`. These establish distinct symbol bindings; the separate compile-time constant table below translates them into numeric runtime event parameters. KONTRA's supported public constants now use those traced runtime literals.

| Symbol | Native symbol ID | Value/name binding addresses |
| --- | --- | --- |
| `$EVENT_PAR_3` | 994 (`0x3e2`) | `0x140459ab3` / `0x140459ac0` |
| `$EVENT_PAR_SOURCE` | 1001 (`0x3e9`) | `0x140459b9a` / `0x140459ba7` |
| `$NI_SYS_EVENT_PAR_3` | 1024 (`0x400`) | `0x140459e05` / `0x140459e10` |

The engine's runtime event-parameter switch independently separates three internal fields from public custom storage: getter `0x1409918b0` and setter `0x1409a8e10` map runtime parameters 18/19/20 to event `+0x27c/+0x280/+0x284`; public custom parameter 3 maps to `+0x294`. The RR template's own annotation identifies its guard as excluding release-trigger-generated events, and the release template marks the new `play_note` event after `_will_never_terminate`; public custom values and `$EVENT_PAR_SOURCE` must not substitute for this private provenance marker. Earlier candidate reset `0x14094d0c0` is rejected: RTTI identifies its object as `BFilterLadder_legacy`, so its coincident field offsets prove no event default.

The name-to-literal chain is closed independently of these field offsets. Symbol lookup `0x140790470` returns the registered ID; expression constructor `0x140791f50` calls it at `0x140791fd2`, sets literal kind 1 at `0x140791fe0`, reads constant table `0x14a5eb030[symbolID]` at `0x140791ff0`, and stores the result at AST `+0x24` (`0x140791ff7`). Literal evaluation returns this value unchanged at `0x14078bcf2`. Static initializer `0x1404591c0` passes that table to constructor `0x14077efc0`. Its array begins at `rsp+0x20`, with `rbp=rsp+0x100`; symbol 1024 is therefore `rbp+0xf20`, explicitly assigned literal 20 at `0x1407802e1`. The finished array is copied to the table at `0x140780c30`.

The same constructor proves the following native event-parameter literals. Symbols 991..998 receive literals 0..7 from the two-iteration SIMD loop `0x140780250`..`0x14078026c`, based at `rbp+0xe9c` (`0x140780039`) with vector `(0,1,2,3)` at `0x144714ad0`. Remaining rows come from the direct/vector stores `0x14078028f`..`0x1407802e1`; this is one constant namespace, not an inference from adjacent IDs.

| Symbol | Symbol ID | Compiled native literal |
| --- | --- | --- |
| `$EVENT_PAR_0` / `_1` / `_2` / `_3` | 991 / 992 / 993 / 994 | 0 / 1 / 2 / 3 |
| `$EVENT_PAR_VOLUME` | 995 | 4 |
| `$EVENT_PAR_PAN` | 996 | 5 |
| `$EVENT_PAR_TUNE` | 997 | 6 |
| `$EVENT_PAR_NOTE` | 998 | 7 |
| `$EVENT_PAR_PLAY_POS` | 999 | 1026 |
| `$EVENT_PAR_ZONE_ID` | 1000 | 1027 |
| `$EVENT_PAR_SOURCE` | 1001 | 8 |
| `$EVENT_PAR_VELOCITY` | 1002 | 9 |
| `$EVENT_PAR_REL_VELOCITY` | 1003 | 27 |
| `$EVENT_PAR_ALLOW_GROUP` | 1004 | 10 |
| `$EVENT_PAR_MIDI_CHANNEL` | 1005 | 11 |
| `$EVENT_PAR_MIDI_COMMAND` | 1006 | 12 |
| `$EVENT_PAR_MIDI_BYTE_1` | 1007 | 13 |
| `$EVENT_PAR_MIDI_BYTE_2` | 1008 | 14 |
| `$EVENT_PAR_POS` | 1009 | 21 |
| `$EVENT_PAR_NOTE_LENGTH` | 1010 | 24 |
| `$EVENT_PAR_ID` | 1011 | 25 |
| `$EVENT_PAR_TRACK_NR` | 1012 | 22 |
| `$EVENT_PAR_CUSTOM` | 1013 | 28 |
| `$EVENT_PAR_MOD_VALUE_IDX` | 1014 | 29 |
| `$EVENT_PAR_MOD_VALUE_ID` | 1015 | 30 |
| `$EVENT_PAR_MOD_VALUE_EX_IDX` | 1016 | 33 |
| `$EVENT_PAR_MOD_VALUE_EX_ID` | 1017 | 34 |
| `$EVENT_PAR_DISALLOW_KEY_TYPE` | 1018 | 35 |
| `$EVENT_PAR_ALLOW_KEY_TYPE` | 1019 | 36 |
| `$EVENT_PAR_OUTPUT_TYPE` | 1020 | 37 |
| `$EVENT_PAR_OUTPUT_INDEX` | 1021 | 38 |
| `$NI_SYS_EVENT_PAR_1` / `_2` / `_3` | 1022 / 1023 / 1024 | 18 / 19 / 20 |

No `$EVENT_PAR_STATUS` binding occurs in this registration; status is not assigned a guessed event-parameter literal. KONTRA's existing event getters/setters and custom-array compilation consume the shared public constants, so named and literal parameters now address the same supported fields. This correction adds no private system fields or unsupported event actions; PLAY_POS retains its existing zero getter behavior. Import cache identity includes the builtin constant source, and the build identity already includes it.

Scripts that hardcoded KONTRA's former numeric aliases must adopt the native IDs: the old aliases collide with supported native fields and cannot remain active together. Persisted script globals and arrays contain untyped user values; they are preserved without numeric rewriting, even if an author stored an old parameter alias in them. Named constants in freshly compiled source use the corrected namespace; migrating authored raw numbers or saved values requires knowing their intended meaning.

The runtime getter confirms SOURCE literal 8 reads event `+0x24` at `0x1409919ba`, while MIDI_CHANNEL literal 11 reads `+0x8` at `0x140991a7d`. Shared event reset initializes SOURCE to -1 via the `+0x24` store at `0x1408dea54`; `play_note` stores the current script slot into `+0x24` at `0x14079ee01`. The public SOURCE value therefore records a script slot independently of the private RR marker, including for script-generated notes.

Actual event reset `0x1408dea00` initializes private fields `+0x27c/+0x280` to zero at `0x1408deada` and `+0x284` at `0x1408deae1`; it clears the public custom array from `+0x288` at `0x1408deae7`. Ordinary ingress creator `0x14086cd30` obtains an event at `0x14086d013`, calls this reset at `0x14086d027`, and inserts the same event into the program event list `+0x113a0` at `0x14086d0de`. Fresh script-generated allocator `0x1408e2fc0` calls the same reset at `0x1408e2fdf`; its direct caller `0x14079ed24` belongs to the `play_note` interpreter branch, independently identified by its named diagnostic references (`0x14079ec85`, `0x14079eebf`). That branch copies parent context only from event `+0x60` through `+0x25f` (`0x14079ed52`..`0x14079edb0`), excluding the private fields, then inserts the generated event through `0x140996050`. Fresh `play_note` therefore does not inherit a parent's private release marker. The native release-generation template explicitly uses `play_note` before its marker write; the slot and queue trace below establishes subsequent Group execution.

The native group block begins with `USE_CODE_IF_NOT(NO_SYS_SCRIPT_GROUP_START)` at `0x144f17cf8`. NI's [KSP preprocessor/system-script documentation](https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/advanced-concepts) confirms that defining this condition bypasses Group Start Options; `NO_SYS_SCRIPT_RLS_TRIG` is a separate condition. Failed group predicates call `disallow_group(group)` (template `0x144f18238`) after user callbacks, as the slot trace below establishes. A successful predicate does not re-allow a group rejected by a user callback. The emitted Key/CC/RR predicates additionally require nonnegative event note and velocity only when the generator's global slice-trigger count is positive (`0x144f18038`, `0x144f18088`); without slice-trigger rows it emits the predicate's closing parenthesis alone. The Boolean compiler table proof below establishes mixed-row operator precedence.

Group-mask and release association are separate from private-marker storage. Runtime ALLOW_GROUP getter case `0x140991a8a` reads the 4096-bit mask at event `+0x60`; a set bit means disallowed. Setter case `0x1409a9495` sets bits for rejection, clears bits for admission, and handles ALL_GROUPS by filling its 512 bytes with FF or zero. The `play_note` copy from `+0x60` through `+0x25f` is exactly this mask when the native callback kind is 1 or 2; other callback kinds clear the generated event's mask through `0x140797a90`. Fresh ordinary NoteOn mask clearing is performed in its dispatcher, as traced below; the shared private-field reset does not clear this mask.

Bounded generator `0x140876bd0`..`0x140878fc4` appends the release-group prelude and group-start predicates to the same note-callback buffer, in that order. When SYS3 is positive, release prelude template `0x144f17ae0` first disallows ALL_GROUPS, then template `0x144f17b80` re-allows the collected release-trigger groups; ordinary notes instead reject those groups (`0x144f17c20`). This replaces the inherited attack mask before RR predicates. The generated RR predicate reads the current global index (`0x144f180b8`), with no per-event RR snapshot emitted by this generator; the private marker suppresses incrementing, rather than recovering the original attack's index. The default key latches are initialized by inclusive comparison with program `+0x21b94`, then the same generated note buffer updates the key latches before RR/predicate evaluation, without a private-release-marker guard. The ascending-slot trace below establishes this order for notes reaching Group.

The release prelude and selector have separate preprocessing guards even though they share the slot-7 note callback. Template `0x144f17ae0` opens `USE_CODE_IF_NOT(NO_SYS_SCRIPT_RLS_TRIG)`; template `0x144f17ce0` closes this block before `0x144f17cf8` opens `USE_CODE_IF_NOT(NO_SYS_SCRIPT_GROUP_START)`. Defining GROUP_START therefore skips key/RR/random lifecycle and predicates but does not skip the release prelude. Defining RLS_TRIG independently skips that prelude and native release generation. An earlier inference that GROUP_START enclosed the prelude is rejected by the emitted order.

Operator templates emit AND, AND NOT, or OR between non-Always rows; Always rows are skipped unless slice-trigger handling needs an ordinary-note predicate. Slice rows use internal event markers derived from saved zone/slice IDs plus 1000; sequencer_only changes whether ordinary external notes are also admitted. Full slice sequencing and KSP reserved-event integration remain separate work.

The generator's Group source is program `+0x1c638`, with Release at `+0x1c578` and Pedal at `+0x1c4b8`. Compiler `0x1409723a0` traverses eight context pointers at `+0x1c078`, source-pointer base `+0x1c0f8` with stride `0xc0`: Group is physical slot 7, Release 6, Pedal 5. At `0x1409726f1` it passes that physical index to context initializer `0x1407a08d0`, which stores it at context `+0xc` (`0x1407a0903`). Note dispatcher `0x1408f476d`..`0x1408f47e2` starts with event `+0x1c+1`, invokes ascending slots through 7, and stops on ignored event `+0x2c` at `0x1408f47c5`. Thus ordinary surviving notes reach Group after user slots 0..4; slot-6 generated release notes reach Group slot 7. The `play_note` branch queues the event without calling its next note callback inline, so the release template's subsequent private-marker write precedes that callback. Earlier pre-user-selector integration proposals are superseded by this trace.

Fresh external NoteOn processing clears the disallow mask when event `+0x1c` is negative; generated notes preserve their copied mask and start after their source slot. The release prelude replaces that mask before condition evaluation. Key latches update for every note that reaches Group, including user-generated and native release-generated notes; ignored notes do not reach it. Random selection draws once for each executed Group note callback before predicates, including release-generated callbacks. RR advances only when private SYS3 is zero, and release predicates use the current global index rather than an attack snapshot.

The actual `BProgram` constructor `0x1407f8f20` has vtable `0x144e98338` (RTTI `BProgram`) and calls reset `0x14094e7b0` at `0x1407f9bca`. Reset writes `0xffff7f00` to program `+0x21b92` at `0x14094e9d9`, including signed default keyswitch `+0x21b94=-1`. Preparation `0x1408ea545` detects a negative default, scans the four Key rows in every group for the minimum signed key_min, and stores it at `0x1408ea5e7` if below 128. Nonnegative defaults, including those outside all ranges, receive no fallback. Range latches are then initialized by inclusive comparison with that value; a note in a gap inside the combined span clears every latch.

Native `%CC` binds symbol 1522 at `0x140467367`; lookup `0x140790948`..`0x140790952` subtracts 1522 to obtain internal array 0. Array reader `0x14078e570` case 0 calls `0x14078f9e0` with current event `+0x8`; CC 0..127 reads raw engine bank `+0x17314[(event.channel*128)+CC]` and shifts right 25. Ingress `0x14086bf00` writes this bank at `0x14086c08e` before invoking controller callbacks at `0x14086c327`..`0x14086c35d`; ignoring the controller stops later processing but does not undo the raw update. Group criteria therefore follow the final note event's mutable channel, rather than a captured attack channel or a group-local CC latch.

The same array reader accepts unsigned indices below 130. Index 128 reads packed bend bank `+0x1f314[channel]`, shifts right 18, and subtracts 8192 (`0x14078f9ec`..`0x14078f9f7`), yielding -8192..8191. Index 129 reads pressure bank `+0x1f414[channel]` and shifts right 25 (`0x14078fa06`..`0x14078fa0e`), yielding 0..127. Negative or larger indices do not authorize truncation to a MIDI CC number. The channel argument comes from mutable event `+0x8`, which MIDI_CHANNEL setter literal 11 can change; it is distinct from physical-input provenance.

Engine constructor `0x1407f3f00` clears the raw CC bank and explicitly writes zero to its 8192 entries at `0x1407f4448`..`0x1407f445c`. Bend entries receive packed `0x80000000` starting at `0x1407f4480`, corresponding to logical zero; pressure entries start zero. These are raw received-state defaults, independently of canonical controller gain/pan defaults. Authored controller generation queues an action through `0x140955b20`; action invoker `0x140996710` calls `0x140955c50` at `0x140996752`, which allocates a controller record through `0x1408e2550` into the engine `+0x140a0` pending list. Scheduler `0x14094fe20` dispatches this as kind 5 through the same `0x14086bf00` ingress routine. Thus raw state changes on delivery, not when the script issues the command. This bounded trace does not establish CC121 rewriting other raw slots: ingress writes the received controller's slot before callbacks and passes the surviving event to downstream canonical processing. Artificial canonical-reset notifications must not be treated as proven native raw input updates.

Native operator emission scans to the next nonzero mode and uses the preceding emitted row's operator (`0x140878054`, row `+0x7b8`), skipping intervening Always rows. All-Always groups without Slice rows emit no predicate. A leading Always row before an occupied row is a distinct malformed generation boundary: without Slice handling, `0x140877cc3` jumps to `0x140878025`, which sets the emitted-row flag and may append its operator despite emitting no predicate. It cannot safely be interpreted as an ignored leading row without tracing the native compile-failure policy. Runtime `condition()` already exposes final successful preprocessing state, but no indexed production caller consumes `NO_SYS_SCRIPT_GROUP_START`; implementing the bypass remains necessary. Malformed/reserved-row behavior remains an explicit integration boundary. This proof does not establish an imported wavetable-quality algorithm.

The actual `BGroup` constructor `0x1407f6fc0` constructs four physical criteria records of size `0x30` at group `+0x790` (`0x1407f7224`) and calls reset `0x14094dbb0` at `0x1407f7412`. Reset initializes all four mode fields to zero at `0x14094de59`, `0x14094de9b`, `0x14094ded8`, and `0x14094df15`; their cycle/operator qwords are also zero. Sparse saved rows must retain their physical slots with default Always rows in holes, rather than compacting later rows into slot zero. The leading-Always generation boundary therefore also applies when an absent default first slot precedes an active saved slot.

Actual Boolean precedence is established by the native KSP parser `0x1419f4550`, separately from its bitwise operator family. Rules `0x3af/0x3b0` pass `&`/`|` at `0x1419f7a35/0x1419f7a4f` through wrapper `0x1407a73d0` to constructor `0x140791200`, which writes Boolean AST kinds `0x2f/0x30` at `0x1407916a0/0x14079168e`. Evaluator `0x14078b780` short-circuits those kinds as AND/OR. The parser's raw tables are yypact `0x1478da680`, yydefact `0x1478d8cd0`, yytable `0x1478dc030`, yycheck `0x1478e71e0`, yyr1 `0x1478d8130`, and yyr2 `0x1478d88f0`. Logical token 45 shifts state 1445; Boolean nonterminal 651 then transitions to state 1913, which defaults to AND reduction `0x3af` and has yypact -633 (no shift). Token 46 shifts 1446 then transitions to 1914, default OR reduction `0x3b0`; this state shifts AND token 45 but does not shift OR token 46. AND therefore binds tighter than OR, with both left-associative. NOT rule `0x3ad` passes `!` through `0x1407afcd0` to `0x140797030`, constructs kind `0x2e`, and completes state 1395 with yypact -633, so it binds tighter than both. AND NOT negates the following parenthesized predicate. The earlier interim analysis of rules `0x2fa/0x2fb` is explicitly rejected as evidence for logical precedence: those are the separate bitwise `.and.`/`.or.` family.

## Voice handling and stable victim ties

The saved Voice Handling rule is read by `0x140d049b0` (archive version `0x60`): kill mode goes to rule `+0x2c`, Pref.Rel to `+0x30`, maximum sample voices to `+0x28`, fade milliseconds to `+0x34`, and exclusion group to `+0x38`. Program `+0x1fb30` contains 129 rules of size `0x40`, with the program rule first and saved voice groups at indices 1..128. Admission `0x1408e4f78`..`0x1408e4fb4` passes the selected rule to victim selector `0x14086fea0`; activation increments its count for each admitted sample voice.

The selector traverses the active intrusive list at engine `+0x11ed0`, following voice `+0x920`. Oldest/Newest compare voice `+0x534`; Highest/Lowest compare pitch `+0x518`. Every comparison is strict, so ties retain the first eligible voice. Allocation `0x14084f8d5`..`0x14084f8ef` supplies a null insertion position to `0x1408e16d0`; its branch at `0x1408e17f0` appends the new sample voice to the tail. Native equal-ranking ties therefore retain sample admission order. KONTRA previously relied on current vector position, which changes when an unrelated voice ends through `swap_remove`. A separate sample-admission serial now breaks ties after the existing rank, mode key, and note age; primary note age remains shared by layers.

Pref.Rel selects a separate best victim whose voice `+0x504` is zero, falling back to all eligible voices. Activation writes this field to one at `0x14096a70e`; actual release handling clears it at `0x14086afb2`. This proves released-first victims in the installed build, despite the retention-priority wording in the [NI Voice Groups manual](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/classic-view#voice-groups). The current released-first implementation is retained.

KillAny compares voice kill priority `+0x51c`, initially one. Native event reset `0x1408dead0` also sets event priority `+0x4d8` to one, copied to the voice at `0x14084f9af`..`0x14084f9b7`. Native interpreter statement 88 implements `set_kill_priority`, clipped to 1..1000, and helper `0x140968ba0` updates pending events and live voices. KONTRA does not expose this unsupported setter; the default equal-priority oldest tie is covered by the admission serial.

Native age `+0x534` starts at zero and increments per voice DSP invocation at `0x140946b51`. KONTRA retains its existing per-note admission age; DSP tick quantization and note-age equivalence are not established. Victim fade `0x140870095`..`0x1408700b2` passes the saved milliseconds to `0x14086d320` for the selected sample voice, but this bounded trace does not certify fade arithmetic, exact host-note ownership, or endpoint parity. The existing `u64` clock exhaustion boundary is unchanged. Staged independent PCM fixtures cover storage compaction, same-note layers, surviving owners, and the local saved-fade behavior; central compilation and execution remain separate validation.

## Local analysis artifacts

- `/tmp/kontra-kontakt8-verified-evidence.txt`: actual group read/write, named mode/operator menu bindings, pedal enum decoder, and named option/action gate assembly.
- `/tmp/kontra-kontakt8-program-options-writer.c`: pedal serialization writer and option access snippets.
- `/tmp/kontra-kontakt8-system-script.c`: bounded decompilation of native system-layer generator.
- `/tmp/kontra-kontakt8-pedal-option-access.asm`: bounded field-access disassembly.
- `/tmp/kontra-kontakt8-cc-delivery-core.c`: confirmed delivery-request list insertion.
- `/tmp/kontra-kontakt8-cc-request-lifecycle.c`: request consumption in controller enqueue.
- `/tmp/kontra-kontakt8-cc-ingress.c`: original-event enqueue and requested-part dispatch.
- `/tmp/kontra-kontakt8-pedal-mode-propagation.asm`: option-change rebuild epilogue.
- `/tmp/kontra-native-voice-victim-review.txt`: saved Voice Handling fields, victim comparisons, release preference, and stable sample-admission ties.

These are diagnostics from this precise build, not reconstructed Kontakt source. The repository report deliberately omits complete proprietary script templates and executable bytes. New importer and runtime-policy validation is pending central compilation and PCM fixtures.


## Program output amplifier: placement and smoothing (bounded follow-up)

This follow-up uses the same installed Kontakt binary/hash recorded above. The
bounded captures are preserved under ignored `artifacts/core-native/`; they are
static evidence, not an executed audio comparison. An initial prologue guess at
`0x140889570` was discarded: that is an adjacent sample-loop function. The actual
ordered program-output caller starts at `0x140889a30`.

The default `BProgramContainer` constructor writes mode `+0xe800=1` at
`0x1407f9f28`. In that mode, output amplifier `0x140825040` reads the selected
program's linear Volume `+0x1fb14` and Pan `+0x1fb18`. It owns separate retained
current scalars at container `+0xe750/+0xe754`; those scalars are not per voice.
The caller processes a first program rack through `0x140823050` at
`0x14088a312`, bus/send work through `0x140824470` and `0x1408efa90`, returned
sends through `0x140999260` at `0x14088a506`, and the final program rack through
`0x140823050` at `0x14088a5d9`. It then sums the program with `0x1408ef460` at
`0x14088a669`, and applies the container output amplifier at `0x14088a73c`.
Thus the default program output gain/pan stage follows both program racks and
returned sends. The [NI signal-flow reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view) independently describes the Instrument Insert chain on the summed Group/Bus signal, parallel returned sends, and the final Main Effects chain. The exact amplifier placement and scalar recurrence here come from the installed binary, rather than that overview. Mode2 instead applies a program amplifier while summing each
program through `0x1408ef460`, followed by the container's separate gain/pan.
This does not admit native bank/program switching.

The gain branch computes float
`abs(current + 1e-12) / abs(target + 1e-12)`. If that ratio is above
`1.0010000467300415` or below `0.9990000128746033`, each channel starts from the
same retained current gain and, for every sample, multiplies by that current
value before updating it as `current + (target-current)*0.0005555555690079927`.
The literal coefficient at `0x144f385f8` is float1/1800. Only the final channel's
identical recurrence is retained. In the tolerance branch, samples multiply by
the target directly; the retained current scalar is not snapped to the target.
The branch is selected once per native program-processing buffer. Its exact
buffer dependence must not be replaced with a decision per KONTRA128-frame,
MIDI, or timer fragment.

For a two-channel output, pan uses a separate current-point-first linear slew:
positive step is float `(10/rate)+(10/rate)`, negative step is float `-20/rate`,
with target crossing clamped. When the absolute difference is at most the
positive step, it uses the target directly without updating retained current
pan. Helper `0x140823d70` applies linear stereo balance: left factor
`1-clamp(pan,0,1)`, right factor `clamp(pan+1,0,1)`. There is no equal-power
panner in this bounded path. The denominator is engine `+0x27d00`; setter
`0x140967c60` stores the same incoming float there and at `+0x116c` at
`0x140967d1e..0x140967d26`, and forms its reciprocal beforehand.

The empty-buffer boundary is also material. The output caller tests container
main-buffer `+0x478` count `+0x18`; an empty buffer sets `+0xe758=1` and skips
this amplifier. At the next fed buffer, the amplifier seeds both retained
scalars directly to their current target and clears that flag. Counting all
voice objects, including cold/pending voices, cannot reproduce this boundary.
A zero-valued but genuinely processed voice buffer is distinct from an absent
buffer. Native effect-tail activity also needs an explicit owner.

External send routing is separate: `0x1408efa90` handles a send slot whose
output field `+0x1b8` is not-1, routes its own output through `0x14081ecd0`, and
passes program Volume separately. `0x140999260` handles returns with output-1
before the final program rack. The saved selector is now closed independently:
`BParFX` private reader `0x140d09aa0` (v0x50) reads the final signed word into
`+0x28`; effect preparation at `0x140962c5f..0x140962c62` and
`0x140963554..0x140963557` copies it unchanged to runtime effect `+0x1b8`.
The external-send caller searches 128 output records by their signed 16-bit
output tag (`record+0x0a`), then resolves their channel count and buffers through
`0x140884270`. Record clear `0x14094ee00` initializes the tag to -1; the main
render caller `0x140889a30` also selects tags 0..3 for the four auxiliary
levels. This is an authored tag, not a proved persistent unique identity or
host output index. The primary
[NI effect-routing reference](https://docs.native-instruments.com/ni-tech-manuals/kontakt-manual/en/using-filters-and-effects-in-classic-view)
also specifies that separately routed send output bypasses Main Effects.

KONTRA retains the selector as per-slot `ProgramFx::send_outputs` metadata.
Only selector -1 (or legacy metadata absence) joins the main return path.
Other tags are diagnosed as unavailable until a native-output/host-output
association exists; their DSP and controls remain intact, but their output and
work receipts do not enter the main mix or its amplifier. This corrects the
previous silent folding of external sends into the main output. It does not
implement external output transfer, its gain law, or native effect-tail parity.
Older JSON metadata defaults to the existing instrument return; importer source
hashes invalidate old file caches so actual saved selectors are re-read.
Bounded captures are `artifacts/core-native/external-send-slot-private-reader.c`,
`external-send-output-field-binders.asm`, and `external-send-output-transfer.c`.
The
actual caller at `0x140889a30` handles a directly routed Bus by applying its own
amplifier through `0x140824910`, then transferring through `0x14081ecd0` with
unity gain, bypassing the program racks and program Volume/Pan. This closes the
Direct Bus header bypass separately from returned buses. The primary signal
flow reference also specifies direct Group/Bus bypass; the exact native Direct
Group transfer remains outside this bounded caller closure.

Production now owns one `ProgramAmplifier` in `InstrumentState`. Native voice
planning receives neutral instrument gain/pan, while the effective instrument
parameters retain their existing authored/physical getter authority. The new
stage applies gain and then balance after the final program rack and returned
sends; direct outputs retain their own amplifiers. Compatibility per-voice
instrument controls, authored `set_controller` output gain/pan, and plugin part
faders keep their prior namespaces and placement. Compatible residency fills
retain the amplifier; destructive bank replacement starts a new owner.

Branch decisions persist across fragments of one balanced host window. An
actual sample-timed target change starts a new branch within that window: this
is KONTRA's progressive MIDI/write adapter policy, not a claim that Kontakt
splits its processing buffer at those same events. A `Voice::Rendered` receipt
records actual nonempty rendering, including silent/muted work and a rendered
prefix before a post-admission underrun; cold or rejected plans produce no
receipt. Routing determines whether that work enters the program buffer.
Program/return/program-routed Bus receipts now propagate actual input work.
Bypassed slots and Send Levels taps add no independent work. The native Gainer
is stateless: its `+0xa0` setter `0x14095fde0` copies upstream work to `+0x1b7`,
and its `+0xa8` trampoline `0x14087f7d0` enters `0x140a98850`, which checks that
flag or typed input-buffer counts without inspecting PCM values. Accordingly,
KONTRA Gainer adds no idle work; actual quiet input still propagates through it.
The native `BEffectStereo` identity is independently resolved through type
descriptor `0x14a3ab290`, COL `0x1498a2350`, and vtable `0x144e961d0`.
Its upstream-work setter +`0xa0` is `0x140abca80`, which copies the receipt to
+`0x1b7`, and activity method +`0xa8` is the same `0x140a98850` typed-input
check. KONTRA's plain Stereo Modeller owns only width and two balance scalars;
its per-sample mid/side matrix has no retained signal, delay, or independent
tail. It therefore also propagates input work without keeping an absent program
buffer alive. This fixes idle reseeding of the Native program amplifier while
preserving every stereo arithmetic operation and DSP call. Pseudo Stereo
remains explicitly unsupported; no Haas/delay behavior is admitted by this
activity evidence. Captures are `program-stateless-activity-candidates.txt`,
`program-stereo-identity.txt`, and `program-stereo-upstream-work.c`.
Returned group sends record nonempty, nonzero-level tap delivery separately
from the dry route and consume that receipt once per event interval.
Other enabled local DSP retain conservative work, including zero-input tails;
their expiry is not claimed equivalent to native `+0xa8` flags. Native Reverb
`0x14087f920` tests retained `+0x1470>0`, and IRC `0x140a988e0` combines base
input work with `+0x400>0`; those fields do not establish the corresponding
local DSP expiry law. Captures are `program-fx-rack-work.c` and
`program-effect-activity-methods.txt`. An absent local program buffer clears the seed marker; Engine
reset also clears it after removing all voices and effect state. Neither path
changes parameter values. Empty render calls do not advance the amplifier.
Local empty-buffer detection is progressive: an absent-input prefix can reseed
a later onset in the same host window. The native caller sees a whole-buffer
receipt, so this onset/idle scheduling boundary remains an explicit adapter
policy rather than native whole-window admission parity.

Protected immediate-before source snapshots accompany the captures. Production
compilation and independent complete-output fixtures are separate validation;
this static evidence does not claim native callback scheduling, external-send,
bank-mode, or proprietary processor parity.


### Program-bank amplifier and retained container metadata

The same default amplifier helper `0x140825040` selects ordinary
Program gain/pan at `+0x1fb14/+0x1fb18` for container mode1, and
Container public gain/pan at `+0x44c/+0x450` for mode2. The actual
render caller `0x140889a30` applies it after the inner program outputs
have been summed. Mode2 additionally applies each inner Program's
gain/pan in `0x1408ef460` before that sum, with mutable state at
Program `+0x1cf68/+0x1cf6c` seeded by the container fresh-work flag
`+0xe758`. These are distinct owners. The standard CC7/10 template
`0x144f18260` still names ordinary Volume/Pan rows660/661; container
scalar rows671/672 are separate. This does not establish a standard
CC7/10 write to the shared container controls.

The saved container output word resolves through `0x140965180` to
`0x1408843d0`: high16 is channel count, low16 is occurrence ordinal
among configured records with that count. It is not a host-output
index. The saved output configuration retains its channel map and
physical records; the output record's signed tag at+0x0a is also used
by auxiliary levels and external sends. Explicit association with a
KONTRA host sink remains required until the audio binding is closed.

Parent Bank master-tune is a pitch ratio (Voice initializer
`0x14084fde2..0x14084fdfc`); master-volume is an output gain (final
render `0x140908154` onward), making1 neutral for each. Saved master
tempo is integer milli-BPM at manager+0x1188: constructor
`0x1407f2de0` loads120000; setter `0x1408d00f0` multiplies f32BPM by
1000 and truncates to i32. Source preparation uses explicit transport
BPM when Engine+0x11c00 exists, otherwise converts the saved integer
to float and multiplies by0.001. These unit/ownership findings do not
claim standalone NKB topology or broader bank output/tail parity.
Protected archive, flag, voice-rule and routing captures are indexed
in `artifacts/core-native/program-container-saved-metadata-contract.md`.
