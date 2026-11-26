//! The editor: [`kontra_ui::app::view`] in a moose window. It draws from a
//! [`Snapshot`] of the rack that [`crate::Tick`] refreshes, so a frame never
//! waits on a load, and it hands what the user does back as [`Edit`]s for
//! the tick to carry out.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use kontra_host::browser::Browser;
use kontra_host::rack::{Event, HostNote, HostPattern, Mix, Rack};
use kontra_host::state::SavedView;
use kontra_host::ui::Library;
use kontra_protocol::Stats;
use kontra_ui::app::{self, Action, App, Face, Part};
use kontra_ui::input::{self, Set};
use kontra_ui::{Pictures, layout};
use kontra_ui_model::Interface;
use moose::mui::MuiEditor;
use moose::mui::window::{WheelAcceleration, WheelPolicy};
use moose::prelude::*;

use crate::readout::Readout;
use crate::{KontraParams, P, Request, lock};

/// Report lines kept; older ones scroll away.
const REPORT: usize = 200;
/// The most scan issues the report carries.
const SCAN_ISSUES: usize = 50;

/// One rack part as the editor shows it.
#[derive(Clone, Debug)]
struct PartView {
    slot: usize,
    /// Which load of the part this is.
    generation: u64,
    path: Option<PathBuf>,
    /// The instrument file its pictures are found near: a user preset's
    /// base, not the preset in KONTRA's own folder.
    source: Option<PathBuf>,
    /// Its library's interface at `revision`; copied only on load or
    /// structural fallback, updated in-place from [`TreePatch`]es on drags.
    tree: Option<Interface>,
    patches: Vec<Arc<kontra_host::view::TreePatch>>,
    revision: u64,
    /// Highest UI command sequence acknowledged by the audio thread in a
    /// returned `LookFrame` (or applied directly for `Binding::Param`).
    applied_seq: u64,
    /// Each group's sound, copied when an edit moves it
    /// ([`kontra_host::sound::Sounds::revision`]).
    sounds: (u64, Arc<Vec<app::Sound>>),
    name: String,
    /// Its library's name in the browser, if the browser lists it.
    library: String,
    /// The preset it plays, as its library's preset index, found by path.
    preset: Option<usize>,
    /// The player changed it since the preset loaded.
    modified: bool,
    mix: Mix,
    solo: bool,
    mute: bool,
    /// Its output's peaks, then each instrument bus's.
    level: Vec<[f32; 2]>,
    /// Diagnostic count when `needs`/`diag_visible`/`diag_latent` were built.
    diag_count: usize,
    /// Actionable Broken/Audible items for the part's header warning popup.
    needs: Arc<Vec<String>>,
    /// Broken/Audible/Cosmetic diagnostics shown in the Report tab by default.
    diag_visible: Arc<Vec<String>>,
    /// Developer-only (`Impact::Latent`) diagnostics hidden behind Developer toggle.
    diag_latent: Arc<Vec<String>>,
}

impl PartView {
    /// Builds a resolved full [`Interface`] from this snapshot (used on initial
    /// face creation or when a reader fell behind the incremental patch window).
    fn full_resolved(&self) -> Option<Interface> {
        let mut tree = self.tree.as_ref()?.clone();
        RESOLVES.fetch_add(1, Ordering::Relaxed);
        let all = 0..tree.widgets.len();
        layout::resolve(&mut tree, all);
        Some(tree)
    }

    /// Applies incremental patches from `prev_rev` to `self.revision` directly
    /// onto `tree`, resolving layout only on changed widgets (0 full-tree
    /// clones and 0 full-tree resolves on single-slider drags).
    fn update_tree(&self, tree: &mut Interface, prev_rev: u64) {
        if prev_rev == self.revision {
            return;
        }
        let contiguous = self
            .patches
            .first()
            .is_some_and(|p| p.from_revision <= prev_rev)
            && self
                .patches
                .last()
                .is_some_and(|p| p.to_revision == self.revision);
        if contiguous {
            let mut changed = Vec::new();
            let mut expected = prev_rev;
            let mut full = false;
            for patch in &self.patches {
                if patch.to_revision <= prev_rev {
                    continue;
                }
                if patch.from_revision != expected {
                    full = true;
                    break;
                }
                expected = patch.to_revision;
                match patch.apply(tree) {
                    Some(indices) => changed.extend(indices),
                    None => {
                        full = true;
                        break;
                    }
                }
            }
            if !full && expected == self.revision {
                changed.sort_unstable();
                changed.dedup();
                layout::resolve(tree, changed);
                return;
            }
        }
        if let Some(fresh) = self.full_resolved() {
            *tree = fresh;
        }
    }

    /// Clean authoritative widget at `index` (used to revert an optimistic
    /// hold when a script rejects/clamps a move without bumping `revision`).
    fn widget_at(&self, index: usize) -> Option<kontra_ui_model::Widget> {
        self.tree.as_ref()?.widgets.get(index).cloned()
    }
}

/// How long a library control the player let go keeps the editor's value
/// if no audio `LookFrame` acknowledgement arrives (e.g. suspended audio).
const HOLD: Duration = Duration::from_millis(250);

/// A library control the player moved: its last move, sent as edit `seq`.
struct Held {
    edit: input::Edit,
    seq: u64,
    /// Set by [`Mine::keep`] while the pointer still holds this part's face.
    kept: bool,
    /// Fallback deadline start when `Tick` dispatched `seq` but audio has not
    /// echoed `applied_seq >= seq` yet.
    fallback_since: Option<Instant>,
}

/// The editor's own edits, which the rack's copy may not show yet.
#[derive(Default)]
pub(crate) struct Mine {
    /// By rack slot: the last edit sent ([`crate::Shared::request_ui`]).
    sent: HashMap<usize, u64>,
    /// By `(rack slot, load generation)`: the library controls the player moved.
    /// Scoped by generation so a previous load's tweaks never stamp onto a
    /// newly loaded preset.
    touched: HashMap<(usize, u64), Vec<Held>>,
}

impl Mine {
    /// Notes `action`, sent to rack `slot` (at load `generation`) as edit `seq`.
    fn note(&mut self, slot: usize, generation: u64, seq: u64, action: &Action) {
        self.sent.insert(slot, seq);
        let (widget, to) = match action {
            Action::Control {
                widget,
                cell: Some(k),
                value,
                ..
            } => (*widget, Set::Cell(*k, *value)),
            Action::Control { widget, value, .. } => (*widget, Set::Value(*value)),
            Action::ControlText { widget, text, .. } => (*widget, Set::Text(text.clone())),
            _ => return,
        };
        let edit = input::Edit {
            widget: kontra_ui_model::WidgetRef(widget),
            to,
            keys: Default::default(),
        };
        let mine = self.touched.entry((slot, generation)).or_default();
        let same = |e: &input::Edit| {
            e.widget == edit.widget
                && match (&e.to, &edit.to) {
                    (Set::Cell(a, _), Set::Cell(b, _)) => a == b,
                    (a, b) => std::mem::discriminant(a) == std::mem::discriminant(b),
                }
        };
        mine.retain(|h| !same(&h.edit));
        mine.push(Held {
            edit,
            seq,
            kept: false,
            fallback_since: None,
        });
    }

    /// The player still holds a control on rack `slot`'s face: its last
    /// move stays over every copy until the press ends.
    fn keep(&mut self, slot: usize, generation: u64) {
        let last = self.touched.get_mut(&(slot, generation));
        if let Some(h) = last.and_then(|held| held.last_mut()) {
            h.kept = true;
            h.fallback_since = None;
        }
    }

    /// Lets go of what the audio thread has acknowledged (`p.applied_seq >= h.seq`),
    /// or after [`HOLD`] past `Tick` dispatch if no audio reply arrives.
    /// Returns `(slot, widget_index)` pairs that were just released.
    fn prune(
        &mut self,
        parts: &[PartView],
        tick_applied: u64,
        now: Instant,
    ) -> Vec<(usize, usize)> {
        self.sent
            .retain(|slot, _| parts.iter().any(|p| p.slot == *slot));
        let mut released = Vec::new();
        self.touched.retain(|&(slot, generation), held| {
            let Some(p) = (parts.iter()).find(|p| (p.slot, p.generation) == (slot, generation))
            else {
                return false;
            };
            held.retain_mut(|h| {
                if std::mem::take(&mut h.kept) {
                    return true;
                }
                if p.applied_seq >= h.seq {
                    released.push((slot, h.edit.widget.0));
                    return false;
                }
                if tick_applied < h.seq {
                    return true;
                }
                let since = *h.fallback_since.get_or_insert(now);
                if now.duration_since(since) < HOLD {
                    true
                } else {
                    released.push((slot, h.edit.widget.0));
                    false
                }
            });
            !held.is_empty()
        });
        released
    }

    /// The edits to rack `slot`'s face (for load `generation`) still held.
    fn held(&self, slot: usize, generation: u64) -> Vec<input::Edit> {
        (self.touched.get(&(slot, generation)).into_iter().flatten())
            .map(|h| h.edit.clone())
            .collect()
    }
}

/// The rack as the editor last saw it.
#[derive(Debug, Default)]
pub struct Snapshot {
    parts: Vec<PartView>,
    /// The opening that needs a new copy and the one this copy is ready for.
    pub(crate) open_epoch: u64,
    pub(crate) ready_epoch: u64,
    /// The browser as lane D's app lists it, and which change of it.
    libraries: (u64, Arc<Vec<app::Library>>),
    recent: Vec<(usize, usize)>,
    folders: Vec<app::LibraryFolder>,
    /// A running library scan, read every tick.
    scan: Option<app::ScanProgress>,
    /// What the last scan could not read, for the report.
    scan_issues: Vec<String>,
    snapshot_folder: (String, Option<bool>),
    /// [`kontra_library::Prefs::next_channel`] and `own_output`.
    new_parts: (bool, bool),
    level: f32,
    stats: Stats,
    readout: Readout,
    report: Vec<String>,
    /// A file a part's script asks the player to pick, by the part's row.
    pick: Option<app::FilePick>,
    /// How many of the editor's edits ([`crate::Shared::request_ui`]) the
    /// rack had carried out when this copy was taken.
    pub(crate) applied: u64,
}

impl Snapshot {
    pub(crate) fn ready(&self) -> bool {
        self.ready_epoch == self.open_epoch
    }

