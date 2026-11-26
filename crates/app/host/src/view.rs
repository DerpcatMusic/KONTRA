//! A part's library interface on the control side: the tree the editor
//! shows, built once the part's `on init` ran, kept current with what the
//! script changes since ([`LookFrame`]), and how a widget the user moves
//! reaches the script that owns it. A KSP script's tree is built from its
//! controls; a Lua script's from the page it made ([`crate::ui::lua`]).

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use kontra_engine::{ControlLook, Engine, FIRST_FONT, Key, MenuItem, Page, Text, WidgetLook};
use kontra_ui_model as model;

use crate::ui::lua::Page as _;
use crate::ui::{Changes, Face, Library, Live, Looks, Waves};

/// Maximum incremental [`TreePatch`]es kept per [`View`] before a lagging
/// reader falls back to a full tree copy.
const MAX_PATCHES: usize = 16;

/// Keyboard/status fields updated together when `Changes::keys` fires.
#[derive(Clone, Debug)]
pub struct KeyState {
    pub keybed: Vec<(u8, model::Rgba)>,
    pub key_names: Vec<(u8, String)>,
    pub key_pressed: Option<Vec<u8>>,
    pub message: String,
}

/// Widget-level diff carried by a [`TreePatch`].
#[derive(Clone, Debug)]
pub enum WidgetPatch {
    /// Same widget count: replace only these `(index, widget)` entries in place.
    InPlace(Vec<(usize, model::Widget)>),
    /// Structural widget change (length changed or >50% of widgets rebuilt).
    ReplaceAll(Arc<model::Interface>),
}

/// Incremental update to a part's [`model::Interface`] between consecutive
/// revisions so the editor updates only changed widgets without cloning or
/// re-resolving the whole tree.
#[derive(Clone, Debug)]
pub struct TreePatch {
    pub from_revision: u64,
    pub to_revision: u64,
    pub page: Option<Vec<model::Page>>,
    pub paint_order: Option<Option<Vec<model::WidgetRef>>>,
    pub keys: Option<KeyState>,
    pub assets: Vec<model::Asset>,
    pub styles: Vec<model::TextStyle>,
    pub widgets: WidgetPatch,
}

impl TreePatch {
    /// Applies this patch onto `tree`. Returns `Some(changed_widget_indices)`
    /// for incremental layout resolution, or `None` when the entire tree was
    /// replaced and needs a full layout resolve.
    pub fn apply(&self, tree: &mut model::Interface) -> Option<Vec<usize>> {
        match &self.widgets {
            WidgetPatch::ReplaceAll(full) => {
                *tree = (**full).clone();
                None
            }
            WidgetPatch::InPlace(updates) => {
                if !self.assets.is_empty() {
                    tree.assets.extend(self.assets.iter().cloned());
                }
                if !self.styles.is_empty() {
                    tree.styles.extend(self.styles.iter().cloned());
                }
                if let Some(pages) = &self.page {
                    tree.pages.clone_from(pages);
                }
                if let Some(order) = &self.paint_order {
                    tree.paint_order.clone_from(order);
                }
                if let Some(keys) = &self.keys {
                    tree.keybed.clone_from(&keys.keybed);
                    tree.key_names.clone_from(&keys.key_names);
                    tree.key_pressed.clone_from(&keys.key_pressed);
                    tree.message.clone_from(&keys.message);
                }
                let mut changed = Vec::with_capacity(updates.len());
                for (idx, widget) in updates {
                    if let Some(slot) = tree.widgets.get_mut(*idx) {
                        *slot = widget.clone();
                        changed.push(*idx);
                    }
                }
                Some(changed)
            }
        }
    }
}

/// The interface of a part's first script with one.
pub struct View {
    pub slot: u8,
    shown: Shown,
    /// Moves whenever [`View::refresh`] changed the tree.
    pub revision: u64,
    /// Highest UI command sequence acknowledged by the audio thread for this
    /// part (`LookFrame.applied_seq`), or applied directly for `Binding::Param`.
    pub applied_seq: u64,
    /// Recent incremental patches leading up to `revision`.
    patches: VecDeque<Arc<TreePatch>>,
    /// The frame between trips to the audio side; `None` while it is there.
    pub(crate) frame: Option<Box<LookFrame>>,
    /// The library's resources, opened once and kept for later pictures.
    library: Option<Library>,
    /// The zone samples its waveform displays show.
    pub(crate) waves: Waves,
}

/// A KSP script's tree and what keeps it current, a Lua script's tree, or a
/// script-free interface driven directly by [`model::Binding::Param`].
enum Shown {
    Ksp(Box<Live>),
    Lua(Box<model::Interface>),
    Direct(Box<model::Interface>),
}

