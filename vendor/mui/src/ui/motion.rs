//! Springs, tweens, plays, glides, appearing, morphs and transitions.
use super::*;
use mui_material::Capture;

/// Policy for decorative and interactive UI motion. Explicit media timelines
/// (`Keys::at`, CUT, and audio/video playback) keep their own clocks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MotionPolicy {
    /// Follow the preference supplied by the host. Until supplied, animate.
    #[default]
    System,
    /// Animate UI transitions even if the host requests reduced motion.
    Full,
    /// Show final UI states immediately, without animation wakeups.
    Reduced,
}

/// Step UI motion without changing a spring's tuning when preferences change.
pub(super) fn step_spring(s: &mut Spring, dt: f64, reduced: bool) -> bool {
    if reduced {
        s.value = s.target;
        s.velocity = 0.;
        false
    } else {
        s.step(dt)
    }
}

impl Ui {
    /// The configured UI motion policy.
    pub fn motion_policy(&self) -> MotionPolicy {
        self.motion_policy
    }
    /// Whether UI effects currently resolve directly to their endpoints.
    pub fn reduced_motion(&self) -> bool {
        match self.motion_policy {
            MotionPolicy::System => self.system_reduced_motion,
            MotionPolicy::Full => false,
            MotionPolicy::Reduced => true,
        }
    }
    /// Set an application override. Returns whether the effective preference
    /// changed: a host should request a redraw only when this returns `true`.
    pub fn set_motion_policy(&mut self, policy: MotionPolicy) -> bool {
        let before = self.reduced_motion();
        self.motion_policy = policy;
        self.motion_changed(before)
    }
    /// Update the OS preference at startup or when its accessibility setting
    /// changes. Overrides remain in effect. Returns whether a redraw is needed.
    pub fn set_system_reduced_motion(&mut self, reduced: bool) -> bool {
        let before = self.reduced_motion();
        self.system_reduced_motion = reduced;
        self.motion_changed(before)
    }
    fn motion_changed(&mut self, before: bool) -> bool {
        let reduced = self.reduced_motion();
        if before == reduced {
            return false;
        }
        // Cached styled subtrees must be rebuilt with the new preference.
        for kept in self.kept.values_mut() {
            kept.still = false;
        }
        if reduced {
            self.ghosts.clear();
            self.play_until = self.time;
            for n in self.nodes.values_mut() {
                for s in n
                    .springs
                    .iter_mut()
                    .flatten()
                    .chain(n.scroll.iter_mut().flatten())
                    .chain(n.glide.iter_mut().flat_map(|(_, s)| s))
                    .chain(n.tween.iter_mut().map(|(_, s)| s))
                    .chain(
                        n.motion
                            .iter_mut()
                            .flat_map(|(_, ch)| ch.iter_mut().map(|(_, s)| s)),
                    )
                {
                    step_spring(s, 0., true);
                }
            }
        }
        true
    }

