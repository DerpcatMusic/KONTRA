# Electric Mint's missing photograph: a wallpaper taller than the renderer's atlas

Written 2026-10-30 on `fix/live-pictures` (from `feat/integration` `01a9f08`).
Everything here is measured on this machine, headless. No GUI was opened.

Instrument: `Electric Mint (Melody).nki` (the one the user's page shows: `Vibrato | Bend |
Chord | Spacious EDM | 1 Mint Pop | Bb2 | Open | Fret Pos | Mint Pop A–D | Muted |
Flageolet | Tremolo | Patterns | Guitar Settings`). `Electric Mint.nki` is the other one and
reads `Impact`; both were measured.

## 1. What differs between the harness and the live plugin

| | harness (`tools/ui-shots`, `view::page` at 1 device pixel) | live plugin |
| --- | --- | --- |
| window | none, vello **CPU** | Vulkan, **`scale_override=Some(2.0)`, `device_scale=2`** |
| wallpaper asked for | 633 × 7952 (`bg-images.png`, whole strip) | 1266 × **15904** (the same strip, `at_integer` doubling it) |
| vello image atlas | none (CPU) | `MAX_ATLAS_SIZE = 8192` a side |

`~/.local/state/MUI/session-1603049-…log` records the live window: `logical_size=(1180, 780)
scale_override=Some(2.0)`, `physical_size=(2360, 1560) device_scale=2`,
`max_texture_dimension_2d: 16384`.

vello 0.10 places every picture in an **image atlas of at most 8192 a side**
(`vello_encoding-0.10.0/src/image_cache.rs`, `const MAX_ATLAS_SIZE: i32 = 8192`;
`resolve_pending_images` in `resolve.rs` sets the draw's image position to `None` and the
encoder writes zero dimensions when an image cannot be allocated). A 15904-row picture is
therefore drawn at **no size at all**, silently — the page loses the photograph and shows its
background colour.

At one device pixel the strip is 7952 rows: under 8192, so it draws — which is why the
harness (and the harness's `--app` render) always had the photograph. Nothing about the
library, the tree, the assets or the device was wrong: the page simply asked the renderer
for something it cannot hold.

## 2. What was fixed

* `crates/app/ui/src/view.rs`: the wallpaper is drawn as the **band the page shows** — rows
  `offset .. offset + page height` — instead of the whole strip. That is 633 × 500 for this
  instrument instead of 633 × 7952, so it fits at any device scale, and the decoded bytes for
  a page fall from 20.3 MB to 1.4 MB.
* `crates/app/ui-pictures`: `Pictures::frame_band` (a `Band` in the picture's own pixels,
  cropped before the resize and before the atlas), plus a general clamp: a picture that would
  be larger than the atlas at the asked scale is decoded at the largest scale that fits
  (`fit`). A `Band` is part of the frame cache key.
* `Pictures::tallest()` reports the largest side held (what the renderer was handed), and the
  editor logs `picture_too_large` to its session log if it ever exceeds the atlas again.

Measured, same instrument, the editor's own face, device scale 2:

| | tallest picture handed over | photograph |
| --- | --- | --- |
| before | 15904 | not drawn by the GPU (over the 8192 atlas) |
| after | 1266 | drawn |

At one device pixel nothing changed: both instruments' harness renders are **pixel-identical**
before and after (0 differing bytes in the page shot and in the `--app` shot).

## 3. What was also added (and still applies)

The library is now found from the part's file **or** the instrument it was read from
(`kontra_host::ui::Loader`, `Library::open_any`), and every picture failure is written to
`~/.cache/kontra/logs/session-<pid>-<stamp>-1.jsonl` with its reason (`NoPath`, `NoLibrary`,
`NoFile`), the asset and the instrument — the log v3 did not have before. A part whose file
sits outside its library drew **0/114** pictures and now draws **113/114**.

## 4. Tests

* `plugin`: `the_pages_pictures_fit_the_renderers_atlas` — draws the Melody page at 1 and 2
  device pixels and asserts no picture handed to the renderer exceeds `ATLAS`. Fails before
  the fix (`a 15904 pixel picture at 2 device pixels cannot be drawn`), passes after
  (633 / 1266).
* `plugin`: `the_editor_face_reads_the_pictures_the_harness_does` — 113/114 and 128/129 for
  the two instruments, the same counts the harness route reads.
* `plugin`: `a_part_outside_its_library_draws_the_pictures_of_the_instrument_it_came_from` —
  113/114 through the fallback, 0/114 without it.
* `kontra-ui-pictures`: `a_band_is_the_rows_it_asks_for`,
  `a_picture_too_tall_for_the_atlas_is_read_at_a_scale_it_holds`.
* `kontra-host` `tests/pictures.rs`: the loader's reasons and the fallback.

## 5. Open

* `Pictures` still remembers a failed read for the life of the face: a library that appears
  later needs the part reloaded. The log now says so.
* The main instrument (`Electric Mint.nki`) had the same defect and is fixed by the same
  change; only the Melody instrument was measured for the counts above.