/// Whether script slot `slot` asked for its page to be the instrument's
/// performance view (`make_perfview`, a loaded Creator Tools view, or a Lua
/// page with `performance`).
fn performs(
    faces: &[Option<Face>],
    pages: &[Option<kontra_lua::Ui>],
    engine: &Engine,
    slot: usize,
) -> bool {
    let Ok(at) = u8::try_from(slot) else {
        return false;
    };
    if let Some(Some(face)) = faces.get(slot) {
        return engine.page(at).is_some_and(|page| page.performance) || face.has_view();
    }
    let now = (engine.vm(at))
        .and_then(kontra_engine::Vm::own_page)
        .and_then(|p| p.downcast::<kontra_lua::Ui>().ok());
    now.as_deref()
        .or_else(|| pages.get(slot)?.as_ref())
        .is_some_and(|page| page.performance)
}

/// How strongly slot `slot` claims the editor: 0 for the instrument's own
/// performance view, 1 for one a multi's folded MIDI player asks for (its
/// page is that player's part's, as Conflux's Chords tool in front of the
/// synth), 2 for a slot that asks for none.
fn rank(
    faces: &[Option<Face>],
    pages: &[Option<kontra_lua::Ui>],
    engine: &Engine,
    slot: usize,
) -> u8 {
    let player = matches!(faces.get(slot), Some(Some(face)) if face.player);
    match (performs(faces, pages, engine, slot), player) {
        (true, false) => 0,
        (true, true) => 1,
        (false, _) => 2,
    }
}

/// The slots to try, in the order the editor chooses between them: by
/// [`rank`], slot order within a rank.
///
/// Returning the order as data is what keeps the failure out of reach. A slot
/// that asks for the view but whose page cannot be built is skipped by the
/// caller like any other candidate, so it cannot end the search and hand the
/// editor an earlier admin page -- which `find(..).and_then(..)` did, because
/// the failed construction was the end of the only preferred candidate.
fn candidates(count: usize, rank: impl Fn(usize) -> u8) -> Vec<usize> {
    let mut order: Vec<usize> = (0..count).collect();
    order.sort_by_key(|&slot| rank(slot));
    order
}

impl View {
    /// The script's page the editor shows, read from `engine` as its init
    /// left it; pictures come from the library at `path`.
    ///
    /// Kontakt shows the script that asked for the performance view
    /// (`make_perfview`, or a Creator Tools view it loaded), not whichever
    /// slot declares a control first: a multi whose first slot is a helper's
    /// admin page would otherwise open with that page instead of the
    /// instrument; nor does a multi's folded MIDI player's view win over the
    /// instrument's own. Only when no slot asks for it does the first slot with a
    /// face (KSP) or a page (Lua) win, in declaration order.
    #[must_use]
    pub fn of(
        faces: &[Option<Face>],
        pages: &[Option<kontra_lua::Ui>],
        engine: &Engine,
        path: Option<&Path>,
    ) -> Option<Self> {
        let library = path.map(|p| RefCell::new(Library::open(p)));
        let meta = |p: &str| library.as_ref()?.borrow_mut().meta(p);
        let count = faces.len().max(pages.len());
        let view = {
            let mut view = candidates(count, |slot| rank(faces, pages, engine, slot))
                .into_iter()
                .find_map(|slot| Self::at_slot(slot, faces, pages, engine, &meta))?;
            view.library = library.map(RefCell::into_inner);
            view
        };
        Some(view)
    }

    /// Gives the page's waveform displays the instrument's zone samples
    /// (`attach_zone`); they draw empty until it has them.
    #[must_use]
    pub fn with_waves(mut self, mut waves: Waves) -> Self {
        if let Shown::Ksp(live) = &mut self.shown {
            waves.fill(&mut live.tree);
        }
        self.waves = waves;
        self
    }

    /// Script slot `slot`'s page, or `None` when it has none or the tree it
    /// declares is malformed.
    fn at_slot(
        slot: usize,
        faces: &[Option<Face>],
        pages: &[Option<kontra_lua::Ui>],
        engine: &Engine,
        meta: &dyn Fn(&str) -> Option<model::ImageMeta>,
    ) -> Option<Self> {
        let at = u8::try_from(slot).ok()?;
        if let Some(Some(face)) = faces.get(slot) {
            let live = Live::new(face, at, engine, meta).ok()?;
            let frame = LookFrame::new(at, &live, face, engine);
            return Some(Self::new(at, Shown::Ksp(Box::new(live)), frame));
        }
        // As init left it (pictures it set there), else as it loaded.
        let now = (engine.vm(at))
            .and_then(kontra_engine::Vm::own_page)
            .and_then(|p| p.downcast::<kontra_lua::Ui>().ok());
        let page = now.as_deref().or(pages.get(slot)?.as_ref())?;
        let mut tree = crate::ui::lua::tree(page, meta);
        let mut text = Text::EMPTY;
        for (n, w) in (0u32..).zip(0..tree.widgets.len()) {
            let Some(look) = engine.widget(at, n) else {
                continue;
            };
            let label = tree.widgets.get(w).is_some_and(|w| worded(&w.kind));
            let words = label && engine.widget_text(at, n, &mut text);
            crate::ui::lua::apply(&mut tree, w, look, words.then(|| text.as_str()));
        }
        tree.keybed = crate::ui::lua::keybed(engine.keys());
        let frame = LookFrame::lua(at, &tree);
        Some(Self::new(at, Shown::Lua(Box::new(tree)), frame))
    }

