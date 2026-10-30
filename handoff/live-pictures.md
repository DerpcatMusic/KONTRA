# Electric Mint's live page: the pictures, the labels, and what was actually wrong

Written 2026-10-30 on `fix/live-pictures` (from `feat/integration` `01a9f08`).
Everything below is measured on this machine, headless. No GUI was opened.

## 1. What was reproduced

| route | instrument | pictures read |
| --- | --- | --- |
| the harness (`tools/ui-shots`' path: `Instrument` → `Live::new` → `Library::open`) | `Electric Mint.nki` | **113 / 114** |
| the plugin's own face (`Snapshot::fill` → `editor::face` → `Pictures`) | `Electric Mint.nki` | **113 / 114** |
| the harness | `Electric Mint (Melody).nki` | **128 / 129** |
| the plugin's own face | `Electric Mint (Melody).nki` | **128 / 129** |

The one asset neither route reads is `Resources/pictures/button_standard.png`, which the
library does not hold at all (logged reason: `the library has no file at that path`).

Drawn headless through `kontra_ui::app::view` — the part view the user actually sees — the
page is complete, photograph and title included:
`/tmp/opencode/livepics-shots/Electric_Mint-0-app.png`.

So at this commit the plugin's own picture path is **not** dropping pictures for either
instrument, and `Part::path` is `Some` on every load path (`Rack::load`, `load_saved`,
`recall`). What the live page did was not reproducible from the code, the instrument or the
library.

## 2. The live page is the *other* instrument (`Bend`, `All…`)

The two labels reported as broken are exactly and only the **Melody** instrument's:

* `Electric Mint.nki` `$kif3y` = `"Impact"`, `$kvref` = `"PitchW."` (the `0-ElMint.ksp` dump
  the report quotes is this instrument).
* `Electric Mint (Melody).nki` `$kif3y` = **`"Bend"`**, `$kvref` = `"PitchW."`, and
  `$0k002` is a bend-mode menu whose first item is `"All             = Bend all notes"`,
  which the page draws truncated as **`All…`** (the same ellipsis the reference page uses
  for `Med…`).

The harness render of that instrument shows both strings, with the wallpaper:
`/tmp/opencode/livepics-shots/Electric_Mint_(Melody)-0.png`. There is no label defect: the
label text is the script's `set_text`, and the scripts of the two instruments differ.

## 3. What *is* wrong, and what was fixed

Any picture failure was silent and indistinguishable from "the page has no pictures":
`face()` chained `path.as_deref()?`, `library.lock().ok()?` and `Library::open(..).read(..)`,
and `Pictures` caches a failed read for the life of the face. A part whose file is not
inside its library — a preset kept elsewhere, a library that moved, no path at all — drew a
page of stock rectangles and said nothing, in any log.

Fixed in one place: `kontra_host::ui::Loader` is now the only thing that turns a tree asset
path into bytes. It opens the library of the first of (the part's file, the instrument the
part was read from) that has resources, and reports every failure with a reason —
`NoPath`, `NoLibrary` (nothing at the library root: the file is outside its library),
`NoFile`. The plugin writes those to its session log.

Measured before/after, same test, part loaded from a file path outside any library
(`Rack::load_ir` with the real instrument's IR and a path in `/tmp`):

* before: **0 / 114** pictures
* after: **113 / 114**

That is the user-visible failure reproduced, and the page it repairs is the "flat page,
stock shapes" one.

## 4. The session log

`~/.cache/kontra/logs/session-<pid>-<stamp>-1.jsonl`, one JSON object per line, written and
flushed as it happens; `KONTRA_LOG_DIR` moves it, `KONTRA_LOG=0` turns it off; a file stops
at 4 MiB (`log_full`) and the newest 16 sessions are kept. v2 wrote this path; v3 (this
tree) wrote nothing, so the log had to be added. Sample from a live run:

```json
{"level":"info","event":"session_started","log":".../session-1731672-...jsonl","build":"Version 3.0.0-dev · 01a9f0803","pid":"1731672"}
{"level":"error","event":"picture_missing","reason":"the library has no file at that path","asset":"Resources/pictures/button_standard.png","instrument":"/tmp/kontra-nowhere/Electric Mint.nki","count":"1"}
```

The first failure of a face is logged, then one line per 64 with the running count.

## 5. Open

* Why the live VST3 showed no picture at all is still not established. The candidates the
  code allows are a part whose path is not inside its library (now repaired when the
  instrument's own source resolves, and always logged) and a build that predates this
  picture path. The next occurrence names the instrument path and the reason.
* `Pictures` still remembers a failed read for the life of the face: a library that appears
  later needs the part reloaded. The log now says so, which it could not before.
