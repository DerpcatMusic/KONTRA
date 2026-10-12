//! Focus, keys, clipboard, IME, pointer states, tips and drag-and-drop.
use super::*;

/// Current focused native surrounding-text snapshot. Ranges use UTF-8 bytes;
/// platforms using UTF-16 can convert with `mui_text::byte_to_utf16`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextInputState {
    /// Editable field identity; native contexts reset when this changes.
    pub id: String,
    pub text: String,
    pub selection: std::ops::Range<usize>,
    pub marked: Option<std::ops::Range<usize>>,
}

impl Ui {
    /// The displayed focused edit buffer, including marked composition. Its
    /// selection and geometry come from the same resolved frame as `Frame::ime`.
    pub fn text_input_state(&self) -> Option<TextInputState> {
        let surface = self.scene()?.surface(self.focus_key()?)?;
        let geometry = surface.text_geometry.as_ref()?;
        Some(TextInputState {
            id: self.focus_key()?.to_owned(),
            text: geometry.text.to_string(),
            selection: if geometry.state.selection.is_empty() {
                geometry.state.caret..geometry.state.caret
            } else {
                geometry.state.selection.clone()
            },
            marked: (!geometry.state.marked.is_empty()).then(|| geometry.state.marked.clone()),
        })
    }

    /// Set a native replacement selection in UTF-8 bytes of the displayed
    /// buffer. Invalid ranges are rejected; offsets into a marked span map to
    /// the original replacement range before the next commit edits the value.
    pub fn set_text_input_selection(&mut self, range: std::ops::Range<usize>) -> bool {
        use unicode_segmentation::UnicodeSegmentation;
        let Some(id) = self.focus_key().map(str::to_owned) else {
            return false;
        };
        let Some(g) = self
            .scene()
            .and_then(|s| s.surface(&id))
            .and_then(|s| s.text_geometry.as_ref())
        else {
            return false;
        };
        if range.start > range.end
            || range.end > g.text.len()
            || !g.text.is_char_boundary(range.start)
            || !g.text.is_char_boundary(range.end)
        {
            return false;
        }
        let collapsed = range.is_empty();
        let mut start = g.display_to_source(range.start);
        let mut end = g.display_to_source(range.end);
        if range.start < g.state.marked.end
            && range.end > g.state.marked.start
            && let Some(replacement) = &g.state.replacement
        {
            start = start.min(replacement.start);
            end = end.max(replacement.end);
        }
        let source = &g.state.value;
        let floor = |byte: usize| {
            source
                .grapheme_indices(true)
                .map(|(b, _)| b)
                .chain([source.len()])
                .take_while(|b| *b <= byte)
                .last()
                .unwrap_or(0)
        };
        let start = floor(start);
        let end = if collapsed {
            start
        } else {
            source
                .grapheme_indices(true)
                .map(|(b, _)| b)
                .chain([source.len()])
                .find(|b| *b >= end)
                .unwrap_or(source.len())
        };
        let selection = (
            source[..start].chars().count(),
            source[..end].chars().count(),
        );
        self.set_sel(&id, selection.0, selection.1);
        true
    }

    /// The clipboard's text now, for an app's own Paste button: the
    /// [`Clipboard`] if the host gave one, else what the host handed in on
    /// this frame's [`Input::clipboard`].
    pub fn paste(&mut self) -> Option<String> {
        match self.board.as_mut() {
            Some(b) => b.get(),
            None => self.pasted.clone(),
        }
    }
    /// The latest pointer state accepted by [`Ui::frame`] or [`Ui::inert`].
    /// Position is in scene coordinates; `None` means the pointer left the
    /// surface. This is a read-only snapshot, including current modifiers.
    pub fn pointer(&self) -> PointerInput {
        self.pointer
    }