    /// Copies what the editor shows from `rack`, with this tick's `report`;
    /// whether any of it changed, so an idle editor draws no frames.
    pub fn refresh(&mut self, rack: &Rack, browser: &Browser, report: Vec<String>) -> bool {
        let before = self.shown();
        let rescanned = self.libraries.0 != browser.generation;
        if rescanned {
            self.libraries = (browser.generation, Arc::new(libraries(browser)));
            self.recent = browser.recent();
            self.folders = (browser.folders().into_iter())
                .map(|f| app::LibraryFolder {
                    path: f.path.display().to_string(),
                    auto: f.auto,
                    status: f.summary,
                    hint: f.hint,
                    problem: f.problem,
                })
                .collect();
            self.scan_issues = (browser.catalog.issues.iter())
                .take(SCAN_ISSUES)
                .map(|i| format!("{}: {}", i.path.display(), i.reason))
                .collect();
            self.new_parts = (browser.prefs.next_channel, browser.prefs.own_output);
            self.snapshot_folder = match browser.kontakt() {
                Some(f) => (f.display().to_string(), Some(f.is_dir())),
                None => (String::new(), None),
            };
        }
        self.scan = browser
            .progress()
            .map(|(folder, files, found, machine)| app::ScanProgress {
                folder,
                files,
                found,
                machine,
            });
        let meters = rack.meters();
        let level = |slot: usize| {
            let out = meters.parts.get(slot).copied().unwrap_or_default();
            let buses = meters.buses.get(slot).into_iter().flatten().copied();
            std::iter::once(out).chain(buses).collect()
        };
        let mut old: HashMap<(usize, u64), PartView> = std::mem::take(&mut self.parts)
            .into_iter()
            .map(|v| ((v.slot, v.generation), v))
            .collect();
        self.parts = (rack.parts().iter().enumerate())
            .filter_map(|(slot, p)| {
                let p = p.as_ref()?;
                let generation = p.script.generation();
                let mut same = old.remove(&(slot, generation));
                let revision = p.view.as_ref().map_or(0, |v| v.revision);
                let applied_seq = p.view.as_ref().map_or(0, |v| v.applied_seq);
                let (tree, patches) = match same.as_mut() {
                    Some(v) if v.revision == revision => {
                        (v.tree.take(), std::mem::take(&mut v.patches))
                    }
                    Some(v) if v.tree.is_some() => {
                        match p
                            .view
                            .as_ref()
                            .and_then(|view| view.patches_since(v.revision))
                        {
                            Some(new_patches)
                                if !new_patches.iter().any(|patch| {
                                    matches!(
                                        patch.widgets,
                                        kontra_host::view::WidgetPatch::ReplaceAll(_)
                                    )
                                }) =>
                            {
                                let mut tree = v.tree.take();
                                if let Some(t) = tree.as_mut() {
                                    for patch in &new_patches {
                                        let _ = patch.apply(t);
                                    }
                                }
                                let mut patches = std::mem::take(&mut v.patches);
                                patches.extend(new_patches);
                                let excess = patches.len().saturating_sub(16);
                                if excess > 0 {
                                    patches.drain(..excess);
                                }
                                (tree, patches)
                            }
                            _ => (p.view.as_ref().map(|v| v.tree().clone()), Vec::new()),
                        }
                    }
                    _ => (p.view.as_ref().map(|v| v.tree().clone()), Vec::new()),
                };
                let sounds = match same.as_ref() {
                    Some(v) if v.sounds.0 == p.sounds.revision => v.sounds.clone(),
                    _ => (p.sounds.revision, Arc::new(sounds(&p.sounds))),
                };
                // A recall changes the path, not the load.
                let (library, preset) = match same.as_ref() {
                    Some(v) if !rescanned && v.path == p.path => (v.library.clone(), v.preset),
                    _ => {
                        let found = p.path.as_deref().and_then(|path| browser.find(path));
                        let library = found.and_then(|(l, _)| browser.catalog.libraries.get(l));
                        let name = library.map(|l| l.name.clone()).unwrap_or_default();
                        (name, found.map(|(_, n)| n))
                    }
                };
                let diag_count = p.diagnostics.len();
                let (needs, diag_visible, diag_latent) = match same.as_ref() {
                    Some(v) if v.diag_count == diag_count => (
                        Arc::clone(&v.needs),
                        Arc::clone(&v.diag_visible),
                        Arc::clone(&v.diag_latent),
                    ),
                    _ => (
                        Arc::new(p.needs()),
                        Arc::new(p.visible_diagnostics(slot)),
                        Arc::new(p.latent_diagnostics(slot)),
                    ),
                };
                Some(PartView {
                    slot,
                    generation,
                    path: p.path.clone(),
                    source: p.source.clone().or_else(|| p.path.clone()),
                    tree,
                    patches,
                    revision,
                    applied_seq,
                    sounds,
                    name: p.name.clone(),
                    library,
                    preset,
                    modified: p.modified,
                    mix: p.mix,
                    solo: p.solo,
                    mute: p.mute,
                    level: level(slot),
                    diag_count,
                    needs,
                    diag_visible,
                    diag_latent,
                })
            })
            .collect();
        self.pick = rack.pick().and_then(|pick| {
            let part = self.parts.iter().position(|v| v.slot == pick.part)?;
            Some(app::FilePick {
                part,
                save: pick.save,
                names: pick.names,
            })
        });
        self.level = meters.master[0].max(meters.master[1]);
        if let Some(s) = rack.stats() {
            self.readout.peak(&s, rack.rate());
            self.stats = s;
        }
        self.readout.sample(&self.stats, rack);
        self.report(report) | (self.shown() != before)
    }

    /// Adds this tick's `report`; whether it held anything.
    pub fn report(&mut self, report: Vec<String>) -> bool {
        let reported = !report.is_empty();
        self.report.extend(report);
        let over = self.report.len().saturating_sub(REPORT);
        self.report.drain(..over);
        reported
    }

    #[cfg(test)]
    pub(crate) fn reports(&self) -> &[String] {
        &self.report
    }

    /// What a frame draws of this, as it reads on screen: the CPU load in
    /// whole percent; a part's interface by its load and revision.
    fn shown(&self) -> impl PartialEq + use<> {
        let parts: Vec<_> = (self.parts.iter())
            .map(|p| {
                let mix = (
                    p.mix,
                    p.solo,
                    p.mute,
                    p.level
                        .iter()
                        .map(|l| l.map(f32::to_bits))
                        .collect::<Vec<_>>(),
                );
                (
                    p.slot,
                    p.generation,
                    p.revision,
                    p.applied_seq,
                    p.diag_count,
                    p.name.clone(),
                    p.library.clone(),
                    p.preset,
                    p.modified,
                    mix,
                )
            })
            .collect();
        (
            self.libraries.0,
            parts,
            self.level.to_bits(),
            self.readout.shown(),
            self.stats.voices,
            self.pick.clone(),
            self.scan.clone(),
        )
    }

    /// Fills `app` for a frame, keeping each part's own view state keyed by
    /// rack slot (never positional Vec index). `mine` is what the editor sent:
    /// a part's mix this copy predates, and a library control [`Mine`] still
    /// holds, keep what the editor drew, so a drag never springs back.
    #[cfg(test)]
    fn fill(&self, app: &mut App, shown: &mut Shown, mine: &Mine) {
        self.fill_with(app, shown, mine, &[], None);
    }

    fn fill_with(
        &self,
        app: &mut App,
        shown: &mut Shown,
        mine: &Mine,
        released: &[(usize, usize)],
        wake: Option<&Arc<dyn Fn() + Send + Sync>>,
    ) {
        let sent = &mine.sent;
        if shown.browser != self.libraries.0 {
            let fresh = self.libraries.1.as_ref();
            // A load or a star changes the list, not its art: the covers
            // decoded so far stay (they are keyed by library and art).
            let same = (app.libraries.iter().map(|l| &l.art)).eq(fresh.iter().map(|l| &l.art));
            if app.covers.is_none() || !same {
                app.covers = Some(match wake {
                    Some(w) => Pictures::with_wake(Box::new(cover), Arc::clone(w)),
                    None => Pictures::new(Box::new(cover)),
                });
                app.banners = Some(match wake {
                    Some(w) => Pictures::with_look_and_wake(
                        Box::new(cover),
                        kontra_ui::artwork::banner,
                        Arc::clone(w),
                    ),
                    None => Pictures::with_look(Box::new(cover), kontra_ui::artwork::banner),
                });
            }
            app.libraries = fresh.clone();
            app.recent.clone_from(&self.recent);
            app.folders.clone_from(&self.folders);
            app.scan_issues.clone_from(&self.scan_issues);
            app.snapshot_folder.clone_from(&self.snapshot_folder);
            (app.next_channel, app.own_output) = self.new_parts;
            shown.browser = self.libraries.0;
        }
        for pictures in [&mut app.covers, &mut app.banners].into_iter().flatten() {
            pictures.poll();
        }
        let prev_slots = std::mem::take(&mut shown.slots);
        let mut old_by_slot: HashMap<usize, Part> = prev_slots
            .into_iter()
            .zip(std::mem::take(&mut app.parts))
            .collect();
        let prev_parts = std::mem::take(&mut shown.parts);
        let prev_sounds = std::mem::take(&mut shown.sounds);
        let parts = self.parts.iter().map(|p| {
            let mut old = old_by_slot.remove(&p.slot);
            let prev = prev_parts.get(&p.slot);
            let held = sent.get(&p.slot).is_some_and(|&s| s > self.applied)
                && prev.is_some_and(|s| s.0 == p.generation)
                && old.is_some();
            let (open, tab, look) = old
                .as_ref()
                .map_or((true, Default::default(), None), |old| {
                    (old.open, old.tab, old.look)
                });
            let at = (p.generation, p.revision);
            let sound_now = (p.generation, p.sounds.0);
            // The editor's own edit shows at once; the rack's copy once it moved.
            let sounds = match old.as_mut() {
                Some(old) if prev_sounds.get(&p.slot) == Some(&sound_now) => {
                    std::mem::take(&mut old.sounds)
                }
                _ => p.sounds.1.as_ref().clone(),
            };
            let face = match old.as_mut() {
                Some(old) if prev.is_some_and(|s| (s.0, s.1) == at) => {
                    let mut f = old.face.take();
                    if let Some(f) = f.as_mut() {
                        // Revert any optimistic hold that just released without
                        // a revision bump (e.g. script clamped/rejected edit).
                        let mut reverted = Vec::new();
                        for &(slot, idx) in released {
                            if slot == p.slot
                                && let Some(clean) = p.widget_at(idx)
                                && let Some(dst) = f.tree.widgets.get_mut(idx)
                            {
                                *dst = clean;
                                reverted.push(idx);
                            }
                        }
                        if !reverted.is_empty() {
                            layout::resolve(&mut f.tree, reverted);
                        }
                    }
                    f
                }
                // Same load: patch only changed widgets incrementally without
                // cloning or re-resolving the whole tree.
                Some(old) if prev.is_some_and(|s| s.0 == p.generation) => {
                    let prev_rev = prev.map_or(0, |s| s.1);
                    old.face.take().map(|mut f| {
                        p.update_tree(&mut f.tree, prev_rev);
                        f
                    })
                }
                // Different load of the same instrument asset root (e.g.
                // snapshot swap): keep decoded Pictures cached to avoid flashes.
                Some(old) if prev.is_some_and(|s| p.source.is_some() && s.2 == p.source) => {
                    old.face.take().zip(p.full_resolved()).map(|(mut f, tree)| {
                        f.tree = tree;
                        f.gesture = Default::default();
                        f
                    })
                }
                _ => p
                    .full_resolved()
                    .map(|tree| face_resolved(tree, p.source.clone(), wake)),
            };
            // The controls the player is moving show where the player put
            // them until the script has had them.
            let mut face = face;
            if let Some(f) = face.as_mut() {
                input::apply(&mut f.tree, &mine.held(p.slot, p.generation));
            }
            let own = |f: fn(&Part) -> f64, theirs: f64| match &old {
                Some(old) if held => f(old),
                _ => theirs,
            };
            let (mix_held, old_mix) = match &old {
                Some(old) if held => (true, (old.channel, old.output, old.solo, old.mute)),
                _ => (false, Default::default()),
            };
            let (channel, output, solo, mute) = if mix_held {
                old_mix
            } else {
                (p.mix.channel, p.mix.output + 1, p.solo, p.mute)
            };
            Part {
                face,
                name: p.name.clone(),
                library: p.library.clone(),
                preset: p.preset,
                modified: p.modified,
                channel,
                output,
                volume: own(|o| o.volume, f64::from(linear_to_db(p.mix.gain))),
                pan: own(|o| o.pan, f64::from(p.mix.pan)),
                level: p.level.iter().map(|l| l.map(f64::from)).collect(),
                solo,
                mute,
                keys: (0, 127),
                needs: p.needs.as_ref().clone(),
                open,
                tab,
                look,
                sounds,
                ..Part::default()
            }
        });
        app.parts = parts.collect();
        // What the picture workers decoded since the last frame.
        for face in app.parts.iter_mut().filter_map(|p| p.face.as_mut()) {
            face.pictures.poll();
        }
        shown.slots = self.parts.iter().map(|p| p.slot).collect();
        shown.parts = self
            .parts
            .iter()
            .map(|p| (p.slot, (p.generation, p.revision, p.source.clone())))
            .collect();
        shown.sounds = self
            .parts
            .iter()
            .map(|p| (p.slot, (p.generation, p.sounds.0)))
            .collect();
        app.level = f64::from(self.level);
        let r = &self.readout;
        app.stats = app::Stats {
            cpu: r.cpu,
            peak: r.peak,
            voices: self.stats.voices,
            ram_mb: r.process_mb.unwrap_or(0),
            samples_mb: r.samples_mb,
            disk_mbs: r.disk_mbs,
            cache_mb: r.cache_mb,
        };
        app.report.clear();
        app.report.extend(self.report.iter().cloned());
        app.report.extend(
            self.parts
                .iter()
                .flat_map(|p| p.diag_visible.iter().cloned()),
        );
        app.report_latent.clear();
        app.report_latent.extend(
            self.parts
                .iter()
                .flat_map(|p| p.diag_latent.iter().cloned()),
        );
        app.pick.clone_from(&self.pick);
        app.scan.clone_from(&self.scan);
    }
}