    /// A keyed spring anyone can read while building the tree: pass the value
    /// you want, get the value to draw. A knob's sweep drawn from
    /// `ui.tween("cutoff", v)` glides when a preset changes it and still
    /// tracks a drag, because the spring is retargeted, never restarted.
    /// First call returns `target`, so nothing flies in from zero.
    pub fn tween(&mut self, id: impl Into<Id>, target: f64) -> f64 {
        let id: Id = id.into();
        let id = id.as_str();
        self.tween_with(Id::runtime(id), target, Spring::DEFAULT)
    }
    /// [`Ui::tween`] with your own spring. The spring's shape is taken on
    /// the first call for `id`.
    pub fn tween_with(&mut self, id: impl Into<Id>, target: f64, spring: Spring) -> f64 {
        let id: Id = id.into();
        let id = id.as_str();
        let reduced = self.reduced_motion();
        let (seen, s) = node(&mut self.nodes, id)
            .tween
            .get_or_insert_with(|| (false, spring.seeded(target)));
        *seen = true;
        s.to(target);
        if reduced {
            step_spring(s, 0., true);
        }
        let (value, rest) = (s.value, at_rest(s));
        self.reads(id, rest);
        value
    }
    /// A running memo closure read `id`; `rest` is whether what it read
    /// will still be true next frame.
    pub(super) fn reads(&mut self, id: &str, rest: bool) {
        if !self.building.is_empty() {
            self.read.push(id.to_owned());
            for b in &mut self.building {
                b.2 &= rest;
            }
        }
    }
    /// `keys` played on the runtime's clock from the first frame `id` is
    /// read: an entrance, an onboarding reveal, a pulse on a beat. The frame
    /// keeps reporting `animating` until the last key lands. Stop reading
    /// `id` for a frame and it starts over the next time it is read; see
    /// also [`Ui::replay`]. Reduced motion returns the final declared value
    /// immediately. A completed UI play lands exactly on that value, while
    /// direct `Keys::at` sampling can keep evaluating a spring past its last key.
    ///
    /// ```
    /// use mui::prelude::*;
    /// let mut ui = Ui::default();
    /// let intro = Keys::new(0.).to(0.5, 1., Ease::OUT);
    /// assert_eq!(ui.play("intro", &intro), 0.);
    /// ```
    pub fn play(&mut self, id: impl Into<Id>, keys: &Keys) -> f64 {
        let id: Id = id.into();
        let id = id.as_str();
        let reduced = self.reduced_motion();
        let now = self.time;
        let (seen, start) = node(&mut self.nodes, id).play.get_or_insert((false, now));
        *seen = true;
        if reduced {
            // Mark completion until replay or an unread frame. This avoids
            // subtract/add rounding restarting an entrance on preference changes.
            *start = f64::NEG_INFINITY;
            self.reads(id, true);
            return keys.target();
        }
        let end = *start + keys.end();
        if now < end {
            self.play_until = self.play_until.max(end);
        }
        let value = if now >= end {
            keys.target()
        } else {
            keys.at(now - *start)
        };
        self.reads(id, now >= end);
        value
    }
    /// Start `id`'s [`Ui::play`] over from its first key on the next frame.
    pub fn replay(&mut self, id: impl Into<Id>) {
        let id: Id = id.into();
        let id = id.as_str();
        if let Some(n) = self.nodes.get_mut(id) {
            n.play = None;
        }
    }
    /// Retarget and step the hover and press springs. Returns whether one is
    /// still moving.
    pub(super) fn hover_springs(&mut self, root: &El, dt: f64) -> bool {
        let reduced = self.reduced_motion();
        let (hovered, held) = (self.interaction.hovered(), self.interaction.held());
        // Identity and state ownership are separate. A named layout surface
        // still participates in hit testing, while only an interactive role
        // or an explicitly declared hover/press look earns springs. Resolve
        // the two possible active targets directly so idle frames do not
        // allocate a policy table for the whole tree.
        let [hovered_policy, held_policy] = state_policy(root, [hovered, held]);
        let mut animating = false;
        for (k, n) in &mut self.nodes {
            let Some([h, p]) = &mut n.springs else {
                continue;
            };
            let hovered = hovered == Some(k.as_str());
            let held = held == Some(k.as_str());
            h.to(f64::from(
                (hovered && hovered_policy[0]) || (held && held_policy[0]),
            ));
            p.to(f64::from(held && held_policy[1]));
            animating |= step_spring(h, dt, reduced) | step_spring(p, dt, reduced);
        }
        // A target that just earned springs starts them from rest, stepped
        // as the rest were.
        let mut start = |k: &str, on: [bool; 2]| {
            let n = node(&mut self.nodes, k);
            if n.springs.is_none() {
                let mut s = [Spring::at(0.0), Spring::at(0.0)];
                for (s, on) in s.iter_mut().zip(on) {
                    s.to(f64::from(on));
                    animating |= step_spring(s, dt, reduced);
                }
                n.springs = Some(s);
            }
        };
        if let Some(k) = hovered.filter(|_| hovered_policy[0]) {
            start(k, [true, held == Some(k) && held_policy[1]]);
        }
        if let Some(k) = held.filter(|_| held_policy[0] || held_policy[1]) {
            start(k, held_policy);
        }
        animating
    }