    /// Which shape of the canvas `id` the pointer is on, by the tag its
    /// [`Draw`](mui_scene::Draw) carried. `None` when the pointer is over no
    /// tagged shape of that node -- including inside its frame but outside
    /// every drawn path.
    ///
    /// Latched at the press: through a drag it stays the shape the gesture
    /// grabbed, so a knot dragged past its neighbours is still that knot.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::default();
    /// let plot = canvas(|size| {
    ///     let box_ = [(0., 0.), (size.width, 0.), (size.width, size.height)];
    ///     vec![Draw::fill(Path::polyline(box_.map(|(x, y)| Point::new(x, y)), true), Role::Primary)
    ///         .tag("wedge")]
    /// })
    /// .square(80.)
    /// .id("plot");
    /// ui.frame(plot, None, PointerInput::default(), 0.016).unwrap();
    /// assert_eq!(ui.tag("plot"), None, "the pointer is nowhere");
    /// ```
    pub fn tag(&self, id: impl Into<Id>) -> Option<&str> {
        let id: Id = id.into();
        let id = id.as_str();
        let (k, t) = self.tagged.as_ref()?;
        (k == id).then_some(t.as_str())
    }
    /// Hover and press amounts for `id`, 0..1 and spring-smoothed.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let ui = Ui::default();
    /// let Interaction { hover, press } = ui.state("go");
    /// assert_eq!((hover, press), (0.0, 0.0), "nothing has touched it");
    /// ```
    pub fn state(&self, id: impl Into<Id>) -> Interaction {
        let id: Id = id.into();
        let id = id.as_str();
        (self.nodes.get(id).and_then(|n| n.springs)).map_or(Interaction::default(), |[h, p]| {
            Interaction {
                hover: h.value,
                press: p.value,
            }
        })
    }
    /// The id that holds the keyboard focus, for a host reporting it.
    pub fn focus_key(&self) -> Option<&str> {
        self.focus.as_deref()
    }
    /// Whether `id` holds the keyboard focus.
    pub fn focused(&self, id: impl Into<Id>) -> bool {
        let id: Id = id.into();
        let id = id.as_str();
        self.focus.as_deref() == Some(id)
    }
    /// Whether `id` holds the focus and got it from the keyboard or from
    /// [`Ui::focus`], not from a click: whether to draw its focus ring.
    /// [`State::FocusVisible`](mui_scene::State::FocusVisible) is the same
    /// test on a node's look.
    pub fn focus_visible(&self, id: impl Into<Id>) -> bool {
        self.focus_visible && self.focused(id)
    }
    /// Drop the keyboard focus from code: a field that submits on Enter
    /// lets go of the keys, as a click on the background would.
    pub fn blur(&mut self) {
        self.preedit = None;
        self.focus = None;
    }
    /// Focus `id` from code. No check that it exists: it may not have been
    /// built yet.
    pub fn focus(&mut self, id: impl Into<Id>) {
        // A composition belongs to the field that started it.
        self.preedit = None;
        self.focus = Some(id.into().as_str().to_owned());
        self.focus_visible = true;
    }
    /// The keys this frame, if `id` is focused. Empty otherwise, so a widget
    /// may loop over it unconditionally.
    pub fn keys(&self, id: impl Into<Id>) -> &[KeyPress] {
        let id: Id = id.into();
        let id = id.as_str();
        if self.focused(Id::runtime(id)) {
            &self.keys
        } else {
            &[]
        }
    }
    /// Every key this frame, whatever holds the focus: the stream a global
    /// shortcut reads. `Ui` has already taken Tab and Escape for focus, and
    /// the keys are here as well as in [`Ui::keys`] -- a shortcut and a
    /// focused widget see the same press.
    ///
    /// The one exception is the rule every editor has: **a focused text
    /// input consumes the stream**, so typing `z` in a search box is a `z`
    /// and not an undo. Nothing else swallows keys; a widget that wants to
    /// claim a key while focused must check [`Ui::focused`] itself.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::default();
    /// # let root = block(10., 10.).id("root");
    /// # ui.frame(root, None, PointerInput::default(), 0.016).unwrap();
    /// let undo = ui
    ///     .shortcuts()
    ///     .iter()
    ///     .any(|k| k.key == Key::Char('z') && (k.mods.ctrl || k.mods.cmd));
    /// assert!(!undo, "nothing was pressed");
    /// ```
    pub fn shortcuts(&self) -> &[KeyPress] {
        if self.focus_is_text() {
            &[]
        } else {
            &self.keys
        }
    }
    /// Whether the focus is on a text field (in the last scene): the keys
    /// are typing, not shortcuts. A host routes keys, or takes the keyboard
    /// from its parent window, by it.
    pub fn focus_is_text(&self) -> bool {
        self.focus.as_deref().is_some_and(|k| {
            self.scene
                .as_ref()
                .and_then(|s| s.surface(k))
                .and_then(|s| s.semantics.as_ref())
                .is_some_and(|s| matches!(s.role, A11y::TextInput { .. }))
        })
    }
    /// The text typed this frame, if `id` is focused.
    pub fn text(&self, id: impl Into<Id>) -> &str {
        let id: Id = id.into();
        let id = id.as_str();
        if self.focused(Id::runtime(id)) {
            &self.typed
        } else {
            ""
        }
    }
    /// The pointer relative to `id`'s frame origin, if both exist.
    pub fn local(&self, id: impl Into<Id>) -> Option<Point> {
        let id: Id = id.into();
        let id = id.as_str();
        let p = self.pointer.pos?;
        let s = self.scene.as_ref()?.surface(id)?;
        Some(s.local(p))
    }
    /// Whether `input` would change nothing a frame shows, so the host can
    /// skip building one: a bare move with no capture held and no edge owed
    /// to the next tree, landing on the hovered target and canvas shape, and
    /// neither over a [`tracks_pointer`](mui_scene::Styled::tracks_pointer)
    /// node nor crossing a scroller whose bar warms under the pointer. An
    /// inert move becomes the pointer the next frame starts from; springs
    /// and the tip's clock run on regardless.
    pub fn inert(&mut self, input: &Input) -> bool {
        let p = input.pointer;
        let Some(scene) = self.scene.as_ref() else {
            return false;
        };
        if !(self.resolved
            && (p.buttons, p.mods) == (self.pointer.buttons, self.pointer.mods)
            && input.wheel == Vec2::ZERO
            && input.keys.is_empty()
            && input.text.is_empty()
            && input.clipboard.is_none()
            && input.ime.is_empty()
            && self.interaction.held().is_none()
            && self.actions.is_empty()
            && self.edits.is_empty()
            && self.delivered.is_empty()
            && self.cancelled.is_none()
            // What the last frame took in is read by the next tree.
            && self.keys.is_empty()
            && self.typed.is_empty()
            && self.wheel == Vec2::ZERO
            && self.press_at.is_none())
        {
            return false;
        }
        if self.retargets(p.pos) {
            return false;
        }
        let on = |at: Option<Point>, f: mui_layout::Frame| at.is_some_and(|q| f.contains(q.x, q.y));
        let was = self.pointer.pos;
        let touched = scene.surfaces().any(|s| {
            (s.tracks_pointer && (on(p.pos, s.frame) || on(was, s.frame)))
                || bar::bar_of(&s.key)
                    .and_then(|(key, _)| scene.surface(key))
                    .is_some_and(|n| on(p.pos, n.frame) != on(was, n.frame))
        });
        if touched {
            return false;
        }
        self.pointer = p;
        true
    }
    /// The target and tagged shape under `pos` in last frame's hit map, as
    /// `reconcile` will read it.
    pub(super) fn under(&self, pos: Option<Point>) -> Option<(&str, Option<&str>)> {
        let scene = self.scene.as_ref()?;
        self.hit.at_tagged_with(pos?, |key, tag, q| {
            if tag.is_some() {
                return None;
            }
            scene.external_contains(key, q)
        })
    }
    /// Whether `pos` lands on another hovered target or tagged shape.
    pub(super) fn retargets(&self, pos: Option<Point>) -> bool {
        let under = self.under(pos);
        let tagged = under.and_then(|(id, tag)| Some((id, tag?)));
        under.map(|(id, _)| id) != self.interaction.hovered()
            || tagged != self.tagged.as_ref().map(|(k, t)| (k.as_str(), t.as_str()))
    }
    /// The pointer the next frame will bring, told before the tree is built:
    /// a memo holding the target it leaves or the one it enters is built
    /// again now, so its hover shows this frame rather than the next.
    pub fn anticipate(&mut self, pos: Option<Point>) {
        if self.kept.is_empty() || !self.retargets(pos) {
            return;
        }
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let entered = self.under(pos).map(|(id, _)| id);
        let left = [
            self.interaction.hovered(),
            self.tagged.as_ref().map(|t| t.0.as_str()),
        ];
        let hot: Vec<u64> = [entered, left[0], left[1]]
            .into_iter()
            .flatten()
            .flat_map(|k| scene.memos_at(k).map(|(id, _)| id))
            .collect();
        self.hot.extend(hot);
    }
    /// Source and target of a drag released this frame.
    pub fn dropped(&self) -> Option<(&str, &str)> {
        self.interaction.dropped()
    }
    /// Attach a payload to the drag `id` has in flight: the value the drop
    /// target will be handed. Call it while the gesture is dragging -- a
    /// second call replaces what the first attached, so a widget may simply
    /// set it every frame.
    ///
    /// The payload is the caller's type, not MUI's. A drag *ghost* is the
    /// caller's too: pin an `El` to the pointer's surface and `.float()` it.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::default();
    /// struct Wave(&'static str);
    /// if ui.get("saw").dragged {
    ///     ui.start_drag("saw", Wave("saw"));
    /// }
    /// assert!(ui.dragging::<Wave>().is_none(), "nothing is dragging");
    /// ```
    pub fn start_drag(&mut self, id: impl Into<Id>, payload: impl Any + Send) {
        let id: Id = id.into();
        let id = id.as_str();
        self.drag = Some((id.to_owned(), Box::new(payload)));
    }
    /// The payload of the drag in flight, for anything that wants to look
    /// before it lands: a drop target that highlights only for a payload it
    /// accepts, a ghost that draws what is being carried. `None` once the
    /// pointer is released, or when the payload is not a `T`.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::default();
    /// struct Wave(&'static str);
    /// assert!(ui.dragging::<Wave>().is_none());
    /// ```
    pub fn dragging<T: Any>(&self) -> Option<&T> {
        self.interaction.held()?;
        self.drag.as_ref()?.1.downcast_ref::<T>()
    }
    /// Take the payload of a drag released over `id` this frame. Delivered
    /// exactly once: the next call, on this frame or any later one, is
    /// `None`. A release anywhere else delivers nothing to `id`, and a
    /// payload of another type is left in place for whoever wants it.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let mut ui = Ui::default();
    /// struct Wave(&'static str);
    /// let mut slot: Option<&'static str> = None;
    /// if let Some(Wave(w)) = ui.dropped_on::<Wave>("slot-0") {
    ///     slot = Some(w);
    /// }
    /// assert_eq!(slot, None, "nothing was dropped");
    /// ```
    pub fn dropped_on<T: Any>(&mut self, id: impl Into<Id>) -> Option<T> {
        let id: Id = id.into();
        let id = id.as_str();
        let from = self.drag.as_ref().map(|(k, _)| k.as_str());
        if !self
            .interaction
            .dropped()
            .is_some_and(|(src, target)| target == id && from == Some(src))
        {
            return None;
        }
        let (src, payload) = self.drag.take()?;
        match payload.downcast::<T>() {
            Ok(v) => Some(*v),
            Err(payload) => {
                self.drag = Some((src, payload));
                None
            }
        }
    }
    /// A button went down this frame outside every one of `ids`: the signal a
    /// popup or menu closes on. Pass the popup and whatever opened it, so
    /// the press that reopens it from its own button does not also close it.
    ///
    /// Outside means outside each id's frame and not on a target nested
    /// under it, so a submenu floated past its parent's edge is still in.
    /// An id not in the last scene is skipped: the press that opened a
    /// popup came before the popup existed, and does not dismiss it.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::default();
    /// let mut open = true;
    /// if ui.dismissed(&["menu", "menu-button"]) {
    ///     open = false;
    /// }
    /// # assert!(open, "nothing was pressed");
    /// ```
    pub fn clicked_outside(&self, ids: &[&str]) -> bool {
        let (Some(p), Some(scene)) = (self.press_at, self.scene.as_ref()) else {
            return false;
        };
        let pressed = self.interaction.pressed();
        let mut any = false;
        for id in ids {
            let Some(s) = scene.surface(id) else {
                continue;
            };
            any = true;
            let f = s.frame;
            if p.x >= f.x && p.x <= f.right() && p.y >= f.y && p.y <= f.bottom() {
                return false;
            }
            // A press on a descendant: walk its named ancestors up to `id`.
            let mut at = pressed.and_then(|k| scene.surface(k));
            while let Some(t) = at {
                if &*t.key == *id {
                    return false;
                }
                at = t.parent.as_deref().and_then(|k| scene.surface(k));
            }
        }
        any
    }
    /// [`Ui::clicked_outside`], or Escape pressed this frame whatever holds
    /// the focus: everything that closes a popup, in one question.
    pub fn dismissed(&self, ids: &[&str]) -> bool {
        self.clicked_outside(ids) || self.keys.iter().any(|k| k.key == Key::Escape)
    }
    /// Keep a selection endpoint on the previous visual line when a soft
    /// wrap shares the same logical byte offset with the next line's start.
    /// Native accessibility clients call this beside their selection action.
    pub fn set_text_selection_affinity(&mut self, id: impl Into<Id>, upstream: bool) {
        let id = id.into();
        node(&mut self.nodes, id.as_str())
            .text_edit
            .get_or_insert_with(Default::default)
            .upstream = upstream;
    }
    pub(crate) fn text_navigation(&self, id: &str) -> (Option<f64>, bool) {
        self.nodes
            .get(id)
            .and_then(|n| n.text_edit.as_ref())
            .map_or((None, false), |e| (e.goal, e.upstream))
    }
    pub(crate) fn set_text_navigation(&mut self, id: &str, goal: Option<f64>, upstream: bool) {
        let e = node(&mut self.nodes, id)
            .text_edit
            .get_or_insert_with(Default::default);
        e.goal = goal;
        e.upstream = upstream;
    }
    pub(crate) fn take_native_text(&mut self, id: &str) -> Vec<Ime> {
        self.nodes
            .get_mut(id)
            .and_then(|n| n.text_edit.as_mut())
            .map_or_else(Vec::new, |e| std::mem::take(&mut e.native))
    }
    pub(crate) fn text_scroll(&self, id: &str) -> f64 {
        self.nodes
            .get(id)
            .and_then(|n| n.text_scroll)
            .unwrap_or(0.0)
    }
    pub(crate) fn set_text_scroll(&mut self, id: &str, y: f64) {
        node(&mut self.nodes, id).text_scroll = Some(y);
    }
    pub(crate) fn stash<T: Any>(&self, id: &str) -> Option<&T> {
        self.nodes.get(id)?.stash.as_ref()?.downcast_ref()
    }
    pub(crate) fn set_stash<T: Any + Send>(&mut self, id: &str, v: Option<T>) {
        match v {
            Some(v) => node(&mut self.nodes, id).stash = Some(Box::new(v)),
            None => {
                if let Some(n) = self.nodes.get_mut(id) {
                    n.stash = None;
                }
            }
        }
    }
    pub(crate) fn sel(&self, id: &str) -> (usize, usize) {
        self.nodes.get(id).and_then(|n| n.sel).unwrap_or((0, 0))
    }
    pub(crate) fn set_sel(&mut self, id: &str, anchor: usize, caret: usize) {
        node(&mut self.nodes, id).sel = Some((anchor, caret));
    }
    /// The clipboard the host handed in because a paste key arrived.
    pub(crate) fn pasted(&self) -> Option<&str> {
        self.pasted.as_deref()
    }
    /// Whether the last press on `id` was the second of a double click.
    pub fn double_click(&self, id: impl Into<Id>) -> bool {
        let id: Id = id.into();
        let id = id.as_str();
        self.double.as_deref() == Some(id)
    }
    /// The text the input method is composing, for the focused field to
    /// paint. It is never part of a value.
    pub fn preedit(&self) -> Option<(&str, Option<(usize, usize)>)> {
        self.preedit.as_ref().map(|(t, c)| (t.as_str(), *c))
    }
    /// Tell the host where this field's caret is, in the field's own space,
    /// so the IME candidate window lands under it. Comes back on
    /// [`Frame::ime`].
    pub fn set_ime_caret(&mut self, id: impl Into<Id>, at: Point, height: f64) {
        let id: Id = id.into();
        let id = id.as_str();
        self.ime_caret = Some((id.to_owned(), at, height));
    }
    /// Put `s` on the clipboard: what an app's own Copy button calls. It
    /// goes to the [`Clipboard`] at the end of the next frame, or comes back
    /// on that frame's [`Frame::clipboard`] for a host without one.
    pub fn set_clipboard(&mut self, s: impl Into<String>) {
        self.copied = Some(s.into());
    }
    /// Move the focus to the next (or previous) focusable surface in z-order,
    /// wrapping. Nothing focused yet starts at either end.
    pub(super) fn cycle_focus(&mut self, back: bool) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let stops: Vec<String> = scene
            .surfaces()
            .filter(|s| s.focusable && !s.disabled)
            .map(|s| s.key.to_string())
            .collect();
        if stops.is_empty() {
            return;
        }
        let at = self
            .focus
            .as_ref()
            .and_then(|c| stops.iter().position(|k| k == c));
        let next = match (at, back) {
            (Some(i), false) => (i + 1) % stops.len(),
            (Some(i), true) => (i + stops.len() - 1) % stops.len(),
            (None, false) => 0,
            (None, true) => stops.len() - 1,
        };
        self.focus = Some(stops[next].clone());
        self.focus_visible = true;
    }

    /// Apply a horizontal or vertical drag on `id` to `value` across `range`,
    /// `px` pixels for the full span. Returns whether it changed.
    ///
    /// Shift is the fine modifier: the same travel moves a tenth as far
    /// ([`mui_input::FINE_DRAG`]), which is how every parameter in a synth
    /// editor is dialled in.
    ///
    /// ```
    /// # use mui::Ui; use mui::prelude::*;
    /// # let ui = Ui::default();
    /// let mut cutoff = 0.5;
    /// // Nothing is dragging, so nothing moves.
    /// assert!(!ui.drag("cutoff", &mut cutoff, 0.0..=1.0, 160.0, false));
    /// ```
    pub fn drag(
        &self,
        id: impl Into<Id>,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool {
        let id: Id = id.into();
        let id = id.as_str();
        if !(range.start().is_finite() && range.end().is_finite()) {
            return false;
        }
        if let Some(value_from_action) = self.actions.iter().rev().find_map(|a| match a {
            SemanticAction::SetValue { id: target, value } if target == id => Some(*value),
            _ => None,
        }) {
            let next = value_from_action.clamp(
                range.start().min(*range.end()),
                range.start().max(*range.end()),
            );
            let changed = next != *value;
            *value = next;
            return changed;
        }
        let r = self.get(Id::runtime(id));
        if !r.dragged || !px.is_finite() || px <= 0.0 || !value.is_finite() {
            return false;
        }
        let delta = r.drag_fine(FINE_DRAG);
        let d = if vertical { -delta.y } else { delta.x };
        if !d.is_finite() {
            return false;
        }
        // An inverted range (`1.0..=0.0`) is a legitimate downward control and
        // the delta math already reverses for it; only `clamp` needs the
        // bounds in order, since it panics on `min > max`.
        let next = (*value + d / px * (range.end() - range.start())).clamp(
            range.start().min(*range.end()),
            range.start().max(*range.end()),
        );
        let changed = next != *value;
        *value = next;
        changed
    }

    /// Which memos the next build must run again: the ones holding a key
    /// whose state moved this frame, or all of them after discrete input.
    /// `was` is the hovered, held and tagged keys before this frame.
    pub(super) fn heat(&mut self, scene: &ResolvedScene, was: &[Option<Id>; 3], buttons: bool) {
        self.hot.clear();
        let me = self.me;
        let ids = &mut self.memo_ids;
        self.kept.retain(|id, k| {
            let live = std::mem::take(&mut k.live);
            if !live {
                TREES.with(|t| t.borrow_mut().remove(&(me, *id)));
                ids.remove(&k.id);
            }
            live
        });
        self.hot_all = buttons
            || !self.keys.is_empty()
            || !self.typed.is_empty()
            || self.pasted.is_some()
            || self.preedit.is_some()
            || !self.delivered.is_empty();
        if self.kept.is_empty() || self.hot_all {
            return;
        }
        let now = [
            self.interaction.hovered(),
            self.interaction.held(),
            self.tagged.as_ref().map(|t| t.0.as_str()),
        ];
        let mut keys: Vec<&str> = Vec::new();
        for (a, b) in now.iter().zip(was) {
            if *a != b.as_deref() {
                keys.extend(a.iter().copied().chain(b.as_deref()));
            }
        }
        keys.extend(self.interaction.held());
        keys.extend(self.focus.as_deref());
        if self.wheel != Vec2::ZERO {
            keys.extend(self.interaction.hovered());
        }
        let moving = |s: &[Spring]| s.iter().any(|s| !at_rest(s));
        for (k, n) in &self.nodes {
            if n.springs.is_some_and(|s| moving(&s))
                || (n.motion.as_ref()).is_some_and(|(_, l)| l.iter().any(|(_, s)| !at_rest(s)))
                || n.glide.is_some_and(|(_, s)| moving(&s))
                || n.scroll.is_some_and(|s| moving(&s))
                || n.morph.as_ref().is_some_and(|m| m.from.is_some())
            {
                keys.push(k);
            }
            // A scrollbar's heat is the runtime's own tween, under its node's key.
            if let (Some(bar), Some((_, s))) = (k.strip_prefix("/bar/"), &n.tween)
                && !at_rest(s)
            {
                keys.push(bar);
            }
        }
        for k in keys {
            self.hot.extend(scene.memos_at(k).map(|(id, _)| id));
        }
        self.hot.extend(scene.memos_floating());
    }

    /// Focus follows a press on a focusable surface, and a press on anything
    /// else -- another target, or empty background that is no target at all
    /// -- drops it; then the keys, typed text and input-method events land
    /// for the widgets to read. `was` is last frame's buttons.
    pub(super) fn intake_focus(
        &mut self,
        was: Buttons,
        keys: Vec<KeyPress>,
        text: String,
        ime: Vec<Ime>,
    ) {
        self.double = None;
        let went_down = [Button::Primary, Button::Secondary, Button::Middle]
            .into_iter()
            .any(|b| self.pointer.buttons.contains(b) && !was.contains(b));
        if went_down && self.interaction.held().is_none() {
            self.focus = None;
        }
        self.press_at = self.pointer.pos.filter(|_| went_down);
        if let Some(id) = self.interaction.pressed().map(str::to_owned) {
            if let Some((prev, t)) = self.last_press.take()
                && prev == id
                && self.time - t < self.double_click
            {
                self.double = Some(id.clone());
            }
            self.last_press = Some((id.clone(), self.time));
            let keeps = self
                .scene
                .as_ref()
                .and_then(|s| s.surface(&id))
                .is_some_and(|s| s.focusable);
            self.focus = keeps.then_some(id);
            self.focus_visible = false;
        }
        for k in &keys {
            match k.key {
                Key::Escape => self.focus = None,
                Key::Tab => self.cycle_focus(k.mods.shift),
                _ => {}
            }
        }
        self.keys = keys;
        self.typed = text;
        for e in ime {
            if matches!(e, Ime::Selection(_) | Ime::Commit(_))
                && let Some(id) = self.focus.as_deref()
            {
                node(&mut self.nodes, id)
                    .text_edit
                    .get_or_insert_with(Default::default)
                    .native
                    .push(e.clone());
            }
            match e {
                // A commit is typed text: it inserts at the caret and
                // replaces the selection exactly as a keystroke would.
                Ime::Commit(s) => {
                    self.preedit = None;
                    self.typed.push_str(&s);
                }
                Ime::Preedit { text, cursor } => {
                    // The platform's byte range is checked here, at the edge,
                    // rather than in every widget that slices by it.
                    let cursor = cursor
                        .filter(|(s, e)| text.is_char_boundary(*s) && text.is_char_boundary(*e));
                    self.preedit = (!text.is_empty()).then_some((text, cursor));
                }
                Ime::Selection(_) => {}
                Ime::Enabled | Ime::Disabled => self.preedit = None,
            }
        }
        if self.focus.is_none() {
            self.preedit = None;
        }
    }

    /// A tip is due after the pointer has rested. Both the hover and the
    /// surface come from last frame's scene, which is the one the pointer
    /// was actually over. Returns the due tip and its anchor, and whether one
    /// is still counting down.
    pub(super) fn tip_due(
        &mut self,
        hovered: Option<&str>,
        dt: f64,
    ) -> (Option<(Arc<str>, String)>, bool) {
        // A tip is pinned to its anchor by id, and a positional key moves
        // when the tip wraps the root: only a named surface has one.
        let hovered = hovered.filter(|k| named(k));
        match (&mut self.hover, hovered) {
            (Some((id, t)), Some(h)) if id == h => *t += dt,
            (_, Some(h)) => self.hover = Some((h.to_owned(), 0.0)),
            (_, None) => self.hover = None,
        }
        // A host may sleep when a frame is otherwise static. Keep it awake
        // until the tooltip deadline, and while a focused text field's caret
        // is blinking; both are time-driven visual changes rather than paint
        // springs. The previous scene is the one that measured this hover.
        let pending = self.hover.as_ref().is_some_and(|(id, t)| {
            *t < TIP_DELAY
                && self
                    .scene
                    .as_ref()
                    .and_then(|scene| scene.surface(id))
                    .is_some_and(|surface| surface.tip.is_some())
        });
        // A focused caret is a deadline, not an animation: `repaint_after`
        // wakes the host at the next blink edge and one catch-up frame in
        // `commit` makes that edge visible.
        let tip = self
            .hover
            .as_ref()
            .filter(|(_, t)| *t >= TIP_DELAY)
            .and_then(|(id, _)| {
                let s = self.scene.as_ref()?.surface(id)?;
                Some((s.tip.clone()?, id.clone()))
            });
        (tip, pending)
    }

    /// Float a due tip over the caller's root.
    pub(super) fn wrap_tip(root: El, tip: Option<&(Arc<str>, String)>) -> El {
        let Some((t, anchor)) = tip else {
            return root;
        };
        // Under the surface, flipping over it at the bottom edge of the
        // window: the placement is the pin's, not arithmetic here. A pinned
        // float is absolute and painted after everything, unclipped, so it
        // joins the root as its last child: no other node's tree path moves,
        // and every cache keyed by one stays warm while a tip comes and goes.
        // ponytail: a tip under a scrolling root is offered no room and does
        // not wrap; give tips a width cap if a long one ever runs off.
        // The box is a stack around the text: a text node's own fill is its
        // ink, so `text(..).fill(..)` would paint no background at all and
        // the tip would read as see-through. Anchored at the start so a
        // stretching parent does not offer the box its whole size.
        let float = stack([text(t.clone())])
            .anchor(mui_layout::Align::Start, mui_layout::Align::Start)
            .pad(S)
            .fill(Role::Raised)
            .radius(6.0)
            .pin(
                Pin::to(anchor.clone())
                    .area(Area::BottomStart)
                    .gap(Xs)
                    .fallback(Area::TopStart),
            )
            .id(mui_scene::Id::runtime(TIP_KEY));
        if root.is_container() {
            return root.push(float);
        }
        // A leaf has no children to take it. Its own key is the only one
        // the wrapper moves, and for the frames the tip is up.
        stack([root, float])
    }
}

