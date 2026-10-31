# A self-testing loop for KONTRA

Goal: work on KONTRA — and on Electric Mint specifically — without a human at the
screen, and without being able to fool myself into thinking something works when
it doesn't.

The loop has seven layers. Each one is cheap enough to run on every change; the
expensive ones run before a release. A layer that cannot be run must say so out
loud rather than pass silently.

The numbers are the ones `tools/verify.sh` prints in its section headers, so a row
in its table names the section below. The gate runs layers 0–3; 4–6 are run by
hand, and 4 and 5 are not built yet.

| Layer | The question it answers | Run by |
|---|---|---|
| 0 | does the engine load and render Electric Mint? | `kontra-inspect load\|play` |
| 1 | do the crates' own tests hold? | `cargo test -p <crate>` |
| 2 | does every control hit-test where it is drawn? | `tools/ui-drive` |
| 3 | does the page still look like the reviewed capture? | `tools/pixel-diff.py` |
| 4 | does a press change the frame in a real window? | `tools/ui-shots --press` (not built) |
| 5 | does a press reach the plugin inside a host? | not built |
| 6 | does it sound like Kontakt? | `tools/oracle-mcp/parity.py` |

---

## Layer 0 — the engine, already here

`tools/inspect` (`kontra-inspect load|play`) loads a real instrument and renders
it headless, reporting `actions_unhandled`, `render_complete`, missing sample
frames, clipped samples and an audio fingerprint.

```
kontra-inspect load "Electric Mint.nki"     # what the script asked for and did not get
kontra-inspect play "Electric Mint.nki"     # 504 events, fingerprint, faults
```

This is the loop's spine: every engine change is judged by whether
`actions_unhandled` fell and the fingerprint changed for a reason I can name.
It does not see the UI, the audio output's timbre, or anything a user does.

A `load` that fails, or that prints no `init unanswered:` line at all, fails the
gate's row instead of counting as zero unanswered calls.

---

## Layer 1 — the crates' own tests

`tools/verify.sh` runs `cargo test` for every crate between the page and the
shipped plugin — engine, host, ui, mui, the native host, the plugin, the picture
renderer, the delay DSP — before it builds anything. A crate that fails to
compile prints no `test result:` line, so a missing line is a failure, not a pass:
that is how a broken test file once reported `ok`.

---

## Layer 2 — the UI's hit-test, headless

The reported bug — "clicks sometimes land" — lives in `crates/app/ui/src/input.rs`:
`read()` walks the visible widgets and asks mui for each one's rect, with a
`scale` argument. If the rects were recorded in a different coordinate space from
the pointer, the hit test is off and clicks land on the wrong control or on
nothing.

**Built: `tools/ui-drive`.** No X server and no host; a run against Electric Mint
takes about 35 s on this machine, nearly all of it loading the library. It draws a
real page into mui's offscreen scene, takes each visible input-taking widget's rect
from what mui recorded when it drew it, presses the rect's centre through the
same reader the editor uses (`input::read` with `Input`/`PointerInput`), and
checks that the press selected that widget and no other. Then it presses two
points outside the rect, which must select nothing — otherwise a hit test that
selects everything would pass. Controls the page paints under another one are
reported as `covered` and named: no click can reach them in Kontakt either.

```
cargo run --release -p ui-drive -- [--scale <f>] [--out <dir>] <instrument>...
```

Last run, Electric Mint at 1×: **39 controls checked on 1 page, 0 skipped, 35
answered with an `Edit`, 2 painted under another control, every one `ok`.**

A run that checks nothing is a failure, not a pass. When every widget lacks a
drawn rect, or the instrument has no page at all, `ui-drive` prints `FAIL: no
control was checked … this run proves nothing` and exits non-zero, and the gate's
`ui-drive` row fails with it. The gate also fails when `ui-drive` does not build:
it is a workspace member, so a compile break is a defect, and calling it
"skipped" is how layer 2 could go unrun while the gate still said PASSED.

**Acceptance:** a widget whose centre press does not select it is a failure
naming the widget, the scale, and the point.

---

## Layer 3 — the page still looks like the reviewed capture

A geometry check reads the rects mui recorded, so it cannot see a page that drew
the right rect with nothing in it — which is how a missing graphic shipped once.
`tools/pixel-diff.py REF GOT --scale --max-pct 0.5` compares a render against a
stored capture and reports the regions that differ, largest first, with their
size, position and mean colour on both sides. It exits 0 within tolerance, 1 over
the ceiling, and 2 when the two cannot be compared at all (missing, unreadable, or
a size mismatch without `--scale`). The gate fails on 1 and on 2, and prints
pixel-diff's own message for a 2 instead of an empty reason.

