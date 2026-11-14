//! A file selector (`ui_file_selector`) as Kontakt 8 draws and plays one
//! (oracle probes, 2026-11-14, scratch/lanes/ui/probe/fs2.ksp to fs4.ksp at
//! 100 %).
//!
//! A column view: the base folder's folders, then its files, each in its
//! own column, and the folder a row picks opening in the next column to the
//! right. A column is `COLUMN_WIDTH` + 2 points wide (150 when unset); its
//! rows are 16 points and a 1 point #A6A69E line, from the control's top,
//! over the page (no field). A row is a 12 point icon 4 in, the name at 20
//! in #FEFEFE, and, on a folder, a ▸ at the right. The selected row of
//! each column fills #F6F6F0. A column with more rows than fit has a 12
//! point scrollbar at its right; columns wider together than the control
//! scroll sideways to the deepest one, over a 15 point bar at the bottom.
//!
//! A click selects (a folder opens its column); a double-click or Return
//! on a file picks it, which runs the callback; Up and Down move the
//! selection. The wheel does not scroll the list in Kontakt; here it does.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use kontra_ui_model as model;
use mui::prelude::*;

/// A row's pitch, in points: 16 and its line.
pub const ROW: f64 = 17.;
/// The vertical scrollbar's width.
pub const BAR: f64 = 12.;
/// The sideways bar's height.
pub const HBAR: f64 = 15.;
/// A column's width when the script sets none.
const COLUMN: f64 = 150.;

/// One folder or file a column lists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub folder: bool,
}

/// One open column: its folder, what it lists, and the row it selects.
#[derive(Clone, Debug)]
pub struct Column {
    pub dir: PathBuf,
    pub entries: Arc<[Entry]>,
    pub picked: Option<usize>,
}

/// The columns a selector at `base` shows with `path` selected: the base
/// folder, then each folder on the way to `path`.
#[must_use]
pub fn columns(base: &str, path: &str, files: model::Files) -> Vec<Column> {
    let mut dir = PathBuf::from(base);
    let rest = Path::new(path)
        .strip_prefix(&dir)
        .ok()
        .map(Path::to_path_buf);
    let mut parts = rest.iter().flat_map(|r| r.components()).peekable();
    let mut out = Vec::new();
    loop {
        let entries = list(&dir, files);
        let next = parts
            .next()
            .map(|c| c.as_os_str().to_string_lossy().into_owned());
        let picked = next.and_then(|n| entries.iter().position(|e| e.name == n));
        let folder = picked.and_then(|i| entries.get(i)).filter(|e| e.folder);
        let open = folder.map(|e| dir.join(&e.name));
        out.push(Column {
            dir,
            entries,
            picked,
        });
        match open {
            Some(d) if out.len() < 32 => dir = d,
            _ => return out,
        }
    }
}

/// The extensions `files` lists.
fn shown(files: model::Files) -> &'static [&'static str] {
    match files {
        model::Files::Any | model::Files::Midi => &["mid", "midi"],
        model::Files::Audio => &["wav", "aif", "aiff", "ncw", "flac", "ogg"],
        model::Files::Data => &["nka"],
    }
}

/// `dir`'s folders then its `files`, each by name, case aside; read once
/// per change of the folder.
fn list(dir: &Path, files: model::Files) -> Arc<[Entry]> {
    type Cache = HashMap<(PathBuf, u8), (Option<SystemTime>, Arc<[Entry]>)>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let stamp = std::fs::metadata(dir).and_then(|m| m.modified()).ok();
    let key = (dir.to_path_buf(), files as u8);
    if let Ok(cache) = CACHE.lock()
        && let Some((at, entries)) = cache.as_ref().and_then(|c| c.get(&key))
        && *at == stamp
    {
        return entries.clone();
    }
    let ext = shown(files);
    let mut entries: Vec<Entry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let folder = e.file_type().ok()?.is_dir();
            let wanted = folder
                || Path::new(&name)
                    .extension()
                    .is_some_and(|x| ext.iter().any(|w| x.eq_ignore_ascii_case(w)));
            (wanted && !name.starts_with('.')).then_some(Entry { name, folder })
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.folder, e.name.to_lowercase()));
    let entries: Arc<[Entry]> = entries.into();
    if let Ok(mut cache) = CACHE.lock() {
        // ponytail: never evicts; a selector browsing thousands of folders
        // keeps their listings until the plugin closes.
        cache
            .get_or_insert_with(HashMap::new)
            .insert(key, (stamp, entries.clone()));
    }
    entries
}

