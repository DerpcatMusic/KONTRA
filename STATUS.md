# STATUS

The single place to look for where KONTRA stands. If this file and an agent
disagree about the state, this file wins — and if it is out of date, that is a
bug worth fixing before anything else.

Last updated: 2026-10-31.

## What is verified, and how

Everything below was produced by a command, not by a claim. The command is
given so it can be re-run rather than believed.

| Claim | Evidence |
|---|---|
| The workspace builds and every test suite passes | `tools/verify.sh` — engine, host, ui, mui, native-host, plugin, ui-pictures and dsp-delay suites, 0 failures (the counts in the gate's own table are the authority; they grow) |
| The plugin a host loads is the one that was tested | gate builds `kontra-plugin`, asserts `clap_entry` is exported, and installs to **both** `~/.clap` and `~/.vst3` |
| Electric Mint plays end to end | gate render: completes, 0 missing frames, 0 clipped, `actions_unhandled` 55, `init unanswered` 51 |
| The rendered audio is reproducible | fingerprint `170201c38ecb94f1` (stable run to run) |
| The page hit-tests where it is drawn | `tools/ui-drive` — 39 controls checked, every press lands on the rect it was drawn in, a press 2 points outside selects nothing |
| The page looks like the reviewed page | `tools/pixel-diff.py` against `tools/kontakt-reference/electric-mint-page.png` — 0 of 316500 px differ |
| The page fits the renderer's image atlas at the live device scale | test asserts the page's pictures fit at 1 **and** 2 device pixels (the 2× case is what shipped blank once) |
| Real Kontakt can be driven headless | `tools/oracle-mcp` — Kontakt 8.14.1 on Xvfb with DXVK + lavapipe paints its instrument page and takes pointer input (`paints=yes click_px=44485`) |
| KONTRA's Rust meets its own lint bar | 48 opted-in crates, `clippy --all-targets --no-deps -- -D warnings` → 0 diagnostics, 0 `#[allow]`/`#[expect]` added |

## What is implemented

- **Formats**: Kontakt `.nki` (script, zones, effects, UI), UVI/Falcon, SFZ,
  native. Picture containers (`.nkr`/`.nkx`) with sidecar metadata.
- **KSP**: the script VM, the UI/control model, `mf_*` MIDI object, string-array
  control parameters, `$KSP_TIMER` on the CPU clock.
- **DSP**: filters, dynamics, drive, delay (Modern + Choral banks), reverb,
  spatial, modulators; Hot Solo+ amp measured at Electric Mint's operating point.
- **Host/plugin**: CLAP + VST3, a rack with parts, a browser, an editor, and a
  one-slot ring so a control edit applies even when the host renders no audio.

## What is open, and what would close it

| Gap | Why it is open | What closes it |
|---|---|---|
| **Choral `process`** | banks, LFO shape, rate/depth/width laws measured; per-voice smoothing networks, the rotation matrices' per-frame use, the seven-tap mix and the global tone filter are unread | read `−0x1f8`/`−0x200`/`−0x1e0`, `0x141939270`, `core+0x1c20..0x1cd0`, then implement against the `kemu_timemod.py` vector |
| **Replika Diffusion mode** | Electric Mint's record is mode 4; parsing it as Modern would be invented DSP | model the diffusion network from the binary; the reader and mode table are already located |
| **Hot Solo+ is one operating point** | the module's coefficient recalculation (`0x140ad5440`, 14.5 KB of SIMD) is unread, so the stage is a *measured transfer function* fitted as biquads | read the recalculation; until then any record that differs from Electric Mint's is **reported** as unmodelled, never silently drawn with the same curve |
| **Hot Solo+ tone knobs, overdrive, mono** | same | the `.nki` knob sweep is possible (`preset_patch.rs` rewrites a copy) but needs an audio-capable Kontakt session |
| **NKS2 navigation, MIDI export areas** | the KSP manual has no entry for these commands and there is nothing in KONTRA for the values to land in | the NKS2 SDK, or a decision to model a page/slot tree |
| **Bus slot 7 phantom filter** | the instrument stores it `bypass: false`, but Kontakt behaves as if it is not running; removing it closes 11.8 dB at 12.9 kHz | wrong to decide by assumption. The decisive test needs the bypass toggled at runtime via KSP in an audio-capable session — editing the `.nki` is refused because Kontakt validates an MD5 in `BNISoundHeader` |
| **A control edit can cost one silent block** | the price of having no lock on the audio path: if the host resumes while the control tick holds the audio side, that block renders silence | accepted trade-off; counted and logged as `block_missed` |

## What is *not* verified — do not cite these as facts

- **No live Kontakt pixel comparison is in the gate.** The pixel check is a
  *regression golden* of our own render. `tools/oracle-mcp` can screenshot real
  Kontakt, and that is the on-demand check, but nothing fails a build on it.
- **Parity is measured on one note of one instrument.** No library-wide sweep,
  no articulation matrix, no regression history.
- **`wave_r` is not a parity statistic** for a single note: Kontakt correlates
  0.604 against itself within a session.
- **Click coordinates in the oracle are fixed**, not resolution-independent.
- **`dump.py` does not run as shipped** — its default EXE path does not exist on
  this machine and its `WINEDLLOVERRIDES=…=b` override is the wined3d assumption
  the oracle disproved.

## How to check the tree

```
tools/verify.sh          # the delivery gate; refuses to pass on any of the above
tools/verify.sh --quick  # everything except the audio render
```

`VERIFY PASSED` means: tests pass, the plugin builds and exports a CLAP entry,
Electric Mint renders completely with no dropped frames or clipping, the UI
hit-tests where it draws, and the page matches its golden. It does **not** mean
the instrument matches Kontakt — that is the oracle's job, and it is on demand.

## Where the work is

The change set is a stack of four PRs against `DerpcatMusic/KONTRA`:
#1275 (UI), #1276 (audio), #1277 (engine), #1278 (tooling). Each is based on the
previous; the tip is tree-identical to the branch they were cut from.