**This is not a comparison with Kontakt.** `tools/kontakt-reference/*-page.png` is
the reviewed page as this tree draws it: the check catches a page that changed,
not a page that was wrong to begin with. The live comparison is layer 6's
instrument, run on demand.

**Acceptance:** the render differs from the capture by less than `MAX_PIXEL_PCT`
(0.5%) of the page. `ALLOW_NO_PIXEL_REF=1` turns the check off and the table says
`OFF` rather than `ok`: an admission, not a pass.

---

## Layer 4 — the real window, headless

`tools/ui-shots` already draws a page headless and can `--press <control>`. Extend
it to:

- run inside a private Xvfb (the `tools/kontakt-oracle/sandbox.sh` pattern), so it
  never touches the user's desktop;
- press a named control by *drawn geometry*, screenshot before and after, and
  compare the two frames — a press that changes nothing is a failure;
- report the settled frame time and paint cost, so a slow UI is visible.

**Acceptance:** `ui-shots --press vibrato` changes at least one pixel inside the
control's rect and leaves the rest of the page within a small delta.

---

## Layer 5 — the plugin in a host

Load the built `libkontra_plugin.so` as a CLAP in a minimal headless host (the
`clap_entry` is already exported), instantiate, load Electric Mint, and drive it
through the same gesture path as layer 2. Assert on the host's state dump, not on
pixels: a click on Vibrato must appear in the saved state.

This is the only layer that catches a bug in the wrapper between mui's window and
the host — which is exactly where a "clicks don't land in REAPER" bug can hide.

**Acceptance:** a click at a control's centre, delivered through the host's event
path, changes that control's saved value.

---

## Layer 6 — does it sound like Kontakt

The native oracle now runs on this machine (Kontakt 8.13.1 at
`~/Armory/VSTs/kontakt-8.13.1-re/Kontakt 8.exe`, `tools/kontakt-oracle/dump.py`
and `render.py`, `tools/re/kemu_timemod.py`). The loop's last layer renders the
same performance through Kontakt and through KONTRA and compares:

- per-band energy over time (the existing `octave_bands` metric in `play`);
- the fingerprint, which must change only when I intend it to.

**Acceptance:** for the default Electric Mint scenario, the two renders agree in
every octave band within a tolerance set once, from a run where I know the answer.

---

## What "Electric Mint works" means, in order

1. It loads and plays: `render_complete`, no missing frames, no clipping.
   **Met** — the gate's `render completes`, `missing sample frames` and
   `clipped samples` rows, measured on every run.
2. The script gets what it asks for: `actions_unhandled` at or below its ceiling.
   **Met as a ceiling, not as zero** — `MAX_UNHANDLED=55` in `tools/verify.sh`,
   which fails a regression above it. The last gate run measured 55 (the
   remaining calls are the insert effects and the volume CC), and 51 init
   unanswered against `MAX_INIT_UNANSWERED=51`.
3. The UI responds: every control hit-tests where it is drawn. **Met headless** —
   `tools/ui-drive`, 39 controls on Electric Mint's page, 0 failures (layer 2).
   The real window (layer 4) and the plugin in a host (layer 5) are not built, so
   "a click lands in REAPER" is still unproven.
4. It sounds like Kontakt: the four insert effects and the volume automation.
   **Open** — layer 6.

Layer 0 protects (1) and (2), layer 1 protects the code under all of them, layer 2
protects (3), layer 3 protects the page from changing under me, layers 4 and 5
would extend (3) to the window and the host, and layer 6 protects (4).

---

## Build isolation (learned the hard way)

Four worktrees sharing one `CARGO_TARGET_DIR` can link each other's rlibs when their sources
diverge: a lane here saw `kontra-ui-model` from another branch (`Kind::LevelMeter` with a `bus`
field) and `kontra-ir` without `Mod::invert`. A lane can then report a pass that belongs to a
different tree. **Every lane gets its own target directory, and the integration tree is built
clean** before `tools/verify.sh` is trusted.

## Order of work

1. Layer 4, so a press can be shown to change a frame in a real window.
2. Layer 5 when a host harness is worth its cost — it is the only defence against
   wrapper bugs.
3. Layer 6 alongside the effects work, since it is what tells me an effect is
   right rather than merely present.