    /// Style the tree by state and step every transition and tween. Returns
    /// whether one is still moving, and the shapes the styled tree declares.
    pub(super) fn sweep(&mut self, root: &mut El, dt: f64) -> (bool, Shapes) {
        let reduced = self.reduced_motion();
        let pal = self.theme.palette;
        // Scrolls step before the walk slides the tree by them.
        let mut animating = false;
        for n in self.nodes.values_mut() {
            if let Some((_, s)) = &mut n.tween {
                // The tree already drew the value before this step: a step
                // that snaps onto the target still owes the frame that shows it.
                let drawn = s.value;
                animating |= step_spring(s, dt, reduced) || s.value != drawn;
            }
            for s in n.scroll.iter_mut().flatten() {
                animating |= step_spring(s, dt, reduced);
            }
        }
        let mut path = std::mem::take(&mut self.path);
        path.clear();
        let mut sweep = Sweep {
            pal: &pal,
            focus: self.focus.as_deref(),
            focus_visible: self.focus_visible,
            nodes: &mut self.nodes,
            dt,
            reduced,
            shaped: Shapes::default(),
        };
        animating |= sweep.node(root, &mut path, false);
        self.path = path;
        (animating, sweep.shaped)
    }

    /// What motion does to a resolved scene: shapes that changed name morph,
    /// and nodes that appeared last frame and are gone now fade out where
    /// they stood. Returns whether either is still moving.
    pub(super) fn after_motion(
        &mut self,
        scene: &mut ResolvedScene,
        shaped: Shapes,
        dt: f64,
    ) -> bool {
        let reduced = self.reduced_motion();
        if reduced {
            self.ghosts.clear();
        }
        let mut animating = false;
        for (key, shape, spring) in shaped.morphs {
            let Some(surface) = scene.surface(&key) else {
                continue;
            };
            // Local to the surface's offset, as every layer along it is.
            let target = surface.path.clone();
            let target_local = mui_geometry::Path::clone(&target);
            let m = node(&mut self.nodes, &key).morph.get_or_insert_with(|| {
                Box::new(Morph {
                    seen: false,
                    shape,
                    from: None,
                    shown: target_local.clone(),
                    t: spring.seeded(1.),
                })
            });
            m.seen = true;
            if m.shape != shape {
                m.shape = shape;
                m.from = Some(std::mem::take(&mut m.shown));
                m.t = spring.seeded(0.);
                m.t.to(1.);
            }
            let Some(from) = &m.from else {
                m.shown = target_local;
                continue;
            };
            let moving = step_spring(&mut m.t, dt, reduced);
            animating |= moving;
            m.shown = match mui_geometry::morph(from, &target_local, m.t.value) {
                Ok(p) if moving => p,
                _ => {
                    m.from = None;
                    target_local
                }
            };
            if m.from.is_none() {
                continue;
            }
            let world = Arc::new(m.shown.clone());
            // ponytail: the hit shape and any analytic shadow stay the target's
            // for the few frames a morph lasts; shells, being offsets of the
            // outline, snap. Offset them per frame if a morph ever lingers.
            for p in &mut scene.paint {
                if *p.key == *key
                    && Arc::ptr_eq(&p.path, &target)
                    && matches!(
                        p.layer,
                        Layer::Fill | Layer::Stroke | Layer::Clip | Layer::Mask
                    )
                {
                    p.path = world.clone();
                    p.rect = None;
                }
            }
        }
        if reduced {
            return false;
        }
        // Gone this frame: last frame's paint of every appearing node that
        // left, kept to fade. One that came back is simply there again; one
        // this frame declared was seen.
        if let Some(last) = &self.scene {
            let mut gone: Vec<&str> = (self.nodes.iter())
                .filter(|(k, n)| {
                    n.appearing.is_some_and(|(seen, _)| !seen)
                        && named(k)
                        && scene.surface(k).is_none()
                })
                .map(|(k, _)| k.as_str())
                .collect();
            gone.sort_unstable();
            for key in gone {
                let Some((_, spring)) = self.nodes.get(key).and_then(|n| n.appearing) else {
                    continue;
                };
                if let Ok(only) = last.isolate(&[key]) {
                    self.ghosts.push(Ghost {
                        key: key.to_owned(),
                        paint: only.paint,
                        fade: spring.seeded(1.),
                    });
                }
            }
        }
        self.ghosts.retain(|g| scene.surface(&g.key).is_none());
        for g in &mut self.ghosts {
            g.fade.to(0.);
            animating |= g.fade.step(dt);
        }
        self.ghosts.retain(|g| g.fade.value > 1e-3);
        // ponytail: ghosts paint on top of everything rather than at their
        // old depth; interleave them into the paint list if a fade ever
        // visibly crosses a neighbour.
        for g in &self.ghosts {
            let group = |layer| Painted {
                key: g.key.as_str().into(),
                layer,
                path: Arc::default(),
                paint: Paint::Solid(Color::oklcha(0., 0., 0., 0.)),
                rect: None,
                offset: Point::ZERO,
                transform: mui_geometry::kurbo::Affine::IDENTITY,
                width: 0.,
                blur: 0.,
                text: None,
            };
            scene.paint.push(group(Layer::Blend {
                mix: Mix::Normal,
                opacity: g.fade.value.clamp(0., 1.) as f32,
            }));
            scene.paint.extend(g.paint.iter().cloned());
            scene.paint.push(group(Layer::Unblend));
        }
        animating
    }
}

