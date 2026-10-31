# The live page is flat black: the wallpaper is one picture, too tall for vello's atlas

Written 2026-10-30 on `fix/live-pictures`, rebased onto `feat/integration` `f9a4b3d`
(the sidecar `yesm` fix). Everything here is measured on this machine, headless; no GUI was
opened.

Instrument: `Electric Mint (Melody).nki` — the page the user has (`Vibrato | Bend | Chord |
Spacious EDM | 1 Mint Pop | Bb2 | Open | Fret Pos | Mint Pop A–D | Muted | Flageolet |
Tremolo | Patterns | Guitar Settings`).

## 1. The wallpaper and the title are the same picture

`Resources/pictures/bg-images.png` is one 633 × 7952 strip; the page shows rows 4612–5112 of
it (`offset_y` 4544 under the 68 px header). That band — dumped from the library with
`kontra-window read … Resources/pictures/bg-images.png` and cropped — is **the photograph
with the "SESSION GUITARIST / Electric Mint" title script painted into it**:

    /tmp/opencode/wall-band.png

There is no separate title asset in the tree (129 picture assets, none of them a title). So
"no wallpaper and no title artwork, flat black" is *one* missing picture, and the page's own
background colour is `None` (black).

## 2. Why it is missing in the plugin and present in the harness

| | harness (`tools/ui-shots`, `view::page`, 1 device pixel) | live plugin |
| --- | --- | --- |
| renderer | vello **CPU**, no image atlas | Vulkan, `max_texture_dimension_2d` 16384 |
| window | none | `~/.local/state/MUI/session-1603049-…log`: `scale_override=Some(2.0)`, `device_scale=2` |
| wallpaper asked for | 633 × 7952 | 1266 × **15904** (`at_integer` doubles from 1.5 up) |

vello 0.10 keeps every picture in an image atlas of at most **8192** a side
(`vello_encoding-0.10.0/src/image_cache.rs`: `const MAX_ATLAS_SIZE: i32 = 8192`).
`Resolver::resolve_pending_images` (`resolve.rs:509`) sets the image's position to `None`
when it cannot be allocated, and the encoder writes zero dimensions — the picture is drawn
at no size, silently. 15904 rows cannot be allocated; 7952 can. The photograph and the
title therefore survived in the harness and vanished in the plugin, at the same commit.

The runtime picture path is not the cause: through the editor's own `face()` the resolved
count equals the harness's exactly — **113/114** for `Electric Mint.nki`, **128/129** for
the Melody instrument (the missing one in both is `button_standard.png`, which the library
does not hold). That is pinned by `the_editor_face_reads_the_pictures_the_harness_does`.

## 3. The fix

* `crates/app/ui/src/view.rs`: the wallpaper is drawn as the **band the page shows** — rows
  `offset .. offset + page height` — instead of the whole strip. 633 × 500 here, so it fits
  at any device scale, and a page's decoded pictures fall from 20.3 MB to 1.4 MB.
* `crates/app/ui-pictures`: `Pictures::frame_band` (a `Band` in the picture's own pixels,
  cropped before the resize and before the atlas, part of the frame cache key) and a general
  clamp: a picture that would still be larger than the atlas at the asked scale is decoded at
  the largest scale that fits. `Pictures::tallest()` reports what the renderer was handed;
  the editor writes `picture_too_large` to its session log if it ever exceeds the atlas.

Measured on the Melody page, the editor's own face, at 2 device pixels:

| | tallest picture handed over | wallpaper + title |
| --- | --- | --- |
| before | **15904** | not drawn (over the 8192 atlas) |
| after | 1266 | drawn |

At 1 device pixel nothing changed: both instruments' harness renders (page shot and `--app`
shot) are **pixel-identical** before and after the change.

## 4. Also on this branch (from the first commit)

The library is found from the part's file **or** the instrument it was read from
(`kontra_host::ui::open_any`, `Loader` in `crates/app/host/src/ui/loader.rs`; `picture.rs`
itself is left exactly as `feat/integration` has it, including `f9a4b3d`), and every picture
failure is written to `~/.cache/kontra/logs/session-<pid>-<stamp>-1.jsonl` with its reason
(`NoPath` / `NoLibrary` / `NoFile`), the asset and the instrument. A part whose file sits
outside its library drew **0/114** pictures and now draws **113/114**.

## 5. Tests

* `plugin`: `the_pages_pictures_fit_the_renderers_atlas` — draws the Melody page at 1 and 2
  device pixels and asserts no picture handed to the renderer exceeds `ATLAS`; *fails* at
  15904 without the fix, passes at 633 / 1266.
* `plugin`: `the_editor_face_reads_the_pictures_the_harness_does` — 113/114 and 128/129.
* `plugin`: `a_part_outside_its_library_draws_the_pictures_of_the_instrument_it_came_from` —
  113/114 with the fallback, 0/114 without.
* `kontra-ui-pictures`: `a_band_is_the_rows_it_asks_for`,
  `a_picture_too_tall_for_the_atlas_is_read_at_a_scale_it_holds`.
* `kontra-host` `tests/pictures.rs`: the loader's reasons and the fallback.

## 6. Open

* `Pictures` still remembers a failed read for the life of a face: a library that appears
  later needs the part reloaded. The log now says so.
* `Electric Mint.nki` has the same wallpaper and the same defect; fixed by the same change,
  but only the Melody instrument was counted.