/// Which change of the rack's parts and of the browser the editor shows.
/// Keyed by rack slot so removing or inserting parts never aliases state.
#[derive(Default)]
struct Shown {
    /// Rack slot for each entry in `app.parts`, in `app.parts` order.
    slots: Vec<usize>,
    /// By rack slot: its load generation, interface revision, and instrument source path.
    parts: HashMap<usize, (u64, u64, Option<PathBuf>)>,
    /// By rack slot: its load generation and sound revision.
    sounds: HashMap<usize, (u64, u64)>,
    browser: u64,
}

/// Saves, renames or deletes the player's preset `part` plays, under the
/// browser's presets folder, and lists the user presets again.
fn edit_preset(
    rack: &mut Rack,
    browser: &mut Browser,
    part: usize,
    edit: app::PresetEdit,
) -> std::io::Result<()> {
    use kontra_host::presets::{self, Overwrite};
    let folder = browser
        .user
        .clone()
        .ok_or_else(|| std::io::Error::other("no presets folder"))?;
    let playing = rack.parts().get(part).and_then(Option::as_ref);
    let path = playing.and_then(|p| p.path.clone());
    let library = (path.as_deref().and_then(|p| browser.find(p)))
        .and_then(|(l, _)| browser.catalog.libraries.get(l))
        .map(|l| l.name.clone())
        .ok_or_else(|| std::io::Error::other("not in a listed library"))?;
    match (edit, path) {
        (app::PresetEdit::Save(name), _) => {
            let saved = rack.save_preset(part, &folder, &library, &name, Overwrite::Create);
            if let Err(e) = saved {
                // The list was stale: read again, Save preset as… offers Replace.
                browser.user_changed();
                return Err(match e.kind() {
                    std::io::ErrorKind::AlreadyExists => std::io::Error::new(
                        e.kind(),
                        format!("\"{name}\" exists: Save preset as… again to replace it"),
                    ),
                    _ => e,
                });
            }
        }
        (app::PresetEdit::Replace(name), _) => {
            rack.save_preset(part, &folder, &library, &name, Overwrite::Replace)?;
        }
        (app::PresetEdit::Rename(name), Some(path)) => {
            let to = presets::rename(&folder, &path, &name)?;
            rack.preset_moved(&path, Some(&to));
        }
        // What plays stays; it is its base instrument, changed, from now on.
        (app::PresetEdit::Delete, Some(path)) => {
            presets::delete(&folder, &path)?;
            rack.preset_moved(&path, None);
        }
        _ => return Err(std::io::Error::other("the part plays no user preset")),
    }
    browser.user_changed();
    Ok(())
}

/// The catalog as the browser lists it: presets by path inside their
/// library, without the extension.
pub(crate) fn libraries(browser: &Browser) -> Vec<app::Library> {
    // Kontakt's user snapshots, read only: `User Content/<instrument>/<name>`.
    let kontakt = browser.kontakt();
    let user_content = kontakt.as_deref().and_then(std::path::Path::parent);
    (browser.catalog.libraries.iter())
        .map(|l| app::Library {
            name: l.name.clone(),
            format: match l.format {
                kontra_library::Format::Kontakt => app::Format::Kontakt,
                kontra_library::Format::Uvi => app::Format::Uvi,
            },
            protection: match &l.protection {
                kontra_library::ProtectionStatus::Open => app::Protection::Open,
                kontra_library::ProtectionStatus::Protected { system, detail } => {
                    app::Protection::Protected {
                        system: system.to_string(),
                        detail: detail.clone(),
                    }
                }
            },
            recalls: (l.presets.iter().enumerate())
                .filter_map(|(n, p)| is_preset(p).then_some(n))
                .collect(),
            presets: (l.presets.iter())
                .map(
                    |p| match browser.user.as_deref().filter(|u| p.starts_with(u)) {
                        // The player's own, in a folder of their own.
                        Some(_) => format!(
                            "{}{}",
                            app::USER,
                            p.file_stem().unwrap_or_default().display()
                        ),
                        None => {
                            let from = user_content.filter(|k| p.starts_with(k));
                            let from = from.unwrap_or(&l.dir);
                            let p = p.strip_prefix(from).unwrap_or(p).with_extension("");
                            p.to_string_lossy().into_owned()
                        }
                    },
                )
                .collect(),
            starred: browser.prefs.starred(&l.dir),
            favorites: (l.presets.iter().enumerate())
                .filter_map(|(n, p)| browser.prefs.starred(p).then_some(n))
                .collect(),
            art: art(&l.art),
            offline: l.offline.clone(),
        })
        .collect()
}

/// Pictures the library holds inside a resource container, best first:
/// its library-browser banner (stored with the dot), then its artwork.
const HELD: [&str; 2] = [".LibBrowser.png", "MST_artwork.png"];

/// The cover keys of a library's ranked pictures and containers: a PNG's
/// path, or a container's path, a NUL, and the picture's name inside it
/// (none for a UVI bank, which picks its own).
/// Any container reads the whole library's, so only the first is asked.
pub(crate) fn art(paths: &[PathBuf]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut held = false;
    for path in paths {
        let ext = path.extension().map(|e| e.to_ascii_lowercase());
        let name = path.to_string_lossy();
        match ext.as_ref().and_then(|e| e.to_str()) {
            Some("png" | "jpg" | "jpeg") => keys.push(name.into_owned()),
            Some("nicnt" | "nkr") if !held => {
                held = true;
                keys.extend(HELD.iter().map(|n| format!("{name}\0{n}")));
            }
            // A bank chooses its own cover when read (an empty name).
            Some("ufs") => keys.push(format!("{name}\0")),
            _ => {}
        }
    }
    keys
}

/// The bytes of the cover picture `key` names (see [`art`]).
pub(crate) fn cover(key: &str) -> Option<Vec<u8>> {
    match key.split_once('\0') {
        Some((bank, "")) => kontra_host::bank_cover(Path::new(bank)),
        Some((container, name)) => Library::open(Path::new(container)).read(name),
        None => {
            let file = std::fs::File::open(key).ok()?;
            let mut bytes = Vec::new();
            std::io::Read::read_to_end(&mut std::io::Read::take(file, COVER_BYTES), &mut bytes)
                .ok()?;
            Some(bytes)
        }
    }
}

/// Places a part the browser added where the player's prefs say: the
/// lowest MIDI channel no other part has, its own output pair.
pub fn place(rack: &mut Rack, browser: &Browser, part: usize) {
    let parts = rack.parts();
    let Some(mut mix) = parts.get(part).and_then(Option::as_ref).map(|p| p.mix) else {
        return;
    };
    if browser.prefs.next_channel {
        let taken = |c: &u8| {
            (parts.iter().enumerate())
                .any(|(n, p)| n != part && p.as_ref().is_some_and(|p| p.mix.channel == *c))
        };
        mix.channel = (1..=64).find(|c| !taken(c)).unwrap_or(0);
    }
    if browser.prefs.own_output {
        mix.output = u8::try_from(part.min(crate::OUTPUTS - 1)).unwrap_or(0);
    }
    rack.set_mix(part, mix);
}

/// The largest cover file read.
const COVER_BYTES: u64 = 32 << 20;