/// Channel ids. Fixed per field, never by position in a list, so a node
/// gaining a shadow does not hand its shells' springs to another channel.
pub(super) const FILL: u32 = 0;
pub(super) const STROKE_WIDTH: u32 = 4;
pub(super) const RADIUS: u32 = 5;
pub(super) const TEXT_SIZE: u32 = 6;
pub(super) const OPACITY: u32 = 7;
pub(super) const INSIDE: u32 = 8;
pub(super) const BEND: u32 = 9;
pub(super) const RAMP: u32 = 10;
pub(super) const STROKE: u32 = 12;
pub(super) const WELD_PROGRESS: u32 = 16;
/// Per-item blocks: gradient stop `i` at `STOPS + 8 i` (offset, then
/// colour), shadow `i` at `SHADOWS + 8 i` (blur, dx, dy, spread, colour),
/// shell `i` at `SHELLS + 8 i` (depth, colour).
pub(super) const STOPS: u32 = 0x100;
pub(super) const SHADOWS: u32 = 0x1000;
pub(super) const SHELLS: u32 = 0x2000;

/// Every numeric paint channel of `e`, each under its stable id, replaced by
/// `ch(id, declared, is_angle)`. Shape padding/bend and border widths share
/// this clock. Colours spring in Oklch, one channel per component.
pub(super) fn channels(e: &mut Element, pal: &Palette, ch: &mut impl FnMut(u32, f64, bool) -> f64) {
    fn color(c: Color, base: u32, ch: &mut impl FnMut(u32, f64, bool) -> f64) -> Color {
        let v = [c.lightness(), c.chroma(), c.hue(), c.alpha()];
        let o: [f32; 4] =
            std::array::from_fn(|i| ch(base + i as u32, f64::from(v[i]), i == 2) as f32);
        Color::oklcha(o[0].clamp(0., 1.), o[1].max(0.), o[2], o[3].clamp(0., 1.))
    }
    let under = pal.background();
    match &mut e.style.fill {
        None => {}
        Some(Fill::Gradient(g)) => {
            for (i, (at, f)) in g.stops.iter_mut().enumerate() {
                let base = STOPS + 8 * i as u32;
                if let Some(Paint::Solid(c)) = f.paint(pal, under) {
                    *f = Fill::Color(color(c, base + 1, ch));
                }
                *at = (ch(base, f64::from(*at), false) as f32).clamp(0., 1.);
            }
        }
        Some(fill) => {
            if let Some(Paint::Solid(c)) = fill.paint(pal, under) {
                *fill = Fill::Color(color(c, FILL, ch));
            }
        }
    }
    if let Some(s) = e.style.stroke.as_mut() {
        if let Some(Paint::Solid(c)) = s.fill.as_ref().and_then(|f| f.paint(pal, under)) {
            s.fill = Some(Fill::Color(color(c, STROKE, ch)));
        }
        if let Some(w) = s.width.as_mut() {
            *w = ch(STROKE_WIDTH, *w, false).max(0.0);
        }
    }
    for (i, s) in e.style.shadow.iter_mut().flatten().enumerate() {
        let base = SHADOWS + 8 * i as u32;
        s.blur = ch(base, s.blur, false).max(0.0);
        s.dx = ch(base + 1, s.dx, false);
        s.dy = ch(base + 2, s.dy, false);
        s.spread = ch(base + 3, s.spread, false);
        if let Some(Paint::Solid(c)) = s.fill.paint(pal, under) {
            s.fill = Fill::Color(color(c, base + 4, ch));
        }
    }
    for (i, (d, f)) in e.style.shells.iter_mut().flatten().enumerate() {
        let base = SHELLS + 8 * i as u32;
        if let Spacing::Px(v) = d {
            *v = ch(base, *v, false).max(0.0);
        }
        if let Some(Paint::Solid(c)) = f.paint(pal, under) {
            *f = Fill::Color(color(c, base + 1, ch));
        }
    }
    if let Some(Radius::Px(r)) = &mut e.style.radius {
        *r = ch(RADIUS, *r, false).max(0.0);
    }
    if let Some(t) = e.text_size.as_mut() {
        *t = ch(TEXT_SIZE, *t, false).max(0.0);
    }
    // Always a channel, so a node that never declared an opacity can still
    // fade in; the layer is only added while it is actually translucent.
    let (mix, o) = e.style.layer.unwrap_or((Mix::Normal, 1.0));
    let o = ch(OPACITY, f64::from(o), false).clamp(0., 1.) as f32;
    if e.style.layer.is_some() || o < 0.999 {
        e.style.layer = Some((mix, o));
    }
    if let Some(Spacing::Px(pad)) = e.extras.as_deref_mut().and_then(|x| x.inside.as_mut())
        && pad.is_finite()
        && *pad >= 0.
    {
        *pad = ch(INSIDE, *pad, false).max(0.);
    }
    if e.bend.is_finite() && e.bend.abs() <= 0.45 {
        e.bend = ch(BEND, e.bend, false).clamp(-0.45, 0.45);
    }
    if let Some(ramp) = e.extras.as_deref_mut().and_then(|x| x.border_ramp.as_mut()) {
        for (i, width) in [&mut ramp.from.1, &mut ramp.to.1].into_iter().enumerate() {
            if width.is_finite() && *width >= 0. {
                *width = ch(RAMP + i as u32, *width, false).max(0.);
            }
        }
    }
    // Leave invalid progress for scene resolution to reject.
    if let Some(w) = e
        .extras
        .as_deref_mut()
        .and_then(|x| x.welding.as_mut())
        .filter(|w| (0.0..=1.0).contains(&w.progress))
    {
        w.progress = ch(WELD_PROGRESS, w.progress, false).clamp(0.0, 1.0);
    }
}

