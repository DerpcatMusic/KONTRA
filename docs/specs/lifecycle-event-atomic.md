---
status: draft
project: plugins/kontra
owner: antigravity
created: 2026-11-26
updated: 2026-11-26
proof_scope: internal
evidence_freshness_days: 180
---

# Event-Driven Lifecycle, Zero-Poll Drag Path & Atomic Generation-Fenced State

## Problem & Goal

- **Problem:** Dragging scripted library controls (such as Analog Strings' four large sliders or Conflux's native UI knobs) lags noticeably behind the pointer and drops responsiveness, while concurrent preset swaps, part additions/removals, and DAW automation carry real torn/stale-state hazards:
  1. **Double 50 ms polling loop on every drag round-trip:** Audio spawns `Tick` on a 20 Hz timer (`plugin/src/lib.rs`, `s.rate / 20.0`) and never wakes `Tick` when `Command::Look` returns (`crates/app/host/src/rack/audio.rs`). `Tick` then flips `dirty = true` without waking the editor window, leaving the UI waiting up to another 50 ms on `.poll(50 ms)` (`plugin/src/editor.rs`). Meanwhile, `Rack::tick` unconditionally polls `Command::Look` and `Command::Capture` every 50 ms even when the engine is completely idle (`crates/app/host/src/rack.rs`).
  2. **Double full-tree deep clone + full layout resolve + Conflux memo cache-busting:** Every single revision bump deep-clones the entire `Interface` in `Snapshot::refresh` (`plugin/src/editor.rs`), deep-clones it a second time in `Snapshot::fill` → `resolved()`, and runs `layout::resolve(0..tree.widgets.len())` across every widget — discarding the exact `Changes { widgets }` list `Live::refresh` already computed (`crates/app/face/src/lib.rs`). On Conflux, `nui::Shown::refresh` (`crates/app/face/src/nui.rs`) re-paints the full page and returns all widgets dirty, while shifting widget indices bust MUI memos keyed by positional Vec index (`format!("kontra-ui-{face}-{}", n.0)` in `crates/app/ui-view/src/lib.rs`).
  3. **Zero per-control drag coalescing & global head-of-line blocking:** Rapid pointer moves push N separate `Command::Control` entries into the ring and execute KSP `on ui_control` / `on ui_controls` / `on ui_update` N times synchronously in one audio block. In `RackAudio::apply_commands`, a single full return ring among 9 unrelated channels halts draining of every incoming command (including `Command::Control` and `Command::Panic`).
  4. **Missing generation fencing, positional part aliasing, and premature optimistic-hold ACKs:** `Command::Control`, `ControlText`, `ControlDrop`, `Widget`, `Param`, `Sound`, `Mix`, `Hold`, `Remove`, and `MacroTarget` carry no `generation: u64`, allowing an edit or DAW automation move racing with a preset swap to mutate the newly loaded instrument. `Snapshot::fill` and `mine.keep` match parts by positional Vec index instead of `slot`, aliasing state across parts when part 0 is removed. `ui_applied` increments when `Tick` enqueues into the ring rather than when audio executes the command, letting optimistic holds release early.
- **Goal:** Make KONTRA's control, script, and UI lifecycle fully event-driven, incremental, and atomically generation-fenced (matching KURV's zero-poll `WorkerWake`, two-speed invalidation, and host-authority boundary rigor):
  1. Interactive drag round-trip (`UI → Audio → Script → UI`) drops from ~50–100 ms (2 polling timers) to **≤ 1 audio block + 1 display frame** via sticky push wakeups.
  2. Idle CPU/IPC work drops to **0 `Command::Look` and 0 `Command::Capture` per second** when no events or scripts ran.
  3. Single-control drag rebuilds **only changed widgets** (0 full-tree double clones, 0 full-page `layout::resolve`, stable non-positional MUI memo keys).
  4. **Zero stale/mixed state** under concurrent preset swap, part add/remove, snapshot recall, and DAW automation — proven by deterministic boundary tests.
- **Plan Reference:** `docs/architecture-v3/VISION.md` (modern, RT-safe, efficiency far below originals); mesh task `#139`.

## Primary-Source & Version Proof

N/A — internal refactor: all IPC, wakeups, UI layout, and command-ring types live inside KONTRA and the in-tree `moose-mui` editor bridge (`MuiEditor::install_host_waker` / `Requests::redraw`), with no new external SDK or protocol assumptions.

## Pinned Crate & SDK Ground Truth