/// Carries out a browser action: a preset loads into the first free part.
pub fn browse(rack: &mut Rack, browser: &mut Browser, action: Action, report: &mut Vec<String>) {
    match action {
        Action::Load {
            library,
            preset,
            into,
        } => {
            let Some(path) = browser.preset(library, preset) else {
                return;
            };
            // A preset recalls into the part named; anything else adds one.
            let into = into
                .filter(|_| is_preset(&path))
                .filter(|&p| rack.parts().get(p).is_some_and(Option::is_some));
            let loaded = match (into, rack.free()) {
                (Some(part), _) => rack.load(part, &path),
                (None, Some(part)) => rack.load(part, &path).map(|()| place(rack, browser, part)),
                (None, None) => Err(kontra_host::Error::NoFreePart),
            };
            match loaded {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        // Replaces the part's instrument with another of its library's.
        Action::Switch { part, preset } => {
            let library = (rack.parts().get(part).and_then(Option::as_ref))
                .and_then(|p| browser.find(p.path.as_deref()?));
            let Some(path) = library.and_then(|(l, _)| browser.preset(l, preset)) else {
                return;
            };
            match rack.load(part, &path) {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        Action::Replace {
            part,
            library,
            preset,
        } => {
            let Some(path) = browser.preset(library, preset) else {
                return;
            };
            match rack.load(part, &path) {
                Ok(()) => browser.loaded(&path),
                Err(e) => report.push(format!("{}: {e}", path.display())),
            }
        }
        Action::Step { part, by } => {
            let found = (rack.parts().get(part).and_then(Option::as_ref))
                .and_then(|p| browser.find(p.path.as_deref()?));
            let Some((library, preset)) = found else {
                return;
            };
            // Through what the header lists: the instrument and its
            // snapshots, or the whole library.
            let family = (browser.catalog.libraries.get(library))
                .map(|l| kontra_ui_model::preset_family(&l.dir, &l.presets, Some(preset)))
                .unwrap_or_default();
            let (count, at) = (family.len(), family.iter().position(|&n| n == preset));
            let Some(at) = at else {
                return;
            };
            let step = by.unsigned_abs() as usize % count;
            // A full turn (or a one-preset library) stays: no reload.
            if step == 0 {
                return;
            }
            let at = if by < 0 { at + count - step } else { at + step } % count;
            let Some(&preset) = family.get(at) else {
                return;
            };
            let switch = Action::Switch { part, preset };
            browse(rack, browser, switch, report);
        }
        Action::Preset { part, edit } => {
            if let Err(e) = edit_preset(rack, browser, part, edit) {
                report.push(format!("preset: {e}"));
            }
        }
        Action::Star(library, on) => browser.star(library, on),
        Action::StarPreset(library, preset, on) => browser.star_preset(library, preset, on),
        Action::Refresh => browser.refresh(),
        Action::ScanMachine => browser.scan_machine(),
        Action::CancelScan => browser.cancel(),
        Action::Relink(line) => {
            if let Some(o) = rack.take_offer(&line) {
                browser.relink(o.from, o.to);
                rack.know(browser);
                let scripts = rack.saved_scripts(o.part);
                if let Err(e) = rack.load_saved(o.part, &o.path, &scripts, &[], &[]) {
                    report.push(format!("{}: {e}", o.path.display()));
                }
            }
        }
        Action::RemoveFolder(n) => browser.remove_root(n),
        Action::Picked(name) => rack.picked(name.as_deref()),
        Action::NewParts {
            next_channel,
            own_output,
        } => browser.new_parts(next_channel, own_output),
        Action::Theme(tokens) => browser.set_theme(&tokens),
        _ => {}
    }
}

/// A library's interface for the editor, laid out, its pictures read from
/// the library at `path` off the UI thread.
#[cfg(test)]
fn face(tree: &Interface, path: Option<PathBuf>) -> Face {
    face_resolved(resolved(tree), path, None)
}

fn face_resolved(
    tree: Interface,
    path: Option<PathBuf>,
    wake: Option<&Arc<dyn Fn() + Send + Sync>>,
) -> Face {
    FACES.fetch_add(1, Ordering::Relaxed);
    let library = Mutex::new(None);
    let read = Box::new(move |p: &str| {
        let path = path.as_deref()?;
        let mut library = library.lock().ok()?;
        library.get_or_insert_with(|| Library::open(path)).read(p)
    });
    let pictures = match wake {
        Some(w) => Pictures::with_wake(read, Arc::clone(w)),
        None => Pictures::new(read),
    };
    Face {
        tree,
        pictures,
        gesture: Default::default(),
        meters: Default::default(),
    }
}

/// Library faces built and trees laid out in this process, for probes.
static FACES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static RESOLVES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// `tree` laid out.
#[cfg(test)]
fn resolved(tree: &Interface) -> Interface {
    RESOLVES.fetch_add(1, Ordering::Relaxed);
    let mut tree = tree.clone();
    let all = 0..tree.widgets.len();
    layout::resolve(&mut tree, all);
    tree
}

/// A user's edit to one rack part, or to the whole rack.
pub struct Edit {
    pub(crate) slot: Option<usize>,
    pub(crate) action: Action,
    pub(crate) seq: u64,
}

/// Carries out `edit` on the rack.
pub fn apply(rack: &mut Rack, edit: Edit) {
    let seq = edit.seq;
    let part = edit
        .slot
        .and_then(|s| Some((s, rack.parts().get(s)?.as_ref()?)));
    let mix = |(slot, p): (usize, &kontra_host::rack::Part)| (slot, p.mix, p.solo, p.mute);
    let Some((slot, mut m, solo, mute)) = part.map(mix) else {
        if edit.action == Action::Panic {
            rack.panic();
        }
        return;
    };
    match edit.action {
        Action::Volume(_, db) => m.gain = db_to_linear(db as f32),
        Action::Pan(_, pan) => m.pan = pan as f32,
        Action::Channel(_, c) => m.channel = c.min(64),
        // Output 0 lets the host pick, which is the first pair.
        Action::Output(_, o) => m.output = o.saturating_sub(1),
        Action::Solo(_, on) => return rack.set_solo_mute(slot, on, mute),
        Action::Mute(_, on) => return rack.set_solo_mute(slot, solo, on),
        Action::Remove(_) => return rack.remove(slot),
        Action::Control {
            widget,
            cell,
            value,
            keys,
            ..
        } => return rack.control_with_seq(slot, widget, cell, value, keys, seq),
        Action::ControlText { widget, text, .. } => {
            return rack.control_text_with_seq(slot, widget, &text, seq);
        }
        Action::ControlDrop {
            widget,
            drop,
            over,
            ref items,
            ..
        } => return rack.control_drop(slot, widget, (drop, over), items),
        Action::Sound {
            group,
            param,
            value,
            ..
        } => return rack.set_sound(slot, group, param_of(param), value),
        _ => return,
    }
    rack.set_mix(slot, m);
}

/// Each group's sound as the Sound tab draws it.
fn sounds(s: &kontra_host::sound::Sounds) -> Vec<app::Sound> {
    use kontra_host::sound::BandKind;
    let filter = |b: kontra_host::sound::Band| app::Filter {
        pass: match b.kind {
            BandKind::Low => app::Pass::Low,
            BandKind::High => app::Pass::High,
            BandKind::Band => app::Pass::Band,
            BandKind::Notch => app::Pass::Notch,
            BandKind::Peak => app::Pass::Peak,
            BandKind::LowShelf => app::Pass::LowShelf,
            BandKind::HighShelf => app::Pass::HighShelf,
        },
        hz: b.hz,
        q: b.q,
        gain_db: b.gain_db,
        poles: b.poles,
    };
    (0..s.len())
        .map(|g| app::Sound {
            envelope: s
                .envelope(g)
                .map(|[attack, hold, decay, sustain, release]| app::Envelope {
                    attack,
                    hold,
                    decay,
                    sustain,
                    release,
                }),
            filters: s.bands(g).into_iter().map(filter).collect(),
            ..app::Sound::default()
        })
        .collect()
}

fn param_of(p: app::Param) -> kontra_host::sound::Param {
    use kontra_host::sound::Param as H;
    match p {
        app::Param::Attack => H::Attack,
        app::Param::Hold => H::Hold,
        app::Param::Decay => H::Decay,
        app::Param::Sustain => H::Sustain,
        app::Param::Release => H::Release,
        app::Param::Hz(n) => H::Hz(n),
        app::Param::Q(n) => H::Q(n),
        app::Param::Gain(n) => H::Gain(n),
    }
}

/// The rack part an action names, by its place in the editor's list.
fn part_of(action: &Action) -> Option<usize> {
    match *action {
        Action::Volume(n, _)
        | Action::Pan(n, _)
        | Action::Channel(n, _)
        | Action::Output(n, _)
        | Action::Solo(n, _)
        | Action::Mute(n, _)
        | Action::Remove(n)
        | Action::Control { part: n, .. }
        | Action::ControlText { part: n, .. }
        | Action::ControlDrop { part: n, .. }
        | Action::Sound { part: n, .. } => Some(n),
        _ => None,
    }
}

/// The editor's state between frames. It outlives the window: closing
/// the editor hands it back to the plugin ([`crate::Shared::editor`]) and
/// the next opening takes it, so reopening finds every face laid out and
/// every picture decoded.
pub(crate) struct Kept {
    app: App,
    shown: Shown,
    mine: Mine,
    /// Where it goes back to when the editor closes.
    home: Option<Arc<KontraParams>>,
}

impl Kept {
    /// What the last editor of `params` left, or a fresh state.
    fn take(params: &Arc<KontraParams>) -> Self {
        let kept = lock(&params.shared.editor).take();
        let mut kept = kept.unwrap_or_else(|| Self {
            app: App {
                build: crate::BUILD,
                ..App::default()
            },
            shown: Shown::default(),
            mine: Mine::default(),
            home: None,
        });
        // A press the last window saw never ends in this one.
        for face in kept.app.parts.iter_mut().filter_map(|p| p.face.as_mut()) {
            face.gesture.cancel();
        }
        kept.home = Some(Arc::clone(params));
        kept
    }
}

impl Drop for Kept {
    fn drop(&mut self) {
        let Some(home) = self.home.take() else { return };
        let kept = Self {
            app: std::mem::take(&mut self.app),
            shown: std::mem::take(&mut self.shown),
            mine: std::mem::take(&mut self.mine),
            home: None,
        };
        *lock(&home.shared.editor) = Some(kept);
    }
}

/// The window a fresh editor opens at, in logical points at 100 %.
const SIZE: (u32, u32) = (1180, 780);
/// The smallest window, in logical points: the chrome still fits at 100 %.
const MIN: (u32, u32) = (940, 600);

/// The interface scale in percent: one of [`app::SCALES`], 0 or another
/// value is 100.
fn percent(scale: u32) -> u32 {
    if app::SCALES.contains(&scale) {
        scale
    } else {
        100
    }
}

/// A new interface scale: the text and controls take it at once and the
/// layout reflows into the window. The window, last laid out at `logical`
/// points, is asked to grow or shrink by the same ratio so what fitted
/// still fits (a host may refuse; the layout reflows either way).
fn rescale(
    zoom: &AtomicU32,
    percent: u32,
    logical: (f64, f64),
    resize: impl FnOnce(u32, u32) -> bool,
) {
    let before = zoom.swap(percent, Ordering::AcqRel);
    if before != percent && logical.0 > 0. && logical.1 > 0. {
        let by = |v: f64, min: u32| ((v * f64::from(percent) / 100.).round() as u32).max(min);
        let _ = resize(by(logical.0, MIN.0), by(logical.1, MIN.1));
    }
}

pub fn editor(params: Arc<KontraParams>) -> Box<dyn Editor> {
    let wheel = WheelPolicy::new(
        moose::mui::mui::host::NOTCH_LINES as f32,
        WheelAcceleration::Fling,
    )
    .unwrap_or_default();
    let p = Arc::clone(&params);
    let watched = Arc::clone(&params);
    let dropping = Arc::clone(&params);
    // Files dragged over the window since the last frame, for the
    // library's mouse areas (kontra_ui::input::files).
    let dragged: Arc<std::sync::Mutex<Option<kontra_ui::input::FileDrag>>> = Arc::default();
    let dragging_files = Arc::clone(&dragged);
    let owner = Arc::clone(&params);
    #[cfg(target_os = "linux")]
    let x11_owner = Arc::clone(&params);
    let closing = Arc::clone(&params);
    let log_params = Arc::clone(&params);
    let cancel_params = Arc::clone(&params);
    let waker_params = Arc::clone(&params);
    let picture_wake: Arc<dyn Fn() + Send + Sync> = {
        let p = Arc::clone(&params);
        Arc::new(move || p.shared.wake_editor())
    };
    // Faces, pictures and view state the last window left.
    let mut kept = Kept::take(&params);
    let mut look = SavedView::default();
    let mut waiting = Vec::new();
    // The host macro of the panel control being dragged, and its value: its
    // gesture stays open until the press ends, or the host would play its
    // own older value back between moves and the control would jump.
    let mut dragging: Option<(u32, f64)> = None;
    let mut restored = None;
    // Where the last load the editor asked for lands.
    let mut target = app::Loading::New;
    let mut buttons = None;
    // Between builds: the frame before's paint and submit, and any idle wait.
    let mut last: Option<std::time::Instant> = None;
    // Escape reaches the host when no field is focused; noted here, it
    // still closes the open menu or sheet.
    let escape = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pressed = Arc::clone(&escape);
    // Host input cancellation (focus loss, Wayland leave, modal dialog):
    // clears any in-flight library-face gesture and macro drag.
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let on_cancelled = Arc::clone(&cancelled);
    // The interface scale: text and controls at this percent of their size,
    // laid out into whatever window the host gives (not zoomed to fit it).
    let pct = {
        let saved = SavedView::read(&params.look.read().unwrap_or_else(PoisonError::into_inner));
        percent(crate::look::scale_of(saved.scale))
    };
    let initial_size = (SIZE.0 * pct / 100, SIZE.1 * pct / 100);
    let zoom = Arc::new(AtomicU32::new(pct));
    let zoomed = Arc::clone(&zoom);
    let editor = MuiEditor::new(params, kontra_ui::ui(), initial_size, move |ui, bridge| {
        let Kept {
            app, shown, mine, ..
        } = &mut kept;
        let start = std::time::Instant::now();
        if escape.swap(false, Ordering::AcqRel) {
            app.view.dismiss();
        }
        if cancelled.swap(false, Ordering::AcqRel) {
            for part in &mut app.parts {
                if let Some(face) = part.face.as_mut() {
                    face.gesture.cancel();
                }
            }
            dragging = None;
        }
        if let Some(t) = last.replace(start) {
            crate::frames::note("between", start - t);
        }
        // Refresh the closed editor's stale snapshot on Moose's background
        // worker. Opening the window must return even if a load holds the rack.
        if let Some(tasks) = bridge.context().and_then(|c| c.tasks::<crate::Tick>()) {
            p.shared.set_tasks(Some(tasks));
            if !p.shared.shown.load(Ordering::Acquire) {
                p.shared.open_editor();
            }
        }
        // On opening, and whenever the host loads a state: its view.
        let now = p.shared.restored.load(Ordering::Acquire);
        if restored != Some(now) {
            restored = Some(now);
            look = SavedView::read(&p.look.read().unwrap_or_else(PoisonError::into_inner));
            crate::look::apply(app, &look);
            waiting.clone_from(&look.parts);
        }
        let (slots, slot_gens): (Vec<usize>, Vec<(usize, u64)>) = {
            let view = lock(&p.shared.view);
            // An older tick may still publish while the editor reopens.
            // Build no interactive rows until a tick for this opening finishes.
            if !p.shared.shown.load(Ordering::Acquire) || !view.ready() {
                return moose::mui::mui::prelude::body("Refreshing rack…");
            }
            // A control pressed on a face keeps its move for as long as the
            // press lasts, keyed by each part's own rack slot (never Vec index).
            for (part, &slot) in app.parts.iter().zip(&shown.slots) {
                if part.face.as_ref().is_some_and(|f| f.gesture.held())
                    && let Some(&(generation, ..)) = shown.parts.get(&slot)
                {
                    mine.keep(slot, generation);
                }
            }
            let now = Instant::now();
            let released = mine.prune(&view.parts, view.applied, now);
            view.fill_with(app, shown, mine, &released, Some(&picture_wake));
            (
                view.parts.iter().map(|v| v.slot).collect(),
                view.parts.iter().map(|v| (v.slot, v.generation)).collect(),
            )
        };
        crate::look::parts(app, &slots, &mut waiting);
        app.master = f64::from(p.volume.value());
        app.loading = p.shared.loading().then_some(target);
        app.load_progress = p.shared.progress.snapshot();
        crate::frames::note("fill", start.elapsed());
        app.view.files = lock(&dragged).take();
        app.view.pointer = bridge.pointer().pos;
        let response = crate::frames::time("view", || kontra_ui::app::view(ui, app));
        app.view.files = None;
        app.view.pointer = None;
        // A frame reads its input as it builds, so what a press or release
        // changes (a switch applied to the face, a row selected) shows in
        // the frame after it: ask for that one, or it waits for more input.
        let held = ui.pointer().buttons;
        if buttons.replace(held) != Some(held) || !response.changed.is_empty() {
            p.shared.wake_editor();
        }
        for action in response.changed {
            let asked = match action {
                Action::Load {
                    library,
                    preset,
                    into: Some(n),
                } if (app.libraries.get(library))
                    .is_some_and(|l| l.recalls.binary_search(&preset).is_ok()) =>
                {
                    Some(app::Loading::Part(n))
                }
                Action::Load { .. } => Some(app::Loading::New),
                Action::Replace { part, .. }
                | Action::Switch { part, .. }
                | Action::Step { part, .. } => Some(app::Loading::Part(part)),
                _ => None,
            };
            if let Some(t) = asked {
                // One more frame, so the chip shows before the load ends.
                target = t;
                p.shared.wake_editor();
            }
            if let Action::Scale(scale) = action {
                let logical = ui
                    .scene()
                    .map_or((0., 0.), |s| (s.layout.size.width, s.layout.size.height));
                rescale(&zoom, percent(scale), logical, |w, h| {
                    bridge.request_resize(w, h)
                });
                continue;
            }
            if let Action::Master(db) = action {
                bridge.drag_gesture(P::Volume, db);
                continue;
            }
            if action == Action::AddFolder {
                // The browser's first folder, from the editor's own copy.
                let from = app.folders.first().map(|f| PathBuf::from(&f.path));
                p.shared.folder.ask(crate::folder::Wanted::Libraries, from);
                if p.shared.folder.has_result() {
                    p.shared.wake_tick();
                }
                continue;
            }
            if let Action::Locate(n) = action {
                // ponytail: by catalog index; a rescan landing before the
                // pick may shift it, and the wrong library is then refused
                // by identity.
                p.shared.folder.ask(crate::folder::Wanted::Locate(n), None);
                if p.shared.folder.has_result() {
                    p.shared.wake_tick();
                }
                continue;
            }
            if action == Action::ChooseSnapshotFolder {
                p.shared.folder.ask(crate::folder::Wanted::Snapshots, None);
                if p.shared.folder.has_result() {
                    p.shared.wake_tick();
                }
                continue;
            }
            if action == Action::CopyDiagnostics {
                ui.set_clipboard(format_diagnostics_report(app));
                continue;
            }
            if action == Action::ExportReport {
                let contents = format_diagnostics_report(app);
                p.shared.folder.ask_save_report(contents, log_dir());
                if p.shared.folder.has_result() {
                    p.shared.wake_tick();
                }
                continue;
            }
            if action == Action::LogFolder {
                open_log_folder(&p);
                continue;
            }
            if let Some(event) = played(&action) {
                if let Some(player) = lock(&p.shared.player).as_ref() {
                    player.play(event);
                }
                continue;
            }
            // The browser names the focused part by its row: the rack's slot.
            if let Action::Load {
                library,
                preset,
                into,
            } = action
            {
                let into = into.and_then(|n| slots.get(n).copied());
                let load = Action::Load {
                    library,
                    preset,
                    into,
                };
                p.shared.request(Request::Browse(load));
                continue;
            }
            if matches!(
                action,
                Action::Star(..)
                    | Action::StarPreset(..)
                    | Action::Refresh
                    | Action::ScanMachine
                    | Action::CancelScan
                    | Action::Relink(_)
                    | Action::RemoveFolder(_)
                    | Action::NewParts { .. }
                    | Action::Theme(_)
                    | Action::Picked(_)
            ) {
                p.shared.request(Request::Browse(action));
                continue;
            }
            // The browser's part actions name the rack slot, not the row.
            let row = |n: usize| slots.get(n).copied();
            let slotted = match action {
                Action::Switch { part, preset } => {
                    row(part).map(|part| Action::Switch { part, preset })
                }
                Action::Step { part, by } => row(part).map(|part| Action::Step { part, by }),
                Action::Preset { part, ref edit } => row(part).map(|part| Action::Preset {
                    part,
                    edit: edit.clone(),
                }),
                Action::Replace {
                    part,
                    library,
                    preset,
                } => row(part).map(|part| Action::Replace {
                    part,
                    library,
                    preset,
                }),
                _ => None,
            };
            if let Some(action) = slotted {
                p.shared.request(Request::Browse(action));
                continue;
            }
            let slot = part_of(&action).and_then(|n| slots.get(n).copied());
            // A panel control moved: its host macro records the move,
            // taking the next free one the first time.
            if let (
                Action::Control {
                    part,
                    widget,
                    value,
                    cell: None,
                    ..
                },
                Some(slot),
            ) = (&action, slot)
            {
                let shown = app.parts.get(*part);
                let title = shown.map_or("", |s| s.name.as_str());
                let face = shown.and_then(|p| p.face.as_ref());
                let w = face.and_then(|f| f.tree.widgets.get(*widget));
                let touched = w.and_then(|w| crate::macros::touch(&p, slot, title, w, *value));
                if let Some((id, value)) = touched {
                    bridge.drag_gesture(id, value);
                    dragging = Some((id, value));
                }
            }
            let noted = slot.map(|s| (s, action.clone()));
            let seq = p.shared.request_ui(Edit {
                slot,
                action,
                seq: 0,
            });
            if let Some((slot, action)) = noted {
                let generation = slot_gens
                    .iter()
                    .find(|&&(s, _)| s == slot)
                    .map_or(0, |&(_, g)| g);
                mine.note(slot, generation, seq, &action);
            }
        }
        let pressed = (app.parts.iter()).any(|s| s.face.as_ref().is_some_and(|f| f.gesture.held()));
        match dragging {
            Some((id, value)) if pressed => bridge.drag_gesture(id, value),
            _ => dragging = None,
        }
        // Parts not loaded yet keep how they were saved.
        let mut now = crate::look::saved(app, &slots);
        now.parts.extend_from_slice(&waiting);
        if now != look {
            *p.look.write().unwrap_or_else(PoisonError::into_inner) = now.write();
            look = now;
        }
        response.el
    })
    // A resized window reflows: the browser and rack share the room, and
    // text and controls keep the interface scale.
    .fixed_zoom()
    .user_zoom(move |_| f64::from(zoomed.load(Ordering::Acquire)) / 100.)
    .resizable(MIN)
    .wheel_policy(wheel)
    .on_key(move |_, k| {
        use moose::mui::mui::host::NativeKey;
        if k.down && k.key == NativeKey::Named(moose::mui::mui::input::Key::Escape) {
            pressed.store(true, Ordering::Release);
        }
        false
    })
    .on_files(move |_, at, paths, dropped| {
        // A let go is kept over a move queued before it, and asks for a frame.
        let mut drag = lock(&dragging_files);
        if dropped || !drag.as_ref().is_some_and(|d| d.dropped) {
            *drag = Some(kontra_ui::input::FileDrag {
                at,
                paths: paths
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                dropped,
            });
        }
        drop(drag);
        dropping.shared.wake_editor();
        let library = paths.iter().any(|p| is_dropped(p));
        let instruments: Vec<_> = paths.iter().filter(|p| is_instrument(p)).cloned().collect();
        // A UVI bank holds many programs: it is added like a folder.
        let folders: Vec<_> = (paths.iter())
            .filter(|p| p.is_dir() || is_bank(p))
            .cloned()
            .collect();
        if dropped {
            for path in &folders {
                dropping.shared.request(Request::AddRoot(path.clone()));
                if is_bank(path) {
                    dropping.shared.request(Request::Notice(format!(
                        "{} is a bank of programs: it is added to the browser, load one from there",
                        path.display()
                    )));
                }
            }
            for path in &instruments {
                dropping.shared.request(Request::Load {
                    part: None,
                    path: path.clone(),
                });
            }
        }
        library || !instruments.is_empty() || !folders.is_empty()
    })
    // Interactive edits, script responses, and decoded pictures all wake the
    // window immediately via `EditorWaker`. Keep only a low-frequency fallback
    // poll for background library-scan progress while audio is suspended.
    .poll(std::time::Duration::from_millis(250))
    .changed({
        let previous = std::sync::atomic::AtomicU64::new(u64::MAX);
        let last_scan_wake = std::sync::atomic::AtomicU64::new(0);
        move || {
            // Keep UI requests, folder-picker completions, and active library
            // scans moving even when the host suspends audio processing.
            if watched.shared.has_pending_work() {
                watched.shared.wake_tick();
            } else if watched.shared.scanning.load(Ordering::Acquire) || watched.shared.loading() {
                let now = crate::macros::now();
                let prev = last_scan_wake.load(Ordering::Relaxed);
                if now.saturating_sub(prev) >= 100
                    && last_scan_wake
                        .compare_exchange(prev, now, Ordering::AcqRel, Ordering::Relaxed)
                        .is_ok()
                {
                    watched.shared.wake_tick();
                }
            }
            let progress = watched.shared.progress.snapshot();
            let visible = ((progress.stage as u64) << 56)
                | (u64::from(progress.percent) << 48)
                | (progress.bytes / 1_000_000).min((1 << 48) - 1);
            let changed =
                watched.shared.loading() && previous.swap(visible, Ordering::Relaxed) != visible;
            // Only events draw: a change the tick or the view marked, or a
            // picture that arrived. Nothing polled (pictures pending, a load
            // under way) asks for frames, so nothing stuck can spin them.
            changed
                | watched.shared.dirty.swap(false, Ordering::AcqRel)
                | kontra_ui::pictures::arrived()
        }
    })
    // Add folder's picker is owned by the host's window (Windows).
    .on_win32_parent(move |hwnd| owner.shared.folder.set_owner(hwnd))
    .on_cancel(move |_| {
        on_cancelled.store(true, Ordering::Release);
        cancel_params.shared.wake_editor();
    })
    .on_log(move |line| {
        eprintln!("[kontra editor] {line}");
        if line.starts_with("Wayland:") || line.contains("failed") || line.contains("error") {
            log_params
                .shared
                .request(Request::Notice(format!("Display: {line}")));
        }
    })
    .on_close(move || {
        closing.shared.shown.store(false, Ordering::Release);
        #[cfg(target_os = "linux")]
        closing.shared.folder.cancel();
    });
    #[cfg(target_os = "linux")]
    let editor = editor.on_x11_window(move |xid| x11_owner.shared.folder.set_x11_parent(xid));
    waker_params.shared.set_waker(Some(editor.waker()));
    editor.into_editor()
}

/// Directory where KONTRA writes and exports diagnostic logs.
fn log_dir() -> Option<PathBuf> {
    dirs::data_local_dir()
        .or_else(dirs::cache_dir)
        .map(|base| base.join("kontra-v3").join("logs"))
}

/// Formats a plain-text diagnostics report for [`Action::CopyDiagnostics`] and
/// [`Action::ExportReport`]. Includes latent developer diagnostics when the
/// Developer toggle is enabled.
fn format_diagnostics_report(app: &App) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "KONTRA Diagnostics Report");
    if !app.build.version.is_empty() {
        let b = &app.build;
        let _ = writeln!(
            out,
            "Build: {} · {} · built {} · {} · {}",
            b.version, b.revision, b.built, b.target, b.formats
        );
    }
    let _ = writeln!(
        out,
        "Parts loaded: {} · Voices: {} · RAM: {} MB",
        app.parts.len(),
        app.stats.voices,
        app.stats.ram_mb
    );
    if !app.parts.is_empty() {
        let _ = writeln!(out, "\nLoaded parts:");
        for (idx, part) in app.parts.iter().enumerate() {
            let _ = writeln!(out, "  {}. {} ({})", idx + 1, part.name, part.library);
            for need in &part.needs {
                let _ = writeln!(out, "     - needs: {need}");
            }
        }
    }
    let _ = writeln!(out, "\nDiagnostics ({}):", app.report.len());
    if app.report.is_empty() {
        let _ = writeln!(out, "  Everything loaded as written.");
    } else {
        for entry in &app.report {
            let _ = writeln!(out, "  - {entry}");
        }
    }
    let _ = writeln!(out, "\nLibrary scan ({} folders):", app.folders.len());
    if app.folders.is_empty() {
        let _ = writeln!(out, "  No library folder: none added and none found.");
    }
    for f in &app.folders {
        let auto = if f.auto { " [auto]" } else { "" };
        let _ = writeln!(out, "  - {}{auto}: {}", f.path, f.status);
        if let Some(hint) = &f.hint {
            let _ = writeln!(out, "      {hint}");
        }
    }
    if !app.scan_issues.is_empty() {
        let _ = writeln!(out, "  Could not read ({}):", app.scan_issues.len());
        for issue in &app.scan_issues {
            let _ = writeln!(out, "    - {issue}");
        }
    }
    if app.view.report_developer && !app.report_latent.is_empty() {
        let _ = writeln!(
            out,
            "\nDeveloper / Latent Diagnostics ({}):",
            app.report_latent.len()
        );
        for entry in &app.report_latent {
            let _ = writeln!(out, "  - {entry}");
        }
    }
    out
}

/// Opens KONTRA's log directory in the platform's file manager off the UI thread.
fn open_log_folder(params: &Arc<KontraParams>) {
    let Some(dir) = log_dir() else {
        params.shared.request(Request::Notice(
            "Could not locate the KONTRA log folder on this system.".into(),
        ));
        return;
    };
    let params = Arc::clone(params);
    let _ = std::thread::Builder::new()
        .name("kontra-log-folder".into())
        .spawn(move || {
            if let Err(err) = std::fs::create_dir_all(&dir) {
                params.shared.request(Request::Notice(format!(
                    "Could not create log folder {}: {err}",
                    dir.display()
                )));
                return;
            }
            #[cfg(target_os = "macos")]
            let mut cmd = std::process::Command::new("open");
            #[cfg(target_os = "windows")]
            let mut cmd = std::process::Command::new("explorer");
            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            let mut cmd = std::process::Command::new("xdg-open");
            if let Err(err) = cmd.arg(&dir).spawn() {
                params.shared.request(Request::Notice(format!(
                    "Could not open log folder {} ({err}).",
                    dir.display()
                )));
            }
        });
}

/// The port the editor's notes come from: no host port, so a host note-off
/// never ends a key the editor holds, nor the other way round.
const EDITOR_PORT: u8 = 255;

/// What the editor's keyboard and wheels play, on channel 1 of every part.
fn played(action: &Action) -> Option<Event> {
    Some(match *action {
        Action::Note(key, true) => Event::NoteOn {
            note: HostNote {
                port: EDITOR_PORT,
                channel: 0,
                key,
                id: -1,
                clap: false,
            },
            velocity: 100.0 / 127.0,
            tune: 0.0,
        },
        Action::Note(key, false) => Event::NoteOff(HostPattern {
            port: i32::from(EDITOR_PORT),
            channel: 0,
            key: i32::from(key),
            id: -1,
            clap: false,
        }),
        // -1..1 onto MIDI 1.0's 14-bit bend, centred on 8192.
        Action::Pitch(x) => {
            let x = x.clamp(-1.0, 1.0);
            let bend = (8192.0 + x * if x < 0.0 { 8192.0 } else { 8191.0 }).round() as u16;
            Event::midi1(0xE0, (bend & 127) as u8, (bend >> 7) as u8)
        }
        Action::Modulation(x) => Event::midi1(0xB0, 1, (x.clamp(0.0, 1.0) * 127.0).round() as u8),
        _ => return None,
    })
}

/// Whether `path` is a preset of an instrument rather than one: a Kontakt
/// snapshot or the player's own preset.
fn is_preset(path: &std::path::Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str());
    ext.is_some_and(|e| {
        e.eq_ignore_ascii_case("nksn") || e.eq_ignore_ascii_case(kontra_host::presets::EXTENSION)
    })
}

/// Whether a dropped file is one a library's mouse area may take: audio,
/// MIDI or a KSP array (KSP manual, `ui_mouse_area`).
// ponytail: accepted anywhere over the window; ask the face's areas if a
// host shows the copy cursor where nothing takes it.
fn is_dropped(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        ext.as_deref(),
        Some("wav" | "aif" | "aiff" | "ncw" | "mid" | "midi" | "nka")
    )
}