    /// A script-free interface whose widgets bind directly to IR parameters
    /// ([`model::Binding::Param`]) without any script VM round-trip.
    #[must_use]
    pub fn direct(tree: model::Interface) -> Self {
        Self {
            slot: 0,
            shown: Shown::Direct(Box::new(tree)),
            revision: 0,
            applied_seq: 0,
            patches: VecDeque::new(),
            frame: None,
            library: None,
            waves: Waves::default(),
        }
    }

    fn new(slot: u8, shown: Shown, frame: Box<LookFrame>) -> Self {
        Self {
            slot,
            shown,
            revision: 0,
            applied_seq: 0,
            patches: VecDeque::new(),
            frame: Some(frame),
            library: None,
            waves: Waves::default(),
        }
    }

    /// Returns the contiguous incremental patches from `revision` to
    /// `self.revision`, or `None` if `revision` fell behind the patch window.
    #[must_use]
    pub fn patches_since(&self, revision: u64) -> Option<Vec<Arc<TreePatch>>> {
        if revision == self.revision {
            return Some(Vec::new());
        }
        let first = self.patches.front()?;
        if revision < first.from_revision || revision > self.revision {
            return None;
        }
        let mut expected = revision;
        let mut out = Vec::new();
        for p in &self.patches {
            if p.to_revision <= revision {
                continue;
            }
            if p.from_revision != expected {
                return None;
            }
            expected = p.to_revision;
            out.push(Arc::clone(p));
        }
        (expected == self.revision).then_some(out)
    }

    fn push_patch(&mut self, patch: TreePatch) {
        if self.patches.len() >= MAX_PATCHES {
            self.patches.pop_front();
        }
        self.patches.push_back(Arc::new(patch));
    }

    /// The tree the editor shows.
    #[must_use]
    pub fn tree(&self) -> &model::Interface {
        match &self.shown {
            Shown::Ksp(live) => &live.tree,
            Shown::Lua(tree) | Shown::Direct(tree) => tree,
        }
    }

    fn tree_mut(&mut self) -> &mut model::Interface {
        match &mut self.shown {
            Shown::Ksp(live) => &mut live.tree,
            Shown::Lua(tree) | Shown::Direct(tree) => tree,
        }
    }

    /// Brings the tree up to `frame` (filled on the audio side), and keeps
    /// the frame for the next trip, grown if the script outgrew it.
    pub(crate) fn refresh(
        &mut self,
        faces: &[Option<Face>],
        path: Option<&Path>,
        frame: Box<LookFrame>,
    ) {
        if frame.state == Filled::Whole {
            self.applied_seq = self.applied_seq.max(frame.applied_seq);
        }
        let live = match &mut self.shown {
            Shown::Ksp(live) => live,
            Shown::Lua(tree) => {
                let mut frame = frame;
                if frame.state == Filled::Whole {
                    let old_assets = tree.assets.len();
                    let old_styles = tree.styles.len();
                    let changed = frame.apply(tree);
                    let keybed = crate::ui::lua::keybed(&frame.keys);
                    let keys = (keybed != tree.keybed).then(|| {
                        tree.keybed = keybed;
                        KeyState {
                            keybed: tree.keybed.clone(),
                            key_names: tree.key_names.clone(),
                            key_pressed: tree.key_pressed.clone(),
                            message: tree.message.clone(),
                        }
                    });
                    if !changed.is_empty() || keys.is_some() {
                        let from = self.revision;
                        self.revision += 1;
                        let to = self.revision;
                        let widgets = if changed.len() <= tree.widgets.len() / 2 + 1 {
                            WidgetPatch::InPlace(
                                changed
                                    .into_iter()
                                    .filter_map(|i| Some((i, tree.widgets.get(i)?.clone())))
                                    .collect(),
                            )
                        } else {
                            WidgetPatch::ReplaceAll(Arc::new((**tree).clone()))
                        };
                        let assets = tree.assets.get(old_assets..).unwrap_or_default().to_vec();
                        let styles = tree.styles.get(old_styles..).unwrap_or_default().to_vec();
                        self.push_patch(TreePatch {
                            from_revision: from,
                            to_revision: to,
                            page: None,
                            paint_order: None,
                            keys,
                            assets,
                            styles,
                            widgets,
                        });
                    }
                }
                self.frame = Some(frame.kept());
                return;
            }
            Shown::Direct(_) => return,
        };
        let Some(face) = faces.get(usize::from(self.slot)).and_then(Option::as_ref) else {
            return;
        };
        let library = RefCell::new(self.library.take());
        // Opened only if a changed widget shows a picture, then kept.
        let meta = |p: &str| {
            let path = path?;
            let mut l = library.borrow_mut();
            l.get_or_insert_with(|| Library::open(path)).meta(p)
        };
        // An overflowed frame has stale lists under new generations: the
        // tree waits for the grown one rather than remember them as seen.
        let mut next_patch = None;
        if frame.state == Filled::Whole {
            let old_len = live.tree.widgets.len();
            let old_assets = live.tree.assets.len();
            let old_styles = live.tree.styles.len();
            let old_order_len = live.tree.paint_order.as_ref().map(Vec::len);
            let changes: Changes = live.refresh(face, self.slot, &*frame, &meta);
            if !changes.is_empty() {
                self.waves.fill(&mut live.tree);
                let from = self.revision;
                self.revision += 1;
                let to = self.revision;
                let mut indices: Vec<usize> = changes.widgets.iter().map(|w| w.0).collect();
                indices.sort_unstable();
                indices.dedup();
                let same_len = live.tree.widgets.len() == old_len;
                let small_diff = same_len && indices.len() <= old_len / 2 + 1;
                let widgets = if small_diff {
                    WidgetPatch::InPlace(
                        indices
                            .into_iter()
                            .filter_map(|i| Some((i, live.tree.widgets.get(i)?.clone())))
                            .collect(),
                    )
                } else {
                    WidgetPatch::ReplaceAll(Arc::new(live.tree.clone()))
                };
                let paint_order = (live.tree.paint_order.as_ref().map(Vec::len) != old_order_len
                    || !same_len)
                    .then(|| live.tree.paint_order.clone());
                let keys = changes.keys.then(|| KeyState {
                    keybed: live.tree.keybed.clone(),
                    key_names: live.tree.key_names.clone(),
                    key_pressed: live.tree.key_pressed.clone(),
                    message: live.tree.message.clone(),
                });
                next_patch = Some(TreePatch {
                    from_revision: from,
                    to_revision: to,
                    page: changes.page.then(|| live.tree.pages.clone()),
                    paint_order,
                    keys,
                    assets: live
                        .tree
                        .assets
                        .get(old_assets..)
                        .unwrap_or_default()
                        .to_vec(),
                    styles: live
                        .tree
                        .styles
                        .get(old_styles..)
                        .unwrap_or_default()
                        .to_vec(),
                    widgets,
                });
            }
        }
        if let Some(patch) = next_patch {
            self.push_patch(patch);
        }
        self.frame = Some(frame.kept());
        self.library = library.into_inner();
    }

