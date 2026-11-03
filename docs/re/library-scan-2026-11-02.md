# The library scan: a library inside a library, and what "hidden" actually counted

Written 2026-11-02, porting the scan rule onto v3 and re-measuring the claim that
sent it. `docs/re/gap-inventory-2026-11-01.md` §1 is the original claim; this file
says what holds and what does not.

## What the scan did

`crates/app/library::scan` walks each root a few levels down. A folder is a
library when it holds a `.nicnt`, an `Instruments`/`Multis` folder, presets beside
a `Samples` folder or a monolith, or — below the root — any presets at all. The
first folder that qualified **ended the search below it** (`visit` returned).

That last part is the defect, and it is narrower than §1 says: a *registered*
product can sit inside a registered bundle, and the inner one was never listed as
its own library. Its presets were folded into the outer entry's walk, so the
instrument still loaded — it appeared under the wrong library, twice if the bundle
ships two copies.

Examples on this machine's library root (`~/Armory/VSTs/Kontakt1`):

| bundle | before | after |
|---|---|---|
| `Action Strings 2 v1.2.0` (a `.nicnt` folder holding `Action Strings 2 Library`, another `.nicnt`) | one `Action Strings 2`, 448 presets | `Action Strings 2` 239 + `Action Strings 2 2` 209 |
| `Guarneri Violin` (same shape) | one, 24 presets | two, 12 each |

Measured over the whole root with
`cargo run --release -p kontra-host --example library -- ~/Armory/VSTs/Kontakt1`:

| | before | after |
|---|---:|---:|
| libraries | 145 | **147** |
| presets, all libraries | 19,496 | **19,496** |
| presets listed twice | 2 (the two cases above) | **0** |

## What §1's own numbers counted

§1 reports "18 libraries hold presets at one level and more in a subfolder … 563
instruments the user cannot reach", with per-library pairs such as Kinetic Metal
100/111 and VSTBuzz Power Move 160/27. Those numbers do not reproduce on v3:

- Kinetic Metal's `Instruments/` holds 100 `.nki` at its top level and 111 under
  `Instruments/ Motion Enabled/`, 211 in total; the scan lists **211**.
- Power Move's `Extra Presets/` holds 27 of its 187 `.nki`; the scan lists **187**
  (223 presets with its 36 multis).
- The browser does not flatten presets into a list either: it groups them by
  folder and draws a collapsible row per folder
  (`crates/app/ui/src/app/browser.rs`), so a subfolder is a row, not a hidden
  instrument.

The pair of numbers in each row is exactly "presets in the folder itself" and
"presets in its subfolders" — 100 and 111, 160 and 27 — which is a count of where
presets *sit*, not a count of what the catalog omits. The instruments were never
hidden by the scan; §1's percentages therefore overstate the defect.

## The rule now, and the one it must not become

`visit` no longer returns at a library: it records it and keeps descending, and
`catalog`'s walk **prunes** a nested library's folder so each preset is listed
under exactly one library.

Inside a library, only a `.nicnt` makes a subfolder a library of its own. That is
deliberate and was measured the hard way: extending the below-the-root rule
("any folder of presets is a library") into a library's subfolders promotes every
instrument folder that keeps its samples beside its presets. Evolve R2's
`Instruments/4 Tonality and FX` is exactly that shape, and it became a library
called "Tonality and FX" that took 99 instruments out of Evolve R2 (1323 → 1224 at
149 libraries, with new entries named `Instruments`, `Tonality …` and `Surround
Instruments`). With the `.nicnt` rule Evolve R2 is back to 1323, there are 147
libraries, and the only new entries are the two registered products above.

Held by `crates/app/library/tests/scan.rs`:
`a_library_inside_a_library_is_a_library_of_its_own` and
`a_preset_folder_inside_a_library_stays_inside_it`.