/// Spring every transitioning node's paint toward what it declared this
/// frame. The springs hold last frame's declaration as their target, so a new
/// target mid-flight retargets the live spring instead of restarting it.
pub(super) fn transitions(
    n: &mut El,
    motion: &mut Option<Channels>,
    pal: &Palette,
    dt: f64,
    reduced: bool,
) -> bool {
    let mut animating = false;
    if let Some(spring) = n.payload().extras().transition {
        let fresh = motion.is_none();
        let (seen, list) = motion.get_or_insert_with(|| (false, Vec::new()));
        *seen = true;
        // An appearing node's very first frame starts from transparent.
        let from_clear = fresh && n.payload().extras().appear.is_some();
        let coupled_gap = n
            .payload()
            .extras()
            .inside
            .is_some_and(|p| p == *n.gap_mut());
        channels(n.payload_mut(), pal, &mut |id, declared, angle| {
            let i = if let Some(i) = list.iter().position(|(k, _)| *k == id) {
                i
            } else {
                let seed = if from_clear && id == OPACITY {
                    0.
                } else {
                    declared
                };
                list.push((id, spring.seeded(seed)));
                list.len() - 1
            };
            let s = &mut list[i].1;
            // Hue is an angle: take the short way round rather than
            // sweeping 350 degrees back to 10.
            if angle {
                s.value += ((declared - s.value) / 360.0).round() * 360.0;
            }
            s.to(declared);
            animating |= step_spring(s, dt, reduced);
            s.value
        });
        // A channel the node stopped declaring (a shadow removed, a stop
        // dropped) has no target any more: forget it.
        // ponytail: `channels` would need to report the ids it visited to
        // prune these per frame; they are dropped with the node instead.
        if coupled_gap {
            *n.gap_mut() = n.payload().extras().inside.expect("coupled inside");
        }
    }
    animating
}