    /// The script's own widget behind `widget` on a page a front end built
    /// itself (a Lua page), as (slot, its number); None on a control page.
    #[must_use]
    pub fn widget(&self, widget: usize) -> Option<(u8, u32)> {
        let Shown::Lua(_) = self.shown else {
            return None;
        };
        let id = self.tree().widgets.get(widget)?.source_id?;
        Some((self.slot, u32::try_from(id).ok()?))
    }

    /// The user pressed (`value` 1) or let go (0) of `widget`, a native_ui
    /// page's tap area: the controls its page set, as (slot, UI id,
    /// value); `None` for any other widget.
    pub fn tap(&mut self, widget: usize, value: f64) -> Option<Vec<(u8, i32, i64)>> {
        let Shown::Ksp(live) = &mut self.shown else {
            return None;
        };
        let slot = self.slot;
        let set = live.tap(widget, value)?;
        let updated = live.tree.widgets.get(widget).cloned();
        let from = self.revision;
        self.revision += 1;
        if let Some(w) = updated {
            self.push_patch(TreePatch {
                from_revision: from,
                to_revision: self.revision,
                page: None,
                paint_order: None,
                keys: None,
                assets: Vec::new(),
                styles: Vec::new(),
                widgets: WidgetPatch::InPlace(vec![(widget, w)]),
            });
        }
        Some(set.into_iter().map(|(id, v)| (slot, id, v)).collect())
    }

    /// Direct IR parameter binding behind `widget`, mapped to its physical
    /// value; `None` for script-backed or unbound widgets.
    #[must_use]
    pub fn param(
        &self,
        widget: usize,
        value: f64,
    ) -> Option<(kontra_engine::ir::Scope, kontra_engine::ir::ParamAddr, f32)> {
        let w = self.tree().widgets.get(widget)?;
        let model::Binding::Param(binding) = w.binding else {
            return None;
        };
        Some((
            binding.scope,
            binding.addr,
            binding.to_physical(value, w.range()),
        ))
    }

    /// Stores a direct parameter edit and returns the mapping of the stored
    /// value, so the engine and the editor agree on integer controls.
    pub(crate) fn apply_param_value(
        &mut self,
        widget: usize,
        value: f64,
    ) -> Option<(kontra_engine::ir::Scope, kontra_engine::ir::ParamAddr, f32)> {
        let w = self.tree().widgets.get(widget)?;
        let (next, value) = match w.value {
            Some(model::Value::Integer(_)) => {
                let value = value.round() as i32;
                (model::Value::Integer(value), f64::from(value))
            }
            _ => (model::Value::Real(value), value),
        };
        let param = self.param(widget, value)?;
        let updated = {
            let w = self.tree_mut().widgets.get_mut(widget)?;
            if w.value.as_ref() != Some(&next) {
                w.value = Some(next);
                Some(w.clone())
            } else {
                None
            }
        };
        if let Some(w) = updated {
            let from = self.revision;
            self.revision += 1;
            self.push_patch(TreePatch {
                from_revision: from,
                to_revision: self.revision,
                page: None,
                paint_order: None,
                keys: None,
                assets: Vec::new(),
                styles: Vec::new(),
                widgets: WidgetPatch::InPlace(vec![(widget, w)]),
            });
        }
        Some(param)
    }

