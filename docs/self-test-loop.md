# A self-testing loop for KONTRA

Goal: work on KONTRA — and on Electric Mint specifically — without a human at the
screen, and without being able to fool myself into thinking something works when
it doesn't.

The loop has four layers. Each one is cheap enough to run on every change; the
expensive ones run before a release. A layer that cannot be run must say so out
loud rather than pass silently.

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

---

## Layer 1 — the UI's hit-test, headless (build this first)

The reported bug — "clicks sometimes land" — lives in `crates/app/ui/src/input.rs`:
`read()` walks the visible widgets and asks mui for each one's rect, with a
`scale` argument. If the rects were recorded in a different coordinate space from
the pointer, the hit test is off and clicks land on the wrong control or on
nothing.

**Test:** lay out a real page (Electric Mint's Interface, the Rack, the browser)
at scale 1.0, 1.5 and 2.0, and for every visible input-taking widget, simulate a
press at its drawn centre and assert `read()` returns an `Edit` for *that*
widget. Then press just outside its edge and assert it returns nothing for it.

This is a pure unit test: no X server, no host, milliseconds. It turns "clicks
sometimes land" into a named failure with a coordinate.

**Acceptance:** a widget whose centre press does not select it is a test failure
naming the widget, the scale, and the point.

---

## Layer 2 — the real window, headless

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

## Layer 3 — the plugin in a host

Load the built `libkontra_plugin.so` as a CLAP in a minimal headless host (the
`clap_entry` is already exported), instantiate, load Electric Mint, and drive it
through the same gesture path as layer 2. Assert on the host's state dump, not on
pixels: a click on Vibrato must appear in the saved state.

This is the only layer that catches a bug in the wrapper between mui's window and
the host — which is exactly where a "clicks don't land in REAPER" bug can hide.

**Acceptance:** a click at a control's centre, delivered through the host's event
path, changes that control's saved value.

---

## Layer 4 — does it sound like Kontakt

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

1. It loads and plays: `render_complete`, no missing frames, no clipping. **Met.**
2. The script gets what it asks for: `actions_unhandled` → 0. Currently 173, all
   effects and the volume CC.
3. The UI responds: every control hit-tests where it is drawn, at every scale.
   **Not met** — this is the current bug.
4. It sounds like Kontakt: the four insert effects and the volume automation.

Layers 1–3 protect (3); layer 0 protects (1) and (2); layer 4 protects (4).

---

## Build isolation (learned the hard way)

Four worktrees sharing one `CARGO_TARGET_DIR` can link each other's rlibs when their sources
diverge: a lane here saw `kontra-ui-model` from another branch (`Kind::LevelMeter` with a `bus`
field) and `kontra-ir` without `Mod::invert`. A lane can then report a pass that belongs to a
different tree. **Every lane gets its own target directory, and the integration tree is built
clean** before `tools/verify.sh` is trusted.

## Order of work

1. Layer 1 for the UI: the hit-test unit test, at three scales. It is small, it
   targets the live bug, and it needs nothing but the existing tree.
2. Fix whatever it finds; re-run.
3. Layer 2, so a press can be shown to change a frame.
4. Layer 3 when a host harness is worth its cost — it is the only defence against
   wrapper bugs.
5. Layer 4 alongside the effects work, since it is what tells me an effect is
   right rather than merely present.
