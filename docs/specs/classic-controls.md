---
status: in-progress
project: plugins/kontra
owner: claude-opus-5-5
created: 2026-11-24
updated: 2026-11-24
proof_scope: internal
evidence_freshness_days: 180
---

# Classic player controls, instant

Branch `kontra-restore-classic-controls`.

## Primary-Source & Version Proof

N/A — internal refactor: UI controls ported from KONTRA's own v0.6.20 code onto MUI; no external API, protocol or SDK assumption.

## Why

Up to v0.6.20 (the monorepo import point, mirror tag `v0.6.20`, commit
`1624f88a0`) KONTRA drew its own sliders in `kit::slider`: a canvas with a
2 px rail, a lit fill from the origin (the middle for a bipolar range), a
centre tick for pan and a 4×10 px block thumb. Pan, the pitch wheel, the mod
wheel, the mixer fader and the master gain all used it. v0.6.21 (Matari #6)
moved `kit::slider` onto MUI's shared slider, and #131 then approximated the
look with `Control::flat()`. The player wants the original controls back.

Both versions eased the drawn value (`ui.tween` in v0.6.20, a 0.12 s spring
in MUI's knob and slider). A control must follow the pointer or the host's
value exactly, so that easing goes.

## Acceptance criteria

1. **Pan:** a part's pan (rack row and mixer strip) is the v0.6.20 classic
   slider: hairline rail, fill from centre, centre tick, block thumb, the
   `C` / `n L` / `n R` readout above it. Dragging emits `Action::Pan(n, -1..1)`;
   a double click returns to C.
2. **Pitch & mod wheels:** the keyboard's two vertical classic sliders
   (TEXT·1.5 × TEXT·6) left of the keybed. The pitch wheel emits
   `Action::Pitch(-1..1)` and snaps back to 0 on release; the mod wheel emits
   `Action::Modulation(0..1)` and stays. The plugin turns them into MIDI 1.0
   pitch bend and CC 1 on the editor port, as if from the host.
3. **Master:** a VLC-style volume wedge in the header: a right-angled
   triangle, rising left to right, filled from the left up to the level. The
   level is amplitude, so the wedge's middle is 100 % (0 dB) and its end is
   200 % (+6 dB), like VLC's 200 % mode. A click or drag sets the level at
   the pointer, the wheel steps it by 5 %, the arrow keys step it, a double
   click returns to 0 dB. The readout shows `nn%` and dB (`-inf dB` at the
   bottom). It drives `Action::Master(dB)` → param `Volume` (−60..+6 dB).
4. **Instant:** the slider, wedge and KONTRA's knobs draw the value of this
   frame: no tween, no spring. Hover/press fades on buttons are not value
   smoothing and stay.
5. **Audio stays click-free:** the engine's parameter ramps (`Volume`'s
   `smooth = "exp(5)"`, the mix gain ramps) are unchanged.
6. **Tests:** each control is found by id in a MUI audit across two sizes;
   driving it by pointer emits the right action; the plugin test maps
   Pitch/Modulation to bend and CC 1. PNG shots of the restored controls are
   attached to the PR.

## Non-goals

- Resurrecting any retired framework or the pre-MUI renderer.
- Changing the engine's smoothing, the mixer's layout or the keyboard.
- A mod/pitch "wheel" drawn as a 3D wheel: v0.6.20 drew them as sliders, and
  that is the look restored.
- Disabling motion that is not a value: layout glides, hover fades.

## Verification Matrix

| Check | Tier | Target / OS | Exact Command | Pass Criterion | Status / Observed Evidence |
|---|---|---|---|---|---|
| Spec proof gate | Doc | any | `python3 tools/check-spec-proof.py plugins/kontra/docs/specs/classic-controls.md` | Exit 0, zero proof errors | PASS (2026-11-24, linux-x86_64) |
| Fast unit/lib tests | Compile/ABI | linux-x86_64 | `matari fast kontra -p kontra-ui -p kontra-plugin` | Exit 0, zero failures | PASS (2026-11-24, linux-x86_64) |
| Full PR verification gate | Validator/Runtime | linux-x86_64 | `matari check origin/main --project plugins/kontra` | fmt, dbg/todo guard, tests, clippy/ratchet green | PENDING |

## Unverified Boundaries & Promotion Gate

- The full PR gate runs in CI (`verify-heavy (plugins/kontra)`); promote to `implemented` once it passes with its run URL.
