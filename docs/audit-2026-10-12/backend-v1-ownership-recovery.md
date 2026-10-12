# Preserved v1 ownership contracts recovered into v2

Source authority is the immutable preserved stash
`b89b78247f83acc8d6932aa930cd4412e9d9e958`, its third-parent untracked tree, and
current v2 baseline `972b4410`. The stash and donor worktrees were read only.
No build, Cargo, test executable, native plugin or library scan was run.

## Actual consumer recoveries

`91523ee5` restores physical MPE RPN0 cents and RP-018 one-cent relative steps,
including coarse-entry clearing of a previous fine byte. Manager and shared
member sensitivities remain separate. Member pitch stays frozen after physical
key-up; manager changes continue reaching those tails through the existing
binding snapshots. The existing whole-semitone API retains its coarse fields;
`pitch_sensitivities` exposes the effective values. Proper MIDI2 registered
controllers retain cents; deprecated MIDI1 bank/parameter CC numbers are ignored
before parameter rebinding, controller callbacks or articulation dispatch.
Incoming MIDI1 parameter CCs now reach their original physical controller origin
before the existing MPE projection. Literal pitch/ownership, protocol-role,
controller-origin and heap regressions are authored.

`d7725cb1` carries original normalized physical NoteOff velocity from CLAP f64
and VST3 f32 exact event bodies through the core event, alignment queue, Runtime
release context and KSP `EVENT_PAR_REL_VELOCITY`. UVI receives the 0..127 projection
while the core retains the original normalized value. The old velocity-absent
NoteOff constructor remains usable. Invalid normalized releases cannot close an
owner.

The same commit separates a host tuple from its internal execution ID. A bounded
per-part allocator chooses an ID not held by another root; `Held` retains the
physical `(port, channel, key, note_id, protocol)` tuple. Distinct physical
channels can carry the same host ID/key after part routing collapses their
musical channel. Anonymous starts stack; duplicate concrete tuples remain
rejected until terminal acceptance, including concrete negative VST3 IDs.

Exact CHOKE now uses `Runtime::cancel_input_note` and the existing nonmusical
discard cleanup. It stops only the matched physical owner, linked sources,
delayed starts and unfinished note callbacks, bypassing musical release callbacks
and pedal retention. Independent generated children and sibling roots preserve
their own lifetimes. Previously CHOKE shared the normal NoteOff arm. Late UVI
commands whose recorded parent has been pruned can no longer become independent
generated roots. The current discard path hard-stops sources; the v1 donor used
a short steal fade. This restores cancellation/ownership, **not native PCM equality
of that fade**.

Authored regressions cover exact adapter conversion, delay retention, tuple
isolation, anonymous/negative IDs, release metadata, invalid values, deferred KSP
work, UVI projection and stale parent rejection. The separate core cancellation
fixture exercises 64 sounding voices, a reserved future start, sustain/sostenuto,
linked and independent children, terminal refusal/retry, source isolation,
serial/parallel rendering and the existing heap guard. They are NOT_RUN.

## Exact source identities and comparisons

| Preserved source | Blob | Consumer disposition |
| --- | --- | --- |
| tracked `src/engine/host_notes.rs` | `e9f023fcb82d580322dc81339fd58e27db86f0e8` | Full-tuple duplicate policy, anonymous starts and physical release recording were missing at the v2 shell boundary and are recovered. Its canonical shared allocation pool for retained program banks remains a separate prerequisite. Runtime already uses typed generational NoteId/ExpressionId ownership rather than copying v1 RootBinding. |
| tracked `src/engine/mod.rs` | `15b8317f8666dc9976103fdc37e4c1a5c68692dd` | `host_note_choke` suppresses owner-specific pending starts/releases and does not call channel panic. The v2 hook now reaches existing discard cleanup; generic envelope/LFO and voice consumer changes belong to the scheduler lane. |
| untracked `src/midi.rs` | `b7611b2d60ecbf46a84938583fd3d636b1724391` | Original normalized velocity is already retained by v2 MIDI2 notes; normalized host release was lost and is recovered. MIDI2 legacy-CC policy and RPN coarse/fine/relative behavior are now consumed by sampler-midi. Full paired-CC receipts, high-resolution MIDI1 velocity prefixes, dynamic dual-zone configuration and RPN tuning 1/2 are larger remaining scopes. |
| untracked `src/plugin/shared_primary_mpe_rpn_tests.rs` | `2d9d938ca6ecc188a5b2b1f68ab75e746cb0ae88` | Its literal physical manager/member coarse/fine and relative-pitch contracts were reauthored through current Mpe APIs. Original PCM/source-window admission belongs to the native primary/ordinary voice lane; its old fixture is not copied wholesale or called native runtime evidence. |
| untracked `src/engine/program_bank.rs` | `8dc9b500fc309641013fac4f96a200aecbe56c63` | Real retained script-free bank owner is absent v2. Its header explicitly keeps plugin admission separate. See concrete remaining prerequisites below; it is not claimed equivalent to fixed rack program selection. |
| tracked `src/engine/rack_timeline_tests.rs` | `e8929fbdf8688378aeb83347aa2d6aab76a27777` | Its chronological part/source clocks and per-owner output routes are partly covered by current core sample scheduling, host transport ABI, alignment and `tests/one_rack.rs`. Shared-primary census/growth and retained program adoption require the scheduler/whole-voice and bank-owner work; no whole-file equivalence is asserted. |