/// Where the columns sit in a control `size` points big.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// One column's width, its bar or edge included.
    pub pitch: f64,
    /// How far the columns are scrolled sideways.
    pub shift: f64,
    /// Whether the sideways bar shows, and the rows that fit above it.
    pub sideways: bool,
    pub rows: usize,
}

impl Layout {
    #[must_use]
    pub fn of(cols: &[Column], column_width: Option<u32>, (w, h): (f64, f64)) -> Self {
        let pitch = column_width.map_or(COLUMN, f64::from) + 2.;
        let total = pitch * cols.len() as f64;
        let sideways = total > w + 0.5;
        let room = if sideways { h - HBAR } else { h };
        Self {
            pitch,
            shift: (total - w).max(0.),
            sideways,
            // Within half a point: a snapped rect may lose a hair.
            rows: ((room + 0.5) / ROW).floor().max(0.) as usize,
        }
    }

    /// Whether column `c` (of `len` rows) has its scrollbar.
    #[must_use]
    pub fn barred(&self, len: usize) -> bool {
        len > self.rows
    }

    /// The column and row index under (`x`, `y`) points, `tops` its first rows.
    #[must_use]
    pub fn hit(&self, cols: &[Column], tops: &[u32], (x, y): (f64, f64)) -> Option<(usize, usize)> {
        if x < 0. || y < 0. || (y / ROW).floor() as usize >= self.rows {
            return None;
        }
        let c = ((x + self.shift) / self.pitch).floor() as usize;
        let col = cols.get(c)?;
        let list = self.pitch
            - if self.barred(col.entries.len()) {
                BAR
            } else {
                0.
            };
        if x + self.shift - c as f64 * self.pitch >= list {
            return None;
        }
        let top = tops.get(c).copied().unwrap_or(0) as usize;
        let i = top + (y / ROW).floor() as usize;
        (i < col.entries.len()).then_some((c, i))
    }

    /// The first row a column of `len` rows may show.
    #[must_use]
    pub fn last_top(&self, len: usize) -> u32 {
        u32::try_from(len.saturating_sub(self.rows)).unwrap_or(u32::MAX)
    }
}