    /// The script control behind `widget`, as (slot, UI id), and `value`
    /// as the script's integer; None for a widget no control owns.
    #[must_use]
    pub fn control(&self, widget: usize, value: f64) -> Option<(u8, i32, i64)> {
        let w = self.tree().widgets.get(widget)?;
        let id = w.source_id?;
        let value = match w.value {
            Some(model::Value::Real(_) | model::Value::Reals(_)) => value.to_bits() as i64,
            _ => value.round() as i64,
        };
        Some((self.slot, id, value))
    }
}

/// One control's look as the audio side last copied it. Its lists keep
/// their room between trips: the audio side never allocates.
struct Look {
    id: i32,
    present: bool,
    value: Option<i64>,
    generation: u32,
    pars: Vec<(i32, i32)>,
    texts: Vec<(i32, Text)>,
    menu: Vec<MenuItem>,
}

/// A widget number, whether its array was read, and its preallocated cells.
type ArrayRead = (u32, bool, Box<[f64]>);

/// A Lua menu's items as last copied: the widget, the revision read (none
/// until copied, or after the room grew), whether this trip copied them,
/// and the items in their preallocated room.
#[derive(Clone)]
struct MenuRead {
    n: u32,
    revision: Option<u32>,
    fresh: bool,
    items: Vec<Text>,
}

/// What a part's [`Live`] reads of its script, copied out of the engine on
/// the audio thread ([`LookFrame::fill`]) for the control side to refresh
/// the tree from. Its ids and cells are sorted.
pub struct LookFrame {
    pub(crate) part: usize,
    pub(crate) generation: u64,
    /// Highest UI command sequence executed by the audio thread before this
    /// frame was filled.
    pub(crate) applied_seq: u64,
    /// Audio-side `ui_epoch` latched when this frame was filled.
    pub(crate) ui_epoch: u64,
    slot: u8,
    looks: Box<[Look]>,
    cells: Box<[(u32, Option<i64>)]>,
    /// Text cells (`@` controls), copied in place.
    strings: Box<[(u32, Text)]>,
    page: Option<Page>,
    /// A Lua page's widgets by number, as read this trip; the ones read
    /// (sorted), and the labels' texts.
    widgets: Box<[Option<WidgetLook>]>,
    reads: Vec<u32>,
    texts: Box<[(u32, bool, Text)]>,
    /// The pictured widgets' `image` paths: a script swaps them at run time.
    images: Box<[(u32, bool, Text)]>,
    arrays: Box<[ArrayRead]>,
    menus: Box<[MenuRead]>,
    /// The script's bitmap fonts from [`FIRST_FONT`] on; `on init` names
    /// them all, so they are copied once.
    fonts: Arc<[String]>,
    /// The keyboard's keys, by note (a fixed 128: copied in place).
    keys: Vec<Key>,
    pressed_support: bool,
    /// The last `message` a script printed.
    message: Text,
    state: Filled,
}

/// How a [`LookFrame`] came back from the audio side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Filled {
    /// Not filled: new, or the part was gone or reloaded when it arrived.
    No,
    Whole,
    /// A list outgrew its room: some look is stale, grow and ask again.
    Overflowed,
}

/// A new frame's lists hold this many times what the script set so far,
/// and at least [`MIN_ROOM`]: more room, fewer trips that overflow and
/// grow, against more memory per control.
const ROOM: usize = 2;
const MIN_ROOM: usize = 4;

impl LookFrame {
    /// A frame for `live`'s reads, with [`ROOM`] times what `now` holds.
    fn new(slot: u8, live: &Live, face: &Face, now: &dyn Looks) -> Box<Self> {
        let (mut ids, mut cells, mut strings) = live.reads(face);
        strings.sort_unstable();
        strings.dedup();
        ids.sort_unstable();
        ids.dedup();
        cells.sort_unstable();
        cells.dedup();
        let looks = ids
            .into_iter()
            .map(|id| {
                let l = now.control_look(slot, id);
                let len = |n: usize| (n * ROOM).max(MIN_ROOM);
                Look {
                    id,
                    present: false,
                    value: None,
                    generation: 0,
                    pars: Vec::with_capacity(len(l.as_ref().map_or(0, |l| l.pars.len()))),
                    texts: Vec::with_capacity(len(l.as_ref().map_or(0, |l| l.texts.len()))),
                    menu: Vec::with_capacity(len(l.as_ref().map_or(0, |l| l.menu.len()))),
                }
            })
            .collect();
        Box::new(Self {
            part: 0,
            generation: 0,
            applied_seq: 0,
            ui_epoch: 0,
            slot,
            looks,
            cells: cells.into_iter().map(|c| (c, None)).collect(),
            strings: strings.into_iter().map(|c| (c, Text::EMPTY)).collect(),
            page: None,
            widgets: Box::default(),
            reads: Vec::new(),
            texts: Box::default(),
            images: Box::default(),
            arrays: Box::default(),
            menus: Box::default(),
            fonts: (FIRST_FONT..)
                .map_while(|id| now.font(slot, id).map(str::to_owned))
                .collect(),
            keys: now.keys().to_vec(),
            pressed_support: now.pressed_support(),
            message: Text::from_str_cut(now.message()).0,
            state: Filled::No,
        })
    }