/// Where an [`Appear`]ing node's frame springs start on its first frame, as
/// `[x, y, w, h]`; anything else starts where layout put it.
pub(super) fn enter(spring: Spring, t: [f64; 4], appear: Option<Appear>) -> [Spring; 4] {
    let [x, y, w, h] = t;
    let from = match appear {
        Some(Appear::Scale(f)) if f.is_finite() => {
            let f = f.max(0.);
            [x + w * (1. - f) / 2., y + h * (1. - f) / 2., w * f, h * f]
        }
        Some(Appear::Slide(dx, dy)) if dx.is_finite() && dy.is_finite() => [x + dx, y + dy, w, h],
        _ => t,
    };
    from.map(|v| spring.seeded(v))
}

/// One node's paint springs, by channel id, and whether this frame saw it.
pub(super) type Channels = (bool, Vec<(u32, Spring)>);

/// A node that left the tree: the paint it last had, fading.
pub(super) struct Ghost {
    pub(super) key: String,
    pub(super) paint: Vec<Painted>,
    pub(super) fade: Spring,
}

/// A [`Styled::morph`](mui_scene::Styled::morph) node: the shape name it
/// last had, the outline it is morphing from (node-local), what it showed
/// last frame, and the spring running `0..1` between.
pub(super) struct Morph {
    pub(super) seen: bool,
    pub(super) shape: u64,
    pub(super) from: Option<mui_geometry::Path>,
    pub(super) shown: mui_geometry::Path,
    pub(super) t: Spring,
}

/// What this frame's tree asks of motion beyond its paint: every morphing
/// node, its shape name and spring.
#[derive(Default)]
pub(super) struct Shapes {
    pub(super) morphs: Vec<(String, u64, Spring)>,
}

/// One walk styling the tree: per node its declared state looks first, so a
/// transition springs toward the style the node actually asked for this
/// frame, then the automatic hover, press and scroll. Each node's key is its
/// id or its tree path, exactly as the scene's.
pub(super) struct Sweep<'a> {
    pub(super) pal: &'a Palette,
    /// Who holds the keyboard focus.
    pub(super) focus: Option<&'a str>,
    /// ... and whether it came from the keyboard or code.
    pub(super) focus_visible: bool,
    pub(super) nodes: &'a mut Nodes,
    pub(super) dt: f64,
    pub(super) reduced: bool,
    /// What morphs, gathered on the way past.
    pub(super) shaped: Shapes,
}
impl Sweep<'_> {
    /// Returns whether a transition is still moving.
    fn node(&mut self, n: &mut El, path: &mut String, off: bool) -> bool {
        // A reused memo is last frame's styled subtree, and nothing in it
        // moved: styling it again would apply its states twice.
        if n.payload().extras().memo.is_some_and(|m| m.reused) {
            return false;
        }
        let off = off || n.payload().has(Element::DISABLED);
        // One lookup per node: its key is its id or its tree path.
        let key = n.key().unwrap_or(path);
        let e = n.payload().extras();
        let (transition, appear, morph) = (e.transition, e.appear, e.morph);
        let focused = self.focus == Some(key);
        let mut st = if transition.is_some() || appear.is_some() {
            Some(node(self.nodes, key))
        } else {
            self.nodes.get_mut(key)
        };
        let springs = st.as_ref().and_then(|s| s.springs);
        declared_states(n, springs, [focused, focused && self.focus_visible], off);
        let mut animating = st
            .as_mut()
            .is_some_and(|s| transitions(n, &mut s.motion, self.pal, self.dt, self.reduced));
        state(
            n,
            self.pal,
            springs,
            st.as_ref().and_then(|s| s.scroll),
            off,
        );
        let spring = transition.unwrap_or(Spring::DEFAULT);
        if let (Some(s), Some(_)) = (st, appear) {
            s.appearing = Some((true, spring));
        }
        if let Some(shape) = morph {
            let key = n.key().unwrap_or(path).to_owned();
            self.shaped.morphs.push((key, shape, spring));
        }
        let mark = path.len();
        for (j, c) in n.children_mut().iter_mut().enumerate() {
            push_index(path, j);
            animating |= self.node(c, path, off);
            path.truncate(mark);
        }
        animating
    }
}

/// A spring that will not move again until something retargets it.
pub(super) fn at_rest(s: &Spring) -> bool {
    s.value == s.target && s.velocity == 0.0
}