/// Whether a dropped file is a UVI bank (`.ufs`).
fn is_bank(path: &std::path::Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("ufs"))
}

/// Whether a dropped file is an instrument the rack loads: a Kontakt
/// instrument or snapshot, a UVI program in its bank, an SFZ file, a
/// DecentSampler preset, or a SoundFont 2 bank.
fn is_instrument(path: &std::path::Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    matches!(
        ext.as_deref(),
        Some("nki" | "nksn" | "uvip" | "sfz" | "dspreset" | "sf2" | "kontra")
    )
}

#[cfg(test)]
mod drag_probe;
#[cfg(test)]
mod memo_probe;

#[cfg(test)]
mod tests {
    use super::*;

    /// Closing the editor hands its state to the plugin and reopening takes
    /// it back: the face stays laid out, its pictures stay decoded, and no
    /// face is made again.
    #[test]
    fn the_editor_reopens_on_the_faces_it_closed_with() {
        let params = Arc::new(KontraParams::new());
        let tree = Interface::default();
        let faces = FACES.load(Ordering::Relaxed);
        {
            let mut kept = Kept::take(&params);
            kept.app.parts.push(Part {
                face: Some(face(&tree, None)),
                ..Part::default()
            });
            kept.shown.slots.push(0);
            kept.shown.parts.insert(0, (7, 3, None));
        }
        assert!(lock(&params.shared.editor).is_some(), "closing keeps it");
        let kept = Kept::take(&params);
        assert!(lock(&params.shared.editor).is_none());
        assert!(kept.app.parts.first().is_some_and(|p| p.face.is_some()));
        assert_eq!(kept.shown.slots, [0]);
        assert_eq!(kept.shown.parts.get(&0), Some(&(7, 3, None)));
        assert_eq!(FACES.load(Ordering::Relaxed), faces + 1, "made once");
        assert_eq!(kept.app.build.version, crate::BUILD.version);
    }