| Dependency / Tool | Pinned Version | Lockfile / Header Source | Verified Symbols / Features |
|---|---|---|---|
| `kontra-host` | workspace | `plugins/kontra/crates/app/host/Cargo.toml` | `Rack`, `RackAudio`, `Command`, `LookFrame`, `MacroTarget`, `View` |
| `kontra-face` | workspace | `plugins/kontra/crates/app/face/Cargo.toml` | `Live::refresh`, `Changes`, `nui::Shown` |
| `kontra-ui-view` | workspace | `plugins/kontra/crates/app/ui-view/Cargo.toml` | `page_as`, `ui.memo` |
| `moose-mui` | in-tree | `frameworks/moose/crates/moose-mui/src/editor.rs` | `MuiEditor`, `Requests::redraw`, `install_host_waker` |

## Invariants & Non-Goals

### Hard Invariants
- **Audio RT-Safety:** Zero heap allocations, zero locks/mutexes, zero blocking syscalls, and zero panics on the audio processing thread (verified with `--features rt-paranoid`). Audio wakeups use only a non-blocking sticky `AtomicBool` + coalescing task wake (never blocking or allocating).
- **Determinism & Compatibility:** Saved plugin state (`Saved`, `SavedView`, `.nksn`, `.kontra`) stays 100% backward-compatible. Offline bounces remain bit-identical and block-size invariant.
- **Mirror Boundaries:** Zero private monorepo paths, local filesystem paths, RE markers, or secrets in `plugins/kontra`.
- **Additional Domain Invariants:**
  - Every part-targeted audio command and `MacroTarget` carries `generation: u64`; `RackAudio` MUST discard any command whose `generation != p.generation`.
  - Removing or inserting a part at slot `k` MUST NOT transfer `open`, `tab`, `look`, `face`, `gesture`, or `Mine` holds to any other slot.
  - Optimistic drag holds (`Mine`) MUST remain active until `LookFrame.applied_seq >= held.seq` (acknowledged by `RackAudio` after executing the command), or until the fallback timeout if no reply arrives.
  - Coalescing drag moves MUST preserve press/release edges and modifier-key transitions; only intermediate continuous moves for the exact same `(slot, generation, widget, cell)` collapse to latest-wins.

### Non-Goals
- Rewriting KSP/Lua script semantics or adding new DSP effect modules (owned by separate parity specs/lanes).
- Changing MUI's core layout engine or adding GPU-backend-specific code.
- Adding full audio-rate modulation across Kontakt/UVI legacy paths in this PR (tracked separately as a post-parity "go beyond" spec).

## API / Data / File Contract

### 1. Generation-Fenced Commands & Sequence ACKs (`crates/app/host/src/rack/command.rs`, `rack/audio.rs`)
- Add `generation: u64` to `Command::Control`, `Command::ControlText`, `Command::ControlDrop`, `Command::Widget`, `Command::Param`, `Command::Sound`, `Command::Mix`, `Command::Hold`, `Command::Remove`, and `MacroTarget`.
- Add `ui_seq: u64` to `Command::Control`, `Command::ControlText`, `Command::Widget`, and `Command::Param`.
- `Playing` tracks `last_ui_seq: u64`, updated on the audio thread immediately after executing a generation-matching UI command, and copied into `LookFrame.applied_seq`.
- Replace the global 9-ring guard in `RackAudio::apply_commands` with per-command room checks (peek the next command and check only the return ring(s) that specific variant writes to; `Control`, `Mix`, `Hold`, `Master`, `Tempo`, `Panic`, `Play`, `Macro`, `Param` require zero return-ring space and never stall behind `looks` or `captured`).

### 2. Event-Driven Push Wakeups (Zero 50 ms Drag Poll)
- **Audio → `Tick` (`crates/app/host/src/rack/audio.rs`, `plugin/src/lib.rs`):** Track engine `ui_epoch` / `persist_epoch` (bumped only when scripts mutate UI controls/pages/keybed or persistent cells) and expose a sticky atomic `wake_needed: AtomicBool`. Only request `Command::Look` / `Command::Capture` when the engine's epoch advanced since the last snapshot (plus one initial fetch on editor open), and trigger `tasks.spawn_coalescing(Tick)` immediately after `RackAudio::process` when `wake_needed` transitions false → true. Keep only a low-frequency fallback housekeeping tick when audio is actively producing meters/stats.
- **`Tick` → Editor window (`frameworks/moose/crates/moose-mui/src/editor.rs`, `plugin/src/editor.rs`):** Expose a thread-safe coalesced `EditorWaker` handle on `MuiEditor` backed by the existing `wake_pending` + `Requests::redraw()` mechanism. `Tick` invokes `waker.wake()` immediately upon publishing a refreshed `Snapshot` (and when pictures arrive), eliminating the 50 ms `.poll` wait on interactive drags while keeping `.poll` strictly as an idle-safe fallback (or removing it once scan/progress push directly).
- Remove the `std::thread::sleep(Duration::from_millis(1))` spin loop in `KontraParams::capture` (`plugin/src/lib.rs`) using condition-variable or lock acquisition bounded by `holding`.