Existing v2 behavior concretely supersedes these narrower donor components:

- MPE physical member key-up snapshots, live manager tail changes and channel
  reuse are already exercised by
  `member_pitch_freezes_at_physical_key_up_and_manager_still_reaches_tails` and
  `pressure_and_timbre_keep_member_snapshots_and_preserve_unrelated_expression`.
- Manager sustain/sostenuto zone capture, ignored member pedals and admission
  pressure are current in `manager_pedals_capture_the_zone_once_and_member_pedals_are_ignored`
  and `zone_pedal_admission_is_atomic_and_pedal_up_needs_no_channel_capacity`.
- First physical release context, normalized velocity, storage EOF, plan
  replacement and refused terminal delivery already exist in core
  `physical_fifo_context_survives_eof_pedals_replacement_and_terminal_retry` and
  `scheduled_release_context_is_sample_exact_and_first_key_up_wins`. The recovered
  shell forwarding makes those capabilities available to exact host releases.
- Family-local delayed starts, ordered choke and generation-safe slot reuse are
  current in core `tests/choke.rs`. The new exact-input bridge cancels all linked
  families of the target physical owner using that existing cleanup path.
- KSP script IDs, typed physical controller origins, host automation and generic
  sample clocks already use current core domains; old hash/positional/monolithic
  adapters are not restored.

## Concrete remaining prerequisites

The retained program-bank owner is materially missing. Current
`sampler-kontakt::read_program_with_controls` sorts outer slots, flattens program
lists and loads a dense selected program into a fixed rack part. It cannot keep
old-program voices/roots/receivers while MIDI Program Change selects a new physical
bank slot. The preserved owner instead retains engines, a `[Option<u8>;128]`
physical lookup, selected slot, canonical roots and a borrowed raw receiver;
`off`, `host_off`, `host_expression`, `render` and terminal accounting revisit
all relevant retained programs.

Its admitted boundary is deliberately narrow: mode2, clean container flags,
valid selection, one addressed channel, no slot aux/flags, unit bank master
volume/tune, positive saved tempo, one stereo output with mapping `[0,1]`, no
output FX, permissive voice budget and script-free inner programs. Unsupported
headers/routes/voice arbitration reject preparation. Copying only Program Change
would lose the ownership contract.

A current-architecture port needs (1) bounded typed bank/slot/output metadata
while preserving the newer tag-based `slot_list` lookup, (2) off-audio prepared
program generations with physical slot identity and their stream/resource
lifetimes, (3) an audio owner that routes starts to the selected generation but
routes releases/expressions/terminal delivery to retained generations, and (4)
matching per-generation mixer/UI publication. No existing public current Runtime
API supplies direct prepared-plan selection with all that shell metadata. The
old `Bank` donor also reverts the safe tag lookup to child7; do not copy it.

Other real remaining scopes are RPN1/2 channel tuning, dynamic dual-zone MCM,
paired MIDI1 CC receipts/velocity prefixes, exact terminal sample timestamps
through the shell's `Core::end_block` callback (it currently conveys only HostNote),
and retained part replacement across held notes. The preserved v1 owners retain
`pending_end` independently of voice removal; current core records first release
sample context, but the shell callback lacks that terminal timestamp. These
require explicit current consumers and regressions, not a name-only declaration
of supersession. Native binary/runtime and full-instrument parity remain unproved.

## Central validation commands (not executed here)

```sh
cargo test --locked --profile ci -p sampler-midi --test mpe --test controller_dispatch --test channel_expression
cargo test --locked --profile ci -p sampler-core --test input_cancellation --test choke --test release
cargo test --locked --profile ci -p sampler-uvi --lib scripted::lifecycle_tests
cargo test --locked --profile ci -p kontakto --lib sound::v2::host_contract_tests
cargo test --locked --profile ci -p kontakto --lib exact_host_release_adapter_keeps_clap_f64_and_vst3_f32_velocity
cargo test --locked --profile ci -p kontakto --lib normalized_host_release_velocity_survives_exact_alignment_and_key_reuse
```

Executed static checks: `git diff --check`; rustfmt checks on changed small
modules and authored regressions; parser-only rustfmt stdout for existing large
v2/plugin/timing/UVI driver modules, discarding formatting output. These do not
prove compilation, runtime behavior, PCM parity or performance.