/// Whether `keys`, read by the focused `id`, edit it: Enter or Space on a
/// button or switch, a step on a slider.
pub(super) fn keyed_edit(scene: Option<&ResolvedScene>, id: &str, keys: &[KeyPress]) -> bool {
    let role = scene
        .and_then(|s| s.surface(id))
        .filter(|s| !s.disabled)
        .and_then(|s| s.semantics.as_ref())
        .map(|s| &s.role);
    keys.iter().any(|k| match role {
        Some(A11y::Button | A11y::Toggle { .. }) => matches!(k.key, Key::Enter | Key::Space),
        Some(A11y::Slider { .. }) => matches!(
            k.key,
            Key::Left
                | Key::Right
                | Key::Up
                | Key::Down
                | Key::PageUp
                | Key::PageDown
                | Key::Home
                | Key::End
        ),
        _ => false,
    })
}

/// Whether a semantic role owns the runtime's default pointer looks.
pub(super) fn interactive(e: &Element) -> bool {
    e.semantics.as_ref().is_some_and(|s| {
        matches!(
            &s.role,
            A11y::Button | A11y::Slider { .. } | A11y::Toggle { .. } | A11y::TextInput { .. }
        )
    })
}

/// Which pointer states, hover then press, deserve springs for each of the
/// two active targets. Structural IDs remain hit-testable, but do not keep
/// the host animating merely because the pointer rests on them. One walk
/// finds both, and none runs while the pointer rests on nothing.
pub(super) fn state_policy(root: &El, ids: [Option<&str>; 2]) -> [[bool; 2]; 2] {
    let policy = |e: &Element| {
        let mut policy = [interactive(e), interactive(e)];
        for (state, _) in &e.states {
            match state {
                State::Hover => policy[0] = true,
                State::Press => policy[1] = true,
                State::Focus | State::FocusVisible | State::Disabled => {}
            }
        }
        policy
    };
    let mut out = [[false; 2]; 2];
    // A tree path is walked by index; only ids need the search.
    let mut left = 0;
    for (i, id) in ids.into_iter().enumerate() {
        match id {
            Some(id) if !named(id) => {
                out[i] = find(root, id).map_or([false; 2], |n| policy(n.payload()));
            }
            Some(_) => left += 1,
            None => {}
        }
    }
    if left > 0 {
        let mut found = [false; 2];
        find_each(root, &mut |n| {
            for (i, id) in ids.into_iter().enumerate() {
                // The first match wins, as `find`'s does.
                if !found[i] && id.is_some_and(|id| named(id) && n.key() == Some(id)) {
                    found[i] = true;
                    out[i] = policy(n.payload());
                    left -= 1;
                }
            }
            left > 0
        });
    }
    out
}