/// The selector drawn `width` by `height` points at scale `s`.
pub(super) fn draw(
    cols: &[Column],
    files: model::Files,
    tops: &[u32],
    lay: Layout,
    (width, height, s): (f64, f64, f64),
) -> El {
    let px = |v: f64| (v * s).round();
    let fill = |x: f64, y: f64, w: f64, h: f64, c: u32| {
        block(px(x + w) - px(x), px(y + h) - px(y))
            .radius(0.)
            .fill(super::color(model::Rgba::rgb(c)))
            .at(px(x), px(y))
    };
    let mut parts = Vec::new();
    for (c, col) in cols.iter().enumerate() {
        let x0 = c as f64 * lay.pitch - lay.shift;
        let len = col.entries.len();
        let barred = lay.barred(len);
        let list = lay.pitch - if barred { BAR } else { 0. };
        let top = tops.get(c).copied().unwrap_or(0) as usize;
        for r in 0..lay.rows {
            let y = r as f64 * ROW;
            let at = top + r;
            if col.picked == Some(at) {
                parts.push(fill(x0, y, list, ROW - 1., 0xF6_F6F0));
            }
            parts.push(fill(x0, y + ROW - 1., list, 1., 0xA6_A69E));
            let Some(e) = col.entries.get(at) else {
                continue;
            };
            parts.push(icon(e.folder, files, (x0 + 4., y + 2.), &fill));
            parts.push(
                text(e.name.clone())
                    .text_size((super::TEXT - 1.) * s)
                    .fill(Fill::from(super::color(model::Rgba::rgb(0xFE_FEFE))))
                    .lines(1)
                    .w((list - 24.).max(0.) * s)
                    .h((ROW - 1.) * s)
                    .at(px(x0 + 20.), px(y)),
            );
            if e.folder {
                // ▸ at the right, 3 points across.
                for k in 0..3 {
                    let k = f64::from(k);
                    parts.push(fill(
                        x0 + list - 9. + k,
                        y + 5. + k,
                        1.,
                        5. - 2. * k,
                        0xF6_F6F0,
                    ));
                }
            }
        }
        if barred {
            let bx = x0 + list;
            let h = lay.rows as f64 * ROW;
            parts.push(fill(bx - 1., 0., 1., h, 0xBF_BEB5));
            parts.push(fill(bx + BAR - 1., 0., 1., h, 0x84_847D));
            parts.push(fill(bx, h - 1., BAR - 1., 1., 0xBF_BEB5));
            let t = h * top as f64 / len as f64;
            let th = (h * lay.rows as f64 / len as f64).max(8.);
            parts.push(fill(bx - 1., t, BAR + 1., th, 0x98_9790));
            parts.push(fill(bx, t + 1., BAR - 1., th - 2., 0xF3_F2EB));
            parts.push(fill(bx + BAR - 1., t, 1., th, 0x69_6963));
            parts.push(fill(bx, t + th - 1., BAR - 1., 1., 0xBF_BEB5));
        } else {
            parts.push(fill(
                x0 + list - 1.,
                0.,
                1.,
                lay.rows as f64 * ROW,
                0xD2_D2CE,
            ));
        }
    }
    if lay.sideways {
        let y = height - HBAR;
        let total = lay.pitch * cols.len() as f64;
        parts.push(fill(0., y, width, HBAR, 0x2F_2F2F));
        let tw = width * width / total;
        parts.push(fill(width * lay.shift / total, y, tw, HBAR, 0x6C_6C6C));
    }
    stack(parts).w(width * s).h(height * s).clip()
}

/// A row's 12 point icon: a folder's outline; a file's light square with
/// a note (MIDI) or a waveform (the others, and a selector without a type).
fn icon(
    folder: bool,
    files: model::Files,
    (x, y): (f64, f64),
    fill: &dyn Fn(f64, f64, f64, f64, u32) -> El,
) -> El {
    let mut p = Vec::new();
    if folder {
        for (fx, fy, fw, fh) in [
            (0., 1., 12., 1.),
            (0., 10., 12., 1.),
            (0., 1., 1., 10.),
            (11., 1., 1., 10.),
        ] {
            p.push(fill(x + fx, y + fy, fw, fh, 0xF6_F6F0));
        }
        return stack(p);
    }
    p.push(fill(x, y, 12., 12., 0xF8_F7F3));
    let ink = 0x0C_0C0C;
    if files == model::Files::Midi {
        // Two stems under a beam, each on its head.
        p.extend([
            fill(x + 4., y + 2., 6., 1., ink),
            fill(x + 4., y + 2., 1., 7., ink),
            fill(x + 9., y + 2., 1., 6., ink),
            fill(x + 2., y + 8., 3., 2., ink),
            fill(x + 7., y + 7., 3., 2., ink),
        ]);
    } else {
        p.push(fill(x + 1., y + 6., 10., 1., ink));
        for (k, h) in [(2., 4.), (4., 8.), (6., 6.), (8., 8.), (10., 4.)] {
            p.push(fill(x + k, y + 6. - h / 2., 1., h, ink));
        }
    }
    stack(p)
}