### 3. Coalescing, Incremental Layout & Stable Memo Identity
- **Drag coalescing (`plugin/src/lib.rs`, `crates/app/host/src/rack.rs`):** Before dispatching a drained `Request` batch to `RackAudio`, coalesce consecutive `Action::Control` edits targeting the same `(slot, widget, cell)` with identical modifiers to the latest value (preserving press/release transitions and ordering relative to non-matching actions).
- **Incremental `Interface` updates (`crates/app/host/src/view.rs`, `plugin/src/editor.rs`):** Share `Arc<Interface>` with `Changes { page, keys, widgets: Vec<WidgetRef> }` (or resolve layout once on the `View` side over `changes.widgets` only, publishing an already-resolved `Arc<Interface>` so `Snapshot::fill` does **0 full-tree clones** and **0 full-tree `layout::resolve`** calls).
- **Stable MUI memo keys (`crates/app/ui-view/src/lib.rs`):** Key widget memos by stable identity (`source_id` + widget kind/name fallback) rather than positional `n.0`, so Conflux `native_ui` splices do not bust unrelated widget memos.
- **Conflux `native_ui` dirty painting (`crates/app/face/src/nui.rs`):** Return only spliced/dirty widget refs instead of `order(bld, self.first)` (all widgets) and invoke `paint()` only on nodes whose subtree actually updated.
- **Slot-keyed editor reconciliation (`plugin/src/editor.rs`):** Replace positional `old.get_mut(n)` / `shown.get(n)` / `app.parts.iter().zip(&view.parts)` with `HashMap<usize, ...>` or explicit `slot` lookup so removing/inserting parts never aliases state.

## Acceptance Criteria

- [x] **AC1 (Event-driven wake):** Dragging a KSP or Conflux control wakes `Tick` and redraws the editor without waiting on the 50 ms poll timer; measured round-trip in `drag_probe` is ≤ 1 audio block + 1 frame (`lifecycle_audio_wakes_tick_immediately_on_drag_and_stays_quiet_when_idle` + `EditorWaker`).
- [x] **AC2 (Zero idle IPC):** With an open editor and idle instrument (no notes, no clicks, no script activity), `Command::Look` and `Command::Capture` counts stay at 0 across 100 blocks (`lifecycle_zero_idle_look_and_capture_across_100_blocks`).
- [x] **AC3 (Drag coalescing):** 16 rapid drag moves for the same control arriving in one batch invoke KSP `on ui_control` once (latest value) while preserving distinct control / press / release ordering (`lifecycle_drag_coalescing_collapses_16_moves_preserving_edges_and_order`).
- [x] **AC4 (Incremental UI & stable memos):** Single-slider drag on Analog Strings and Conflux performs 0 full-tree `layout::resolve(0..N)` passes (`RESOLVES` delta = 0 on Analog Strings, VWinds, Una Corda, and all 18 steady-state moves on Conflux after initial popup splice) and rebuilds only the changed widget(s) in `kontra-ui-view` (`lifecycle_incremental_drag_performs_zero_full_resolves`, `lifecycle_stable_memo_keys_survive_widget_splice_without_busting_tail`).
- [x] **AC5 (Atomic generation fencing & slot keying):** Boundary tests prove: (a) stale `Command::Control`/`Mix`/`Param`/`MacroTarget` after a preset swap are dropped without mutating the new generation; (b) removing part 0 preserves part 1's `open`/`tab`/`look`/held-drag state intact; (c) optimistic drag hold persists until `LookFrame.applied_seq >= seq`; (d) a full `looks` or `captured` ring does not block `Command::Control` or `Command::Panic`.
- [x] **AC6 (RT-safety):** `rt-paranoid` passes with 0 audio-thread allocations/locks/syscalls (`reset_is_realtime_after_prepare`).

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/lifecycle-event-atomic.md` | Exit 0, zero proof errors | PASS (2026-11-26, linux-x86_64, checked=1 passed=1 warnings=0) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `build-slot cargo nextest run -p kontra-host -p kontra-ui-view -p kontra-face -p kontra-ui-pictures` + `build-slot cargo nextest run -p moose-mui` | Exit 0, zero failures | PASS (2026-11-26, linux-x86_64, all `kontra-host`/`kontra-ui-view`/`kontra-face`/`kontra-ui-pictures`/`moose-mui` tests green; `kontra-plugin` 73/74 green — pre-existing `origin/main` Conflux `$Header__Page` `Engine::new` reload diff) |
| Lifecycle boundary tests | Validator/Runtime | linux-x86_64 | `build-slot cargo nextest run -p kontra-host -p kontra-plugin -p kontra-ui-view -p kontra-face -E 'test(lifecycle)'` | All fencing, coalescing, HoL, and slot-aliasing tests pass | PASS (2026-11-26, linux-x86_64, 8/8 lifecycle boundary tests green) |
| RT-safety audit | Validator/Runtime | linux-x86_64 | `build-slot cargo test -p kontra-plugin --features rt-paranoid reset_is_realtime_after_prepare` | Zero RT violations | PASS (2026-11-26, linux-x86_64, `reset_is_realtime_after_prepare` ok, 0 RT violations) |
| Real-library drag probe | Validator/Runtime | linux-x86_64 | `build-slot cargo test --release -p kontra-plugin -- --ignored --nocapture a_drag_on_a_real_page_frame_by_frame` | Analog Strings & Conflux: 0 full resolves per drag frame, ≤ 1-block round-trip | PASS (2026-11-26, linux-x86_64, Analog Strings 1.55 ms move / 1.63 ms echo `full resolves=0` 6.5 widgets/move; VWinds & Una Corda `full resolves=0`; Conflux 0.96 ms move / 1.23 ms echo `full resolves=1` initial popup splice then 0 across moves 3..20) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`, `python3 tools/sync-stack.py --check` | fmt, clippy, stack/mirror boundaries green | PENDING full `matari check origin/main` run (local `clippy -D warnings`, `fmt --check`, `sync-stack.py --check` 19/19 projects & 4/4 mirrors PASS) |