/// Visit `n` and its descendants in pre-order while `f` says to go on.
fn find_each(n: &El, f: &mut dyn FnMut(&El) -> bool) -> bool {
    f(n) && n.children().iter().all(|c| find_each(c, f))
}

/// Replace every node's style with what it declared for the states it
/// is in, in declaration order. Keep the declarations on the node: the later
/// automatic-state pass uses them to avoid applying the same state twice.
/// `off` is the enclosing subtree's disabled flag, `false` at the root: a card
/// that switched itself off greys the controls inside it too, which is the same
/// rule the hit gate uses. An unnamed node is keyed by its tree path, the key
/// the hit map gives it when it declares a hover or press look.
/// `focused` is [focused, focused from the keyboard or code].
pub(super) fn declared_states(
    n: &mut El,
    springs: Option<[Spring; 2]>,
    focused: [bool; 2],
    off: bool,
) {
    // A declared look follows the spring's target, not its value: gating on
    // the value held every hover and press back ~84 ms (the default spring's
    // half-rise) before `.animate()` even began its own fade, so a menu row
    // lit several frames behind the pointer. `.animate()` still eases it.
    let is = |st: State| match st {
        State::Hover => springs.is_some_and(|[h, _]| h.target > 0.5),
        State::Press => springs.is_some_and(|[_, p]| p.target > 0.5),
        State::Focus => focused[0],
        State::FocusVisible => focused[1],
        // Declared by the node and answered by `off` below.
        State::Disabled => false,
    };
    if !n.payload().states.is_empty() {
        let e = n.payload_mut();
        let states = std::mem::take(&mut e.states);
        let mut style = std::mem::take(&mut e.style);
        for (st, f) in &states {
            let on = match st {
                State::Disabled => off,
                // A disabled node is never hovered or pressed -- it is not in
                // the hit map -- and a focus it held before it was switched
                // off is not a reason to paint it lit.
                _ => !off && is(*st),
            };
            if on {
                style = f.0(style);
            }
        }
        let e = n.payload_mut();
        e.style = style;
        e.states = states;
    }
}