    #[test]
    fn a_new_scale_zooms_and_asks_the_window_to_follow() {
        let zoom = AtomicU32::new(100);
        let mut asked = None;
        rescale(&zoom, 125, (1000., 700.), |w, h| {
            asked = Some((w, h));
            false
        });
        // Refused or not, the scale holds: the layout reflows instead.
        assert_eq!(
            (zoom.load(Ordering::Acquire), asked),
            (125, Some((1250, 875)))
        );
        let mut again = false;
        rescale(&zoom, 125, (800., 560.), |_, _| {
            again = true;
            true
        });
        assert!(!again, "the same scale asks for no resize");
        assert_eq!((percent(0), percent(67), percent(7)), (100, 67, 100));
    }

    /// A drag's frames come faster than the rack's copy: a copy taken
    /// before the editor's last edit keeps the editor's value, and the
    /// rack's shows once the copy has carried that edit out.
    #[test]
    fn a_drag_never_springs_back_to_an_older_copy() {
        let part = |gain: f32| PartView {
            slot: 0,
            generation: 1,
            path: None,
            source: None,
            tree: None,
            patches: Vec::new(),
            revision: 0,
            applied_seq: 0,
            sounds: Default::default(),
            name: String::new(),
            library: String::new(),
            preset: None,
            modified: false,
            mix: kontra_host::rack::Mix {
                channel: 0,
                output: 0,
                gain,
                pan: 0.,
                silent: false,
            },
            solo: false,
            mute: false,
            level: Vec::new(),
            diag_count: 0,
            needs: Arc::default(),
            diag_visible: Arc::default(),
            diag_latent: Arc::default(),
        };
        let mut view = Snapshot {
            parts: vec![part(1.)],
            ..Snapshot::default()
        };
        let (mut app, mut shown) = (App::default(), Shown::default());
        view.fill(&mut app, &mut shown, &Mine::default());
        let drawn = |app: &App| app.parts.first().map(|p| p.volume.round());
        assert_eq!(drawn(&app), Some(0.));
        // The editor dragged to +6 dB and sent it as edit 1.
        if let Some(p) = app.parts.first_mut() {
            p.volume = 6.;
        }
        let mine = Mine {
            sent: HashMap::from([(0, 1)]),
            ..Mine::default()
        };
        view.fill(&mut app, &mut shown, &mine);
        assert_eq!(drawn(&app), Some(6.), "the copy predates the edit");
        view.applied = 1;
        view.parts = vec![part(db_to_linear(6.))];
        view.fill(&mut app, &mut shown, &mine);
        assert_eq!(drawn(&app), Some(6.));
        view.parts = vec![part(db_to_linear(-3.))];
        view.fill(&mut app, &mut shown, &mine);
        assert_eq!(drawn(&app), Some(-3.), "the rack's own change shows");
    }