    /// A frame for a Lua page's widgets, and its labels' and menus' texts
    /// (a script renames a menu's items).
    fn lua(slot: u8, tree: &model::Interface) -> Box<Self> {
        let labels = (tree.widgets.iter().enumerate())
            .filter(|(_, w)| worded(&w.kind) || matches!(w.kind, model::Kind::Menu { .. }))
            .filter_map(|(n, _)| Some((u32::try_from(n).ok()?, false, Text::EMPTY)));
        Box::new(Self {
            part: 0,
            generation: 0,
            applied_seq: 0,
            ui_epoch: 0,
            slot,
            looks: Box::default(),
            cells: Box::default(),
            strings: Box::default(),
            page: None,
            widgets: vec![None; tree.widgets.len()].into(),
            reads: readable(tree),
            texts: labels.collect(),
            images: (0u32..)
                .zip(&tree.widgets)
                .filter(|(_, w)| w.images.iter().any(|i| i.role == model::Role::Background))
                .map(|(n, _)| (n, false, Text::EMPTY))
                .collect(),
            arrays: tree
                .widgets
                .iter()
                .enumerate()
                .filter_map(|(n, w)| {
                    let model::Kind::Table { cells, .. } = &w.kind else {
                        return None;
                    };
                    Some((
                        u32::try_from(n).ok()?,
                        false,
                        cells.clone().into_boxed_slice(),
                    ))
                })
                .collect(),
            menus: (tree.widgets.iter().enumerate())
                .filter_map(|(n, w)| {
                    let model::Kind::Menu { items } = &w.kind else {
                        return None;
                    };
                    Some(MenuRead {
                        n: u32::try_from(n).ok()?,
                        // Built from the page as it stands: copied.
                        revision: None,
                        fresh: false,
                        items: Vec::with_capacity((items.len() * ROOM).max(MIN_ROOM)),
                    })
                })
                .collect(),
            fonts: Arc::from([]),
            // Room for the engine's 128 keys, though a Lua page shows none.
            keys: Vec::with_capacity(128),
            pressed_support: false,
            message: Text::EMPTY,
            state: Filled::No,
        })
    }

    /// Brings a Lua page's `tree` to this frame, and picks what the next
    /// trip reads; returns the widget indices that changed.
    fn apply(&mut self, tree: &mut model::Interface) -> Vec<usize> {
        let mut texts = self.texts.iter().filter(|t| t.1).peekable();
        let mut changed = Vec::new();
        // Items first: the value then picks the shown one among them.
        for m in self.menus.iter().filter(|m| m.fresh) {
            if let Some(model::Kind::Menu { items }) =
                tree.widgets.get_mut(m.n as usize).map(|w| &mut w.kind)
            {
                let now: Vec<model::MenuItem> = (1..)
                    .zip(&m.items)
                    .map(|(value, text)| model::MenuItem {
                        text: text.as_str().to_owned(),
                        value,
                        visible: true,
                    })
                    .collect();
                if *items != now {
                    *items = now;
                    changed.push(m.n as usize);
                }
            }
        }
        for (n, look) in self.widgets.iter_mut().enumerate() {
            let text = texts.next_if(|t| t.0 as usize == n).map(|t| t.2.as_str());
            if let Some(look) = look.take()
                && crate::ui::lua::apply(tree, n, look, text)
            {
                changed.push(n);
            }
        }
        for (n, _, path) in self.images.iter().filter(|i| i.1) {
            if crate::ui::lua::picture(tree, *n as usize, path.as_str()) {
                changed.push(*n as usize);
            }
        }
        for (n, has, cells) in &self.arrays {
            if *has
                && let Some(w) = tree.widgets.get_mut(*n as usize)
                && !matches!(&w.value, Some(model::Value::Reals(old)) if old.as_slice() == cells.as_ref())
            {
                w.value = Some(model::Value::Reals(cells.to_vec()));
                changed.push(*n as usize);
            }
        }
        // Within the room it was made with: every widget fits.
        let reads = readable(tree);
        self.reads.clear();
        self.reads.extend(reads);
        changed.sort_unstable();
        changed.dedup();
        changed
    }