/// Push automatic hover and press into interactive surfaces' fills,
/// proportionally, and slide the ones the wheel has scrolled. A named layout
/// node is an identity and hit-test surface, not automatically a control. An
/// explicit state owns its channel so a declared look is applied once.
/// `off` is the enclosing subtree's disabled flag, `false` at the root: a
/// hover spring still decaying from before the node was switched off must not
/// tint it.
pub(super) fn state(
    n: &mut El,
    pal: &Palette,
    springs: Option<[Spring; 2]>,
    scroll: Option<[Spring; 2]>,
    off: bool,
) {
    if let Some([x, y]) = scroll.map(|s| s.map(|s| s.value)) {
        // `scrolled` is a builder and a built node cannot be reopened.
        let node = std::mem::replace(n, mui_scene::block(0.0, 0.0));
        *n = node.scrolled(x, y);
    }
    if let Some([h, p]) = springs.map(|s| s.map(|s| s.value)).filter(|_| !off) {
        let bg = pal.background();
        let (auto_hover, auto_press) = {
            let e = n.payload();
            (
                interactive(e) && !e.states.iter().any(|(state, _)| *state == State::Hover),
                interactive(e) && !e.states.iter().any(|(state, _)| *state == State::Press),
            )
        };
        let e = n.payload_mut();
        if e.style.fill.as_ref().is_some_and(|f| !f.is_none())
            && ((auto_hover && h > 0.0) || (auto_press && p > 0.0))
        {
            e.style.fill = e.style.fill.as_ref().map(|f| {
                f.map(pal, bg, |c| {
                    let c = if auto_hover {
                        c.mix(pal.hover(c), h as f32)
                    } else {
                        c
                    };
                    if auto_press {
                        c.mix(pal.pressed(c), p as f32)
                    } else {
                        c
                    }
                })
            });
        }
    }
}