    /// Rack slot 2 at load 7, its interface at `revision` and audio-acknowledged `applied_seq`.
    fn slot_two(revision: u64, applied_seq: u64) -> Vec<PartView> {
        vec![PartView {
            slot: 2,
            generation: 7,
            path: None,
            source: None,
            tree: None,
            patches: Vec::new(),
            revision,
            applied_seq,
            sounds: Default::default(),
            name: String::new(),
            library: String::new(),
            preset: None,
            modified: false,
            mix: Mix {
                channel: 0,
                output: 0,
                gain: 1.,
                pan: 0.,
                silent: false,
            },
            solo: false,
            mute: false,
            level: Vec::new(),
            diag_count: 0,
            needs: Arc::default(),
            diag_visible: Arc::default(),
            diag_latent: Arc::default(),
        }]
    }

    /// A library control the player drags keeps its last move over every
    /// copy of the rack for as long as the press lasts, however late the
    /// script answers; let go, it holds until the audio thread echoes
    /// `applied_seq >= seq` (or falls back after `HOLD` if no reply arrives).
    #[test]
    fn a_dragged_control_holds_until_the_script_answers_its_release() {
        let (mut mine, t) = (Mine::default(), Instant::now());
        let moved = |value| Action::Control {
            part: 0,
            widget: 3,
            cell: None,
            value,
            keys: Default::default(),
        };
        let shows = |mine: &Mine| {
            mine.held(2, 7)
                .iter()
                .map(|e| e.to.clone())
                .collect::<Vec<_>>()
        };
        mine.note(2, 7, 1, &moved(0.4));
        mine.note(2, 7, 2, &moved(0.5));
        assert_eq!(shows(&mine), [Set::Value(0.5)], "one entry per control");
        // Pressed: late copies, and copies that carried the move out, never
        // take it back, long after any timer.
        for (k, applied_seq) in [1, 2, 2, 2].into_iter().enumerate() {
            mine.keep(2, 7);
            let late = t + Duration::from_secs(10 * k as u64);
            mine.prune(&slot_two(10 + k as u64, applied_seq), 2, late);
            assert_eq!(shows(&mine), [Set::Value(0.5)], "copy {k} mid-drag");
        }
        // Let go, Tick enqueued seq=2, but audio has NOT executed seq=2 yet
        // (`applied_seq = 1 < 2`): hold stays active!
        mine.prune(&slot_two(13, 1), 2, t);
        assert_eq!(
            shows(&mine),
            [Set::Value(0.5)],
            "must hold until audio thread echoes applied_seq >= seq"
        );
        // Audio executes seq=2 and echoes `applied_seq = 2` in LookFrame: releases immediately!
        let released = mine.prune(&slot_two(14, 2), 2, t);
        assert!(shows(&mine).is_empty(), "audio ACK releases hold");
        assert_eq!(released, [(2, 3)]);
        // Suspended audio (no LookFrame reply): holds while Tick hasn't dispatched,
        // then falls back after HOLD once Tick dispatched it.
        mine.note(2, 7, 3, &moved(0.6));
        mine.prune(&slot_two(14, 2), 2, t + HOLD * 9);
        assert_eq!(
            shows(&mine),
            [Set::Value(0.6)],
            "not dispatched by Tick yet"
        );
        mine.prune(&slot_two(14, 2), 3, t);
        mine.prune(&slot_two(14, 2), 3, t + HOLD / 2);
        assert_eq!(shows(&mine), [Set::Value(0.6)]);
        mine.prune(&slot_two(14, 2), 3, t + HOLD);
        assert!(shows(&mine).is_empty(), "fallback timeout releases hold");
        // Another load of the part drops what the last one held.
        mine.note(2, 7, 4, &moved(0.7));
        assert!(mine.held(2, 8).is_empty() && mine.held(1, 7).is_empty());
        mine.prune(&[], 4, t);
        assert!(shows(&mine).is_empty());
    }

    #[test]
    fn instruments_and_banks_can_be_dropped_into_the_player() {
        assert!(super::is_instrument(std::path::Path::new("instrument.SFZ")));
        assert!(super::is_instrument(std::path::Path::new("piano.dspreset")));
        assert!(super::is_instrument(std::path::Path::new("bank.SF2")));
        assert!(super::is_bank(std::path::Path::new("Falcon.UFS")));
    }

    #[test]
    fn the_keyboard_and_wheels_play_midi() {
        let on = played(&Action::Note(60, true));
        assert!(matches!(on, Some(Event::NoteOn { note, .. }) if note.key == 60));
        assert!(matches!(played(&Action::Note(60, false)), Some(Event::NoteOff(p)) if p.key == 60));
        // Centre, top and bottom of the bend.
        let bend = |x| match played(&Action::Pitch(x)) {
            Some(Event::Ump([w, _])) => (w >> 8 & 127) | (w & 127) << 7,
            other => panic!("{other:?}"),
        };
        assert_eq!((bend(0.0), bend(1.0), bend(-1.0)), (8192, 16383, 0));
        // The mod wheel is CC 1, 0..127.
        let cc1 = |x| match played(&Action::Modulation(x)) {
            Some(Event::Ump([w, _])) => (w >> 16 & 0xff, w >> 8 & 127, w & 127),
            other => panic!("{other:?}"),
        };
        assert_eq!(cc1(0.0), (0xB0, 1, 0));
        assert_eq!(cc1(1.0), (0xB0, 1, 127));
        assert!(played(&Action::Panic).is_none());
        assert!(is_instrument(std::path::Path::new("/a/B.NKI")));
        assert!(!is_instrument(std::path::Path::new("/a/b.wav")));
    }