    /// Copies the reads out of `engine`; on the audio thread, in place.
    pub(crate) fn fill(&mut self, engine: &Engine) {
        let mut whole = true;
        for l in &mut *self.looks {
            let Some(c) = engine.control_look(self.slot, l.id) else {
                l.present = false;
                continue;
            };
            l.value = c.value;
            // The lists move only with the look's generation.
            if !l.present || l.generation != c.generation {
                let ok = copy(&mut l.pars, c.pars)
                    & copy(&mut l.texts, c.texts)
                    & copy(&mut l.menu, c.menu);
                whole &= ok;
            }
            l.present = true;
            l.generation = c.generation;
        }
        for (cell, value) in &mut *self.cells {
            *value = engine.script_cell_in(self.slot, *cell as usize);
        }
        for (cell, text) in &mut *self.strings {
            let vm = engine.vm(self.slot);
            *text = vm
                .and_then(|vm| vm.text(*cell as usize))
                .copied()
                .unwrap_or(Text::EMPTY);
        }
        self.page = engine.page(self.slot).copied();
        // Same length every trip, so this never allocates.
        if !copy(&mut self.keys, engine.keys()) {
            whole = false;
        }
        self.pressed_support = engine.pressed_support();
        self.message = Text::from_str_cut(engine.last_message()).0;
        // Only what can show: a widget whose parent shows (Augmented
        // Orchestra makes 10,145, about 100 of them shown at once).
        for &n in &self.reads {
            if let Some(look) = self.widgets.get_mut(n as usize) {
                *look = engine.widget(self.slot, n);
            }
        }
        for (n, has, text) in &mut *self.texts {
            *has = self.reads.binary_search(n).is_ok() && engine.widget_text(self.slot, *n, text);
        }
        for (n, has, path) in &mut *self.images {
            *has = self.reads.binary_search(n).is_ok() && engine.widget_image(self.slot, *n, path);
        }
        for (n, has, cells) in &mut *self.arrays {
            *has = self.reads.binary_search(n).is_ok()
                && engine
                    .vm(self.slot)
                    .is_some_and(|vm| vm.widget_cells(*n, cells));
        }
        // A menu's items only when the script changed them: copied into
        // their room, or the frame grows and asks again.
        let vm = engine.vm(self.slot);
        for m in &mut *self.menus {
            m.fresh = false;
            let look = (self.reads.binary_search(&m.n).is_ok())
                .then(|| self.widgets.get(m.n as usize).copied().flatten())
                .flatten();
            let (Some(look), Some(vm)) = (look, vm) else {
                continue;
            };
            if m.revision == Some(look.items) {
                continue;
            }
            m.items.clear();
            let mut text = Text::EMPTY;
            let mut fits = true;
            while vm.widget_item(m.n, m.items.len() as u32, &mut text) {
                if m.items.len() == m.items.capacity() {
                    fits = false;
                    break;
                }
                m.items.push(text);
            }
            whole &= fits;
            m.fresh = fits;
            m.revision = fits.then_some(look.items);
        }
        self.state = if whole {
            Filled::Whole
        } else {
            Filled::Overflowed
        };
    }

    /// The frame for the next trip: itself, or grown if the script outgrew
    /// it, on a control page and a Lua page alike.
    fn kept(self: Box<Self>) -> Box<Self> {
        match self.state {
            Filled::Overflowed => self.grown(),
            Filled::No | Filled::Whole => self,
        }
    }

    /// The same reads with twice the room in every list, to ask again.
    fn grown(&self) -> Box<Self> {
        let looks = (self.looks.iter())
            .map(|l| Look {
                id: l.id,
                present: false,
                value: None,
                generation: 0,
                pars: Vec::with_capacity(l.pars.capacity() * 2),
                texts: Vec::with_capacity(l.texts.capacity() * 2),
                menu: Vec::with_capacity(l.menu.capacity() * 2),
            })
            .collect();
        Box::new(Self {
            looks,
            cells: self.cells.clone(),
            strings: self.strings.clone(),
            widgets: self.widgets.clone(),
            reads: self.reads.clone(),
            arrays: self.arrays.clone(),
            menus: (self.menus.iter())
                .map(|m| MenuRead {
                    revision: None,
                    fresh: false,
                    items: Vec::with_capacity(m.items.capacity() * 2),
                    ..*m
                })
                .collect(),
            texts: self.texts.clone(),
            images: self.images.clone(),
            fonts: Arc::clone(&self.fonts),
            keys: self.keys.clone(),
            state: Filled::No,
            ..*self
        })
    }

    /// Not filled: the part was gone or reloaded when it arrived.
    pub(crate) fn unfilled(&mut self) {
        self.applied_seq = 0;
        self.ui_epoch = 0;
        self.state = Filled::No;
    }
}

/// Whether a Lua widget shows words the script can change: a label's
/// text, or a value's `displayText`.
fn worded(kind: &model::Kind) -> bool {
    matches!(
        kind,
        model::Kind::Label | model::Kind::ValueEdit { .. } | model::Kind::Knob { .. }
    )
}

/// The widgets of a Lua page that can show or change what shows: those
/// whose parent shows. Sorted.
fn readable(tree: &model::Interface) -> Vec<u32> {
    let mut out = Vec::with_capacity(tree.widgets.len());
    out.extend(
        (0u32..)
            .zip(&tree.widgets)
            .filter_map(|(n, w)| w.parent.is_none_or(|p| tree.visible(p)).then_some(n)),
    );
    out
}