## Unverified Boundaries & Promotion Gate

None — all verification tiers execute on the current host.

**Hard Promotion Rules:**
1. This spec **MUST NOT** move to `status: implemented` while any required row in `## Verification Matrix` is `PENDING`, `PENDING_OS_CI`, or `BLOCKED`, or while any unchecked `- [ ]` acceptance criterion remains.

## Task Decomposition

| # | Subtask | Owned Paths | Depends On | Verification |
|---|---|---|---|---|
| 1 | Generation-fence all audio commands + `MacroTarget`, add `ui_seq` ACK in `LookFrame`, and remove global 9-ring HoL guard | `plugins/kontra/crates/app/host/src/rack/` | — | `matari fast kontra -p kontra-host` |
| 2 | Add `EditorWaker` to `moose-mui` and wire sticky audio→`Tick`→editor push wakeups + dirty-gated `Look`/`Capture` | `frameworks/moose/crates/moose-mui/src/editor.rs`, `plugins/kontra/plugin/src/{lib,editor}.rs` | 1 | `matari fast kontra -p kontra-plugin` |
| 3 | Add drag coalescing, slot-keyed editor state, incremental `layout::resolve(changes)`, stable MUI memo keys, and Conflux dirty-only `nui::Shown::refresh` | `plugins/kontra/plugin/src/editor.rs`, `plugins/kontra/crates/app/{host/src/view.rs,face/src/nui.rs,ui-view/src/lib.rs}` | 1, 2 | `drag_probe` + `matari fast kontra` |
| 4 | Add KURV-style host-authority lifecycle boundary test suite + `rt-paranoid` verification | `plugins/kontra/plugin/src/tests.rs`, `plugins/kontra/crates/app/host/tests/` | 1, 2, 3 | `matari check origin/main --project plugins/kontra` |

## Rollout & Affected Dependents

- **Affected Projects:** `plugins/kontra` (primary); `frameworks/moose` (`moose-mui` additive `EditorWaker` API — backward-compatible for all other plugins).
- **Mirror Impact:** `plugins/kontra` subtree mirror + `frameworks/moose` Copybara mirror (no private paths or cross-plugin leaks).
- **Versioning & Changelog:** Patch bump on `kontra` (`matari bump kontra patch`) with changelog note: instant event-driven slider/knob response on scripted instruments (Analog Strings, Conflux), zero idle UI polling, and atomic preset/part state transitions.