    #[test]
    fn snapshot_wires_part_needs_and_splits_latent_diagnostics() {
        use kontra_host::rack::{Mix, Rack, Store, StoreConfig};
        let (mut rack, _audio) = Rack::new(48_000, Store::new(StoreConfig::DEFAULT).unwrap(), 256);
        let mut inst = kontra_host::rack::ir::Instrument {
            name: "DiagTest".into(),
            ..kontra_host::rack::ir::Instrument::DEFAULT
        };
        inst.diagnostics = vec![
            kontra_host::rack::ir::Diagnostic {
                impact: kontra_host::rack::ir::Impact::Broken,
                feature: "samples not found".into(),
                detail: "2 samples not found in /lib\nfirst: /lib/a.wav".into(),
            },
            kontra_host::rack::ir::Diagnostic {
                impact: kontra_host::rack::ir::Impact::Audible,
                feature: "unsupported effect".into(),
                detail: "magic_fx".into(),
            },
            kontra_host::rack::ir::Diagnostic {
                impact: kontra_host::rack::ir::Impact::Cosmetic,
                feature: "unused knob".into(),
                detail: "cc 122".into(),
            },
            kontra_host::rack::ir::Diagnostic {
                impact: kontra_host::rack::ir::Impact::Latent,
                feature: "ignored group.midi_channel".into(),
                detail: "1× (first: 3)".into(),
            },
        ];
        rack.load_ir(0, inst, None).unwrap();
        let _ = Mix::DEFAULT;
        let browser = Browser::open(None, None);
        let mut snap = Snapshot::default();
        snap.refresh(&rack, &browser, vec!["Startup notice".into()]);
        let mut app = App {
            build: crate::BUILD,
            ..App::default()
        };
        let mut shown = Shown::default();
        snap.fill(&mut app, &mut shown, &Mine::default());

        assert_eq!(app.parts.len(), 1);
        assert_eq!(
            app.parts[0].needs,
            vec![
                "samples not found: 2 samples not found in /lib".to_owned(),
                "unsupported effect: magic_fx".to_owned(),
            ]
        );
        assert!(
            app.report
                .iter()
                .any(|l| l.contains("[Broken]") && l.contains("samples not found")),
            "{:?}",
            app.report
        );
        assert!(
            app.report.iter().any(|l| l.contains("[Cosmetic]")),
            "{:?}",
            app.report
        );
        assert!(
            !app.report.iter().any(|l| l.contains("[Latent]")),
            "Latent diagnostics must stay out of default app.report: {:?}",
            app.report
        );
        assert_eq!(app.report_latent.len(), 1);
        assert!(app.report_latent[0].contains("ignored group.midi_channel"));

        let default_txt = format_diagnostics_report(&app);
        assert!(default_txt.contains("samples not found"));
        assert!(!default_txt.contains("ignored group.midi_channel"));

        app.view.report_developer = true;
        let dev_txt = format_diagnostics_report(&app);
        assert!(dev_txt.contains("ignored group.midi_channel"));
    }

    /// The exported report says how each library folder scanned, with its
    /// hint, and what could not be read.
    #[test]
    fn the_report_carries_the_library_scan() {
        let app = App {
            folders: vec![
                app::LibraryFolder {
                    path: "/mnt/hdd/Kontakt".into(),
                    auto: true,
                    status: "can't read: permission denied".into(),
                    hint: Some("flatpak override --user --filesystem=/mnt/hdd/Kontakt x".into()),
                    problem: true,
                },
                app::LibraryFolder {
                    path: "/home/u/Libs".into(),
                    status: "2 libraries found".into(),
                    ..app::LibraryFolder::default()
                },
            ],
            scan_issues: vec!["/mnt/hdd/Kontakt: Permission denied (os error 13)".into()],
            ..App::default()
        };
        let txt = format_diagnostics_report(&app);
        assert!(txt.contains("Library scan (2 folders):"), "{txt}");
        assert!(txt.contains("/mnt/hdd/Kontakt [auto]: can't read: permission denied"));
        assert!(txt.contains("flatpak override --user --filesystem=/mnt/hdd/Kontakt x"));
        assert!(txt.contains("/home/u/Libs: 2 libraries found"));
        assert!(txt.contains("Could not read (1):"));
        let none = format_diagnostics_report(&App::default());
        assert!(none.contains("No library folder: none added and none found."));
    }

    /// AC5(b): Removing part 0 must preserve part 1's `open`, `tab`, `look`,
    /// `face`, and `Mine` holds without positional Vec-index aliasing.
    #[test]
    fn lifecycle_removing_part_zero_preserves_part_one_state_and_holds() {
        let make_part = |slot: usize, generation: u64, name: &str| {
            let mut tree = Interface::default();
            tree.widgets.push(kontra_ui_model::Widget::new(
                name,
                kontra_ui_model::PageRef(0),
                kontra_ui_model::Rect::new(0, 0, 40, 40),
                kontra_ui_model::Kind::Slider {
                    range: kontra_ui_model::Range {
                        min: 0.,
                        max: 100.,
                        default: 0.,
                        step: Some(1.),
                    },
                    orientation: kontra_ui_model::Orientation::Vertical,
                },
            ));
            PartView {
                slot,
                generation,
                path: None,
                source: None,
                tree: Some(tree),
                patches: Vec::new(),
                revision: 0,
                applied_seq: 0,
                sounds: Default::default(),
                name: name.to_owned(),
                library: String::new(),
                preset: None,
                modified: false,
                mix: Mix::DEFAULT,
                solo: false,
                mute: false,
                level: Vec::new(),
                diag_count: 0,
                needs: Arc::default(),
                diag_visible: Arc::default(),
                diag_latent: Arc::default(),
            }
        };
        let mut snap = Snapshot {
            parts: vec![make_part(0, 10, "$a"), make_part(1, 20, "$b")],
            ..Snapshot::default()
        };
        let (mut app, mut shown, mut mine) = (App::default(), Shown::default(), Mine::default());
        snap.fill(&mut app, &mut shown, &mine);
        assert_eq!(app.parts.len(), 2);
        app.parts[0].open = false;
        app.parts[0].tab = app::PartTab::Sound;
        app.parts[0].look = Some(kontra_ui_model::Presentation::Kontra);
        app.parts[1].open = true;
        app.parts[1].tab = app::PartTab::Mapping;
        app.parts[1].look = Some(kontra_ui_model::Presentation::Original);
        mine.note(
            0,
            10,
            1,
            &Action::Control {
                part: 0,
                widget: 0,
                cell: None,
                value: 25.,
                keys: Default::default(),
            },
        );
        mine.note(
            1,
            20,
            2,
            &Action::Control {
                part: 1,
                widget: 0,
                cell: None,
                value: 75.,
                keys: Default::default(),
            },
        );
        let part1_widgets_ptr = app.parts[1]
            .face
            .as_ref()
            .expect("part 1 face")
            .tree
            .widgets
            .as_ptr();
        // Remove part 0; part 1 shifts from Vec index 1 to Vec index 0.
        snap.parts.remove(0);
        let released = mine.prune(&snap.parts, 0, Instant::now());
        snap.fill_with(&mut app, &mut shown, &mine, &released, None);
        assert_eq!(app.parts.len(), 1);
        assert_eq!(shown.slots, [1]);
        assert!(app.parts[0].open, "part 1 open state preserved");
        assert_eq!(
            app.parts[0].tab,
            app::PartTab::Mapping,
            "part 1 tab preserved (not overwritten by part 0's Sound tab)"
        );
        assert_eq!(
            app.parts[0].look,
            Some(kontra_ui_model::Presentation::Original),
            "part 1 look preserved"
        );
        assert_eq!(
            app.parts[0]
                .face
                .as_ref()
                .expect("part 1 face")
                .tree
                .widgets
                .as_ptr(),
            part1_widgets_ptr,
            "part 1 kept its existing face without rebuilding"
        );
        assert!(mine.held(0, 10).is_empty(), "removed part 0 hold pruned");
        assert_eq!(
            mine.held(1, 20).len(),
            1,
            "part 1 held drag edit preserved across part 0 removal"
        );
        assert_eq!(
            app.parts[0]
                .face
                .as_ref()
                .map(|f| f.tree.widgets[0].name.as_str()),
            Some("$b")
        );
    }

    /// AC4: Single-slider drags patch only the changed widgets in place with
    /// zero full-tree `layout::resolve(0..N)` passes (`RESOLVES` delta = 0).
    #[test]
    fn lifecycle_incremental_drag_performs_zero_full_resolves() {
        use kontra_host::rack::{Rack, Store, StoreConfig, ir};
        let store = Store::new(StoreConfig::DEFAULT).unwrap();
        let (mut rack, mut audio) = Rack::new(48_000, store, 256);
        let mut inst = ir::Instrument {
            name: "DragInc".into(),
            ..ir::Instrument::DEFAULT
        };
        inst.scripts.push(ir::Script {
            name: "main".into(),
            slot: 0,
            dialect: ir::Dialect::Ksp,
            source: "on init
                make_perfview
                declare ui_slider $s (0, 100)
                declare ui_label $lbl (1, 1)
                declare ui_knob $k1 (0, 100, 1)
                declare ui_knob $k2 (0, 100, 1)
                declare ui_knob $k3 (0, 100, 1)
                declare ui_knob $k4 (0, 100, 1)
                declare ui_knob $k5 (0, 100, 1)
                declare ui_knob $k6 (0, 100, 1)
                declare ui_knob $k7 (0, 100, 1)
                declare ui_knob $k8 (0, 100, 1)
                set_text($lbl, \"0\")
            end on
            on ui_control ($s)
                set_text($lbl, $s)
            end on"
                .into(),
            saved: Vec::new(),
            data: Vec::new(),
            links: Vec::new(),
            container: None,
            player: false,
        });
        rack.load_ir(0, inst, None).unwrap();
        for _ in 0..2 {
            rack.tick();
            audio.process(64, &[]);
        }
        rack.tick();
        let browser = Browser::open(None, None);
        let (mut snap, mut app, mut shown, mine) = (
            Snapshot::default(),
            App::default(),
            Shown::default(),
            Mine::default(),
        );
        snap.refresh(&rack, &browser, Vec::new());
        snap.fill(&mut app, &mut shown, &mine);
        let resolves_before = RESOLVES.load(Ordering::Relaxed);
        let slider = app.parts[0]
            .face
            .as_ref()
            .unwrap()
            .tree
            .widgets
            .iter()
            .position(|w| w.name == "$s")
            .unwrap();
        let label = app.parts[0]
            .face
            .as_ref()
            .unwrap()
            .tree
            .widgets
            .iter()
            .position(|w| w.name == "$lbl")
            .unwrap();
        for step in 1..=5u64 {
            rack.control_with_seq(
                0,
                slider,
                None,
                (step * 15) as f64,
                Default::default(),
                step,
            );
            rack.tick();
            audio.process(64, &[]);
            assert!(
                audio.take_wake(),
                "audio must wake Tick on LookFrame return"
            );
            rack.tick();
            snap.refresh(&rack, &browser, Vec::new());
            snap.fill(&mut app, &mut shown, &mine);
            assert_eq!(snap.parts[0].applied_seq, step);
            let tree = &app.parts[0].face.as_ref().unwrap().tree;
            assert_eq!(tree.widgets[slider].number(), (step * 15) as f64);
            assert_eq!(tree.widgets[label].text, format!("{}", step * 15));
        }
        assert_eq!(
            RESOLVES.load(Ordering::Relaxed),
            resolves_before,
            "single-slider drags must perform 0 full-tree layout::resolve passes"
        );
    }
}