/// `to` as `from`, if it has room; else false and `to` is left as it was.
fn copy<T: Copy>(to: &mut Vec<T>, from: &[T]) -> bool {
    if from.len() > to.capacity() {
        return false;
    }
    to.clear();
    to.extend_from_slice(from);
    true
}

impl Looks for LookFrame {
    fn control_look(&self, slot: u8, id: i32) -> Option<ControlLook<'_>> {
        if slot != self.slot {
            return None;
        }
        let n = self.looks.binary_search_by_key(&id, |l| l.id).ok()?;
        let l = self.looks.get(n).filter(|l| l.present)?;
        Some(ControlLook {
            value: l.value,
            pars: &l.pars,
            texts: &l.texts,
            menu: &l.menu,
            generation: l.generation,
        })
    }

    fn script_cell_in(&self, slot: u8, n: usize) -> Option<i64> {
        let n = u32::try_from(n).ok().filter(|_| slot == self.slot)?;
        let at = self.cells.binary_search_by_key(&n, |c| c.0).ok()?;
        self.cells.get(at)?.1
    }

    fn script_text_in(&self, slot: u8, n: usize) -> Option<&str> {
        let n = u32::try_from(n).ok().filter(|_| slot == self.slot)?;
        let at = self.strings.binary_search_by_key(&n, |c| c.0).ok()?;
        Some(self.strings.get(at)?.1.as_str())
    }

    fn page(&self, slot: u8) -> Option<&Page> {
        self.page.as_ref().filter(|_| slot == self.slot)
    }

    fn font(&self, slot: u8, id: i32) -> Option<&str> {
        let n = usize::try_from(id.checked_sub(FIRST_FONT)?).ok()?;
        (self.fonts.get(n).filter(|_| slot == self.slot)).map(String::as_str)
    }

    fn keys(&self) -> &[Key] {
        &self.keys
    }

    fn pressed_support(&self) -> bool {
        self.pressed_support
    }

    fn message(&self) -> &str {
        self.message.as_str()
    }
}

/// Whether the parts' interfaces are on screen. Hidden, [`crate::rack::Rack::tick`]
/// stops asking the audio side for what their scripts changed; the first
/// tick after they show again asks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Interfaces {
    Shown,
    Hidden,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Airy Soul (Conflux): the folded Chords player's `make_perfview` page
    /// (slot 3) must not win over the synth's own Creator Tools view (slot 4).
    #[test]
    fn the_instruments_own_view_wins_over_a_folded_players() {
        let ranks = [2, 2, 2, 1, 0, 2];
        assert_eq!(candidates(6, |s| ranks[s]), [4, 3, 0, 1, 2, 5]);
    }

    #[test]
    fn copied_table_cells_update_the_live_tree() {
        let page = kontra_lua::compile(r#"Table{"curve", 2, 0, 0, 1, false}"#, &[], &[])
            .unwrap()
            .ui;
        let mut tree = crate::ui::lua::tree(&page, &|_| None);
        let mut frame = LookFrame::lua(0, &tree);
        frame.arrays[0].1 = true;
        frame.arrays[0].2.copy_from_slice(&[0.25, 0.75]);
        assert!(!frame.apply(&mut tree).is_empty());
        assert_eq!(
            tree.widgets[0].value,
            Some(model::Value::Reals(vec![0.25, 0.75]))
        );
        assert!(frame.apply(&mut tree).is_empty());
    }

    #[test]
    fn an_outgrown_lua_frame_comes_back_with_more_room() {
        let page = kontra_lua::compile(r#"Menu{"m", {"a"}}"#, &[], &[])
            .unwrap()
            .ui;
        let tree = crate::ui::lua::tree(&page, &|_| None);
        let mut frame = LookFrame::lua(0, &tree);
        let room = frame.menus[0].items.capacity();
        frame.state = Filled::Overflowed;
        let frame = frame.kept();
        assert_eq!(frame.menus[0].items.capacity(), room * 2);
        assert!(frame.state == Filled::No);
    }

    #[test]
    fn copied_menu_items_replace_the_dropdown() {
        let page = kontra_lua::compile(r#"Menu{"m", {"a"}}"#, &[], &[])
            .unwrap()
            .ui;
        let mut tree = crate::ui::lua::tree(&page, &|_| None);
        let mut frame = LookFrame::lua(0, &tree);
        let menu = &mut frame.menus[0];
        menu.items
            .extend([Text::from_str_cut("a").0, Text::from_str_cut("b").0]);
        menu.fresh = true;
        assert!(!frame.apply(&mut tree).is_empty());
        let model::Kind::Menu { items } = &tree.widgets[0].kind else {
            panic!("a menu");
        };
        let texts: Vec<_> = items.iter().map(|i| (i.text.as_str(), i.value)).collect();
        assert_eq!(texts, [("a", 1), ("b", 2)]);
        assert!(frame.apply(&mut tree).is_empty());
    }
}
