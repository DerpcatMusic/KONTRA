//! What the rack shows of a part below its header: why it is silent or
//! incomplete, then the library's own interface (drawn from its UI IR, as
//! authored or with our controls) or, without one, what its load decoded and
//! could not translate.

use super::{Cx, inside, ir_view, theme::*};
use moose::mui::mui::prelude::*;
use sampler_ui_ir::{self as ir, Presentation};
use std::sync::Arc;

/// One current raw materialization per source, never a history of publications.
#[derive(Default)]
struct SourceProjection {
    patch: ir::InterfacePatch,
    raw: Option<ir::Interface>,
}

impl SourceProjection {
    fn interface<'a>(&'a self, base: &'a ir::Interface) -> &'a ir::Interface {
        self.raw.as_ref().unwrap_or(base)
    }

    /// Returns (materialized, bindings changed). Empty patches borrow the base.
    fn update(&mut self, base: &ir::Interface, patch: &ir::InterfacePatch) -> (bool, bool) {
        if self.patch == *patch {
            return (false, false);
        }
        // ponytail: clone/apply only a changed source; sparse in-place updates if profiling warrants.
        let raw = (patch != &ir::InterfacePatch::default()).then(|| {
            let mut raw = base.clone();
            patch.apply(base, &ir::InterfacePatch::default(), &mut raw);
            raw
        });
        let before = self.interface(base);
        let after = raw.as_ref().unwrap_or(base);
        let bindings_changed = !same_bindings(before, after);
        let materialized = raw.is_some();
        self.raw = raw;
        self.patch.clone_from(patch);
        (materialized, bindings_changed)
    }
}

// Native connection names/locations are fixed when its Session starts.
fn same_bindings(a: &ir::Interface, b: &ir::Interface) -> bool {
    a.source == b.source
        && a.widgets.len() == b.widgets.len()
        && a.widgets.iter().zip(&b.widgets).all(|(a, b)| {
            a.name == b.name && a.source_id == b.source_id && a.binding == b.binding
        })
}

/// A part's interface as drawn: resolved, with the pictures it needs.
pub struct Face {
    /// The interfaces it was made from, to notice a reload.
    from: Arc<[ir::Interface]>,
    sources: Vec<SourceProjection>,
    updates: Arc<[ir::InterfacePatch]>,
    path: std::path::PathBuf,
    program: u32,
    generation: u64,
    revision: u64,
    patch: ir::InterfacePatch,
    pub page: ir::PageRef,
    /// Which of them is shown.
    pub shown: usize,
    face: ir::Interface,
    assets: ir_view::Assets,
    pub presentation: Presentation,
    values: ir_view::Values,
    pub input: ir_view::InputState,
    native: Option<super::native_ui::State>,
}

impl Face {
    fn new(
        path: &std::path::Path,
        generation: u64,
        from: Arc<[ir::Interface]>,
        shown: usize,
        presentation: Presentation,
    ) -> Self {
        Self::new_for_program(path, generation, from, shown, presentation, 0)
    }

    fn new_for_program(
        path: &std::path::Path,
        generation: u64,
        from: Arc<[ir::Interface]>,
        shown: usize,
        presentation: Presentation,
        program: u32,
    ) -> Self {
        let face = ir_view::resolved(&from[shown]);
        let mut out = Self {
            sources: (0..from.len()).map(|_| SourceProjection::default()).collect(),
            updates: Arc::from([]),
            from,
            path: path.into(),
            program,
            generation,
            revision: u64::MAX,
            patch: Default::default(),
            page: ir::PageRef(0),
            shown,
            face,
            assets: Default::default(),
            presentation,
            values: Default::default(),
            input: Default::default(),
            native: None,
        };
        out.sync();
        out
    }

    /// Exact retained publication identity makes shell-only builds O(1) here.
    /// The return value counts only newly materialized sources and their widgets.
    fn publish(
        &mut self,
        from: Arc<[ir::Interface]>,
        updates: Arc<[ir::InterfacePatch]>,
        fallback: usize,
    ) -> (usize, usize) {
        let replaced = !Arc::ptr_eq(&self.from, &from);
        if !replaced && Arc::ptr_eq(&self.updates, &updates) {
            return (0, 0);
        }
        let previous = if replaced {
            let sources = (0..from.len())
                .map(|_| SourceProjection::default())
                .collect();
            let previous = (
                std::mem::replace(&mut self.from, from),
                std::mem::replace(&mut self.sources, sources),
            );
            if self.shown >= self.from.len() {
                self.shown = fallback;
                self.page = ir::PageRef(0);
                self.input = Default::default();
            }
            Some(previous)
        } else {
            None
        };
        let mut work = (0, 0);
        for (n, (source, projection)) in self.from.iter().zip(&mut self.sources).enumerate() {
            let (materialized, bindings_changed) =
                projection.update(source, updates.get(n).unwrap_or(&Default::default()));
            if materialized {
                work.0 += 1;
                work.1 += projection.interface(source).widgets.len();
            }
            if bindings_changed && !replaced {
                self.native = None;
            }
        }
        if let Some((bases, sources)) = previous {
            let same = bases.len() == self.from.len()
                && bases.iter().zip(&sources)
                    .zip(self.from.iter().zip(&self.sources))
                    .all(|((a, old), (b, new))| {
                        same_bindings(old.interface(a), new.interface(b))
                    });
            if !same {
                self.native = None;
            }
            // Equivalent bindings preserve Lua state/drafts; changed names/count/source reseed.
        }
        self.updates = updates;
        if replaced {
            self.face = ir_view::resolved(self.sources[self.shown].interface(&self.from[self.shown]));
            self.patch = self.sources[self.shown].patch.clone();
            self.page.0 = self.page.0.min(self.face.pages.len().saturating_sub(1));
            self.sync();
        } else {
            if self.patch != self.sources[self.shown].patch {
                self.update(self.sources[self.shown].patch.clone());
            }
            // A different source can change Native bindings without changing the shown patch.
            if self.native.is_none() && self.face.native_ui.is_some() {
                self.sync();
            }
        }
        work
    }

    fn show(&mut self, shown: usize) {
        self.shown = shown;
        self.page = ir::PageRef(0);
        self.input = Default::default();
        self.face = ir_view::resolved(self.sources[shown].interface(&self.from[shown]));
        self.patch = self.sources[shown].patch.clone();
        self.sync();
    }

    fn update(&mut self, patch: ir::InterfacePatch) {
        if patch == self.patch {
            return;
        }
        let mut changed: Vec<_> = self
            .patch
            .widgets
            .iter()
            .chain(&patch.widgets)
            .map(|(n, _)| *n)
            .collect();
        changed.sort_unstable();
        changed.dedup();
        let (_, bindings_changed) = self.sources[self.shown].update(&self.from[self.shown], &patch);
        if bindings_changed {
            self.native = None;
        }
        let raw = self.sources[self.shown].interface(&self.from[self.shown]);
        if patch.assets != self.patch.assets
            || patch.pages != self.patch.pages
            || patch.source != self.patch.source
            || patch.widget_count != self.patch.widget_count
        {
            // Image-size removal/reversion must normalize authored geometry, not old pixels.
            self.face = ir_view::resolved(raw);
        } else {
            patch.apply(&self.from[self.shown], &self.patch, &mut self.face);
            for &n in &changed {
                if let (Some(widget), Some(authored)) =
                    (self.face.widgets.get_mut(n), raw.widgets.get(n))
                {
                    widget.clone_from(authored);
                }
            }
            ir_view::resolve_changed(&mut self.face, changed);
        }
        self.patch = patch;
        self.page.0 = self.page.0.min(self.face.pages.len().saturating_sub(1));
        self.sync();
    }

    /// Decoded picture bytes the view keeps.
    pub fn bytes(&self) -> usize {
        self.assets.bytes() + self.native.as_ref().map_or(0, |n| n.bytes())
    }

    fn sync(&mut self) {
        self.assets.epoch += 1;
        let entry = self.face.native_ui.as_ref().map(|n| n.entry.as_str());
        if self.native.as_ref().map(|n| n.entry()) != entry {
            self.native = entry.map(|entry| {
                super::native_ui::State::new_for_program(
                    &self.path,
                    self.program,
                    entry,
                    self.from
                        .iter()
                        .zip(&self.sources)
                        .flat_map(|(base, projection)| {
                            let f = projection.interface(base);
                            f.widgets
                                .iter()
                                .enumerate()
                                .map(move |(n, w)| (f.source, n, w.clone()))
                        })
                        .collect(),
                )
            });
        }
        if self.native.is_some() && self.presentation == Presentation::Bitmap {
            return;
        }
        self.assets.prepare(
            &self.path,
            &self.face,
            self.page,
            self.presentation,
            1.,
            &self.values,
        );
    }
}

pub(super) fn view_available(face: &ir::Interface, patch: Option<&ir::InterfacePatch>) -> bool {
    patch.and_then(|p| p.performance).unwrap_or(face.performance)
        || patch.and_then(|p| p.native_ui.as_ref()).unwrap_or(&face.native_ui).is_some()
        || patch.and_then(|p| p.widget_count).unwrap_or(face.widgets.len()) > 0
}

/// A NativeUI request owns the composition; its controls may live in another
/// script's classic performance view. Declaration order breaks ties.
fn main_face(faces: &[ir::Interface], updates: &[ir::InterfacePatch]) -> Option<usize> {
    (0..faces.len()).find(|&n| updates.get(n).and_then(|p| p.native_ui.as_ref()).unwrap_or(&faces[n].native_ui).is_some())
        .or_else(|| (0..faces.len()).find(|&n| updates.get(n).and_then(|p| p.performance).unwrap_or(faces[n].performance)))
        .or_else(|| (0..faces.len()).find(|&n| view_available(&faces[n], updates.get(n))))
}

/// One resolver for saved v1 overrides and the global preference.
pub fn mode(cx: &Cx, slot: usize) -> crate::library::ViewMode {
    resolve_mode(&cx.selection.parts[slot], &cx.settings)
}

fn resolve_mode(
    part: &crate::plugin::Part,
    settings: &crate::library::Settings,
) -> crate::library::ViewMode {
    use crate::library::ViewMode;
    match part.view {
        1 => ViewMode::Original,
        2 => ViewMode::Kontra,
        3 => ViewMode::Vectorized,
        _ => settings
            .instrument_views
            .get(&part.path)
            .copied()
            .unwrap_or(settings.view_mode),
    }
}

pub fn available(cx: &Cx, slot: usize) -> bool {
    let part = &cx.view.parts[slot];
    main_face(&part.interfaces, &part.updates).is_some()
}

pub(super) fn scale_to_fit(room: Size, authored: Size, setting: f32) -> f64 {
    if setting.is_finite() && setting > 0. {
        return f64::from(setting);
    }
    if room.width <= 0. || authored.width <= 0. {
        return 1.;
    }
    let mut fit = room.width / authored.width;
    if room.height > 0. && authored.height > 0. {
        fit = fit.min(room.height / authored.height);
    }
    fit
}

pub(super) fn room_height(ui: &Ui, slot: usize) -> f64 {
    let Some(scene) = ui.scene() else { return 0. };
    let (Some(rack), Some(part), Some(stage)) = (
        scene.surface("rack-view"),
        scene.surface(&format!("part-{slot}")),
        scene.surface(&format!("stage-{slot}")),
    ) else {
        return 0.;
    };
    let tabs = scene
        .surface(&format!("face-bar-{slot}"))
        .map_or(0., |s| s.frame.size.height);
    // Less the one-point rule at the part's foot, so a lone part ends exactly at the
    // rack's bottom edge (its resize edge stays inside the view, nothing scrolls).
    (rack.frame.size.height - (stage.frame.y - part.frame.y).max(0.) - tabs - 1.).max(0.)
}

pub(super) fn interaction(edit: &ir_view::Edit) -> sampler_core::WidgetInteraction {
    sampler_core::WidgetInteraction {
        index: edit.index,
        cursor: edit.cursor,
        event: edit.event,
        mouse_over: edit.mouse_over,
        modifiers: u8::from(edit.mods.shift)
            | (u8::from(edit.mods.ctrl || edit.mods.cmd) << 1)
            | (u8::from(edit.mods.alt) << 2),
        ..Default::default()
    }
}

/// `slot`'s library interface, when its scripts declare one.
fn interface(ui: &mut Ui, cx: &mut Cx, slot: usize, lead: Option<El>) -> Option<El> {
    use crate::plugin::ui_activity::{Count, Phase};
    let activity = cx.p.shared.ui_activity.clone();
    let projection = activity.span(Phase::UiProjectionSync);
    let from = cx.view.parts.get(slot)?.interfaces.clone();
    let main = main_face(&from, &cx.view.parts[slot].updates)?;
    let published = &cx.view.parts[slot];
    let path = published.resource_path.clone()
        .unwrap_or_else(|| std::path::PathBuf::from(&cx.selection.parts[slot].path));
    let generation = published.generation;
    let mode = mode(cx, slot);
    let presentation = if mode == crate::library::ViewMode::Original {
        Presentation::Bitmap
    } else {
        Presentation::Vector
    };
    let stale = cx.state.faces.get(&slot).is_none_or(|f| {
        f.path != path
            || f.generation != generation
            || f.program != cx.selection.parts[slot].program
    });
    if stale {
        activity.add(Count::FaceNew, 1);
        cx.state.faces.insert(
            slot,
            Face::new_for_program(
                &path,
                generation,
                from.clone(),
                main,
                presentation,
                cx.selection.parts[slot].program,
            ),
        );
    }
    let face = cx.state.faces.get_mut(&slot)?;
    if !Arc::ptr_eq(&face.from, &from)
        || !Arc::ptr_eq(&face.updates, &published.updates)
        || face.revision != published.ui_revision
    {
        activity.add(Count::FaceUpdates, 1);
        activity.add(
            Count::FaceNoops,
            u64::from(activity.enabled
                && Arc::ptr_eq(&face.from, &from)
                && published.updates.get(face.shown)
                    .map_or(face.patch == Default::default(), |patch| *patch == face.patch)),
        );
        let (sources, widgets) = face.publish(from.clone(), published.updates.clone(), main);
        activity.add(Count::NativeMaterializations, sources as u64);
        activity.add(Count::NativeWidgets, widgets as u64);
        face.revision = published.ui_revision;
    }
    if face.presentation != presentation {
        face.presentation = presentation;
        face.sync();
    }

    drop(projection);

    // Which script's view, when several have one, and how it is drawn.
    let mut bar: Vec<El> = lead.into_iter().collect();
    let with: Vec<usize> = (0..from.len()).filter(|&n| n == face.shown || view_available(&from[n], published.updates.get(n))).collect();
    let mut pick = None;
    if with.len() > 1 {
        let tabs: Vec<El> = with
            .iter()
            .map(|&n| {
                let label = from[n]
                    .pages
                    .first()
                    .map(|p| p.name.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| match from[n].source {
                        ir::Source::Ksp { slot } => format!("Script {}", slot + 1),
                        _ => format!("View {}", n + 1),
                    });
                let (hit, el) = latch(
                    ui,
                    format!("face-{slot}-{n}"),
                    &label,
                    "Show this script's view",
                    face.shown == n,
                );
                if hit {
                    pick = Some(n);
                }
                el
            })
            .collect();
        bar.push(segmented(tabs));
    }
    if face.face.pages.len() > 1 {
        let pages = face
            .face
            .pages
            .iter()
            .enumerate()
            .map(|(n, p)| {
                let label = if p.name.is_empty() {
                    format!("Page {}", n + 1)
                } else {
                    p.name.clone()
                };
                let (hit, el) = latch(
                    ui,
                    format!("face-page-{slot}-{n}"),
                    &label,
                    "Show this page",
                    face.page.0 == n,
                );
                if hit {
                    face.page = ir::PageRef(n);
                }
                el
            })
            .collect();
        bar.push(segmented(pages));
    }
    bar.push(spacer());
    let original = mode == crate::library::ViewMode::Original;
    let (a, orig) = latch(
        ui,
        format!("face-original-{slot}"),
        "Original",
        "The library's own artwork",
        original,
    );
    let (b, vect) = latch(
        ui,
        format!("face-vector-{slot}"),
        "Vector",
        "The library's background with KONTRA's controls; frees the control pictures",
        mode == crate::library::ViewMode::Vectorized,
    );
    let (c, generated) = latch(
        ui,
        format!("face-kontra-{slot}"),
        "KONTRA",
        "Readable sections and channel strips",
        mode == crate::library::ViewMode::Kontra,
    );
    bar.push(segmented(vec![orig, vect, generated]));
    if let Some(n) = pick.filter(|&n| n != face.shown) {
        face.show(n);
    }
    if a || b || c {
        cx.selection.parts[slot].view = if b {
            3
        } else if c {
            2
        } else {
            1
        };
        let chosen = if b {
            crate::library::ViewMode::Vectorized
        } else if c {
            crate::library::ViewMode::Kontra
        } else {
            crate::library::ViewMode::Original
        };
        cx.p.shared.libraries.edit(|settings| {
            settings
                .instrument_views
                .insert(path.to_string_lossy().into_owned(), chosen);
        });
        face.presentation = if a {
            Presentation::Bitmap
        } else {
            Presentation::Vector
        };
        face.sync();
    }

    let held = face.bytes() as f64 / (1024. * 1024.);
    bar.insert(
        bar.len() - 1,
        caption(format!("{held:.1} MB pictures"))
            .fill(secondary())
            .lines(1)
            .tip("Decoded artwork this view keeps in memory"),
    );
    let mode = if a {
        crate::library::ViewMode::Original
    } else if b {
        crate::library::ViewMode::Vectorized
    } else if c {
        crate::library::ViewMode::Kontra
    } else {
        mode
    };
    let page = &face.face.pages[face.page.0];
    let avail = ui
        .scene()
        .and_then(|s| s.surface(&format!("face-{slot}")))
        .map_or(f64::from(page.size.width), |s| s.frame.size.width);
    let room = room_height(ui, slot);
    let scale = scale_to_fit(
        Size::new(avail, room),
        Size::new(
            f64::from(page.size.width),
            f64::from(ir_view::height(&face.face, face.page)),
        ),
        cx.settings.view_scale,
    );
    // The core's values (scripts change them too); edits go back as widget edits.
    let shared = cx.p.shared.part(slot);
    face.assets.meter = shared.clone().map(|part| {
        Arc::new(move |_bus: Option<u32>, channel: u8| {
            let levels = part
                .meter
                .each_ref()
                .map(|v| f32::from_bits(v.load(std::sync::atomic::Ordering::Relaxed)));
            if channel == 0 {
                levels
            } else {
                [levels[(channel as usize).min(1)]; 2]
            }
        }) as Arc<dyn Fn(Option<u32>, u8) -> [f32; 2] + Send + Sync>
    });
    let readback = activity.span(Phase::UiControlReadback);
    let current: Vec<_> = shared
        .as_ref()
        .map(|p| p.display_values())
        .unwrap_or_default();
    face.values.extend(current.iter().copied());
    if let Some(shared) = &shared {
        face.input.values.extend(shared.widget_values(&face.face));
        face.input.meters = shared.widget_meters(&face.face, generation);
        let waveforms =
            shared.widget_waveforms(&face.face, generation, scale * ui.scale().unwrap_or(1.));
        face.input.wave_duration_us = waveforms
            .iter()
            .map(|(widget, envelope)| (*widget, envelope.duration_us))
            .collect();
        face.input.peaks = waveforms
            .into_iter()
            .map(|(widget, envelope)| (widget, envelope.peaks))
            .collect();
    }
    drop(readback);
    if face.native.is_none() || mode == crate::library::ViewMode::Kontra {
        face.assets.prepare(
            &face.path,
            &face.face,
            face.page,
            face.presentation,
            scale * ui.scale().unwrap_or(1.),
            &face.values,
        );
    }
    let namespace = format!("part-{slot}-epoch-{generation}-script-{}", face.shown);
    // Both authored presentations lower the evaluated graph to live MUI
    // primitives. Raw KSP rectangles are parameter handles, not this layout.
    let view = if mode != crate::library::ViewMode::Kontra
        && let Some(native) = &mut face.native
    {
        native.set_activity(&activity);
        let projection = activity.span(Phase::UiProjectionSync);
        for (source, projection) in face.from.iter().zip(&face.sources) {
            let current = projection.interface(source);
            let typed = shared
                .as_ref()
                .map(|shared| shared.widget_values(current))
                .unwrap_or_default();
            let meters = shared
                .as_ref()
                .map(|shared| shared.widget_meters(current, generation))
                .unwrap_or_default();
            native.update_view(current, &face.values, &typed, &meters);
        }
        drop(projection);
        let authored = native.authored();
        let scale = scale_to_fit(Size::new(avail, room), authored, cx.settings.view_scale);
        let view = native.view(
            ui,
            slot,
            scale,
            face.sources[face.shown].interface(&face.from[face.shown]),
            &face.values,
            &face.input,
        );
        for edit in native.edits() {
            let widget = face.from.iter()
                .zip(&face.sources)
                .map(|(base, projection)| projection.interface(base))
                .find(|f| f.source == edit.source)
                .and_then(|f| f.widgets.get(edit.widget.0));
            let admitted = widget.is_some_and(|widget| {
                let source_slot = match edit.source {
                    ir::Source::Ksp { slot } => slot,
                    _ => 0,
                };
                let admitted = cx.p.shared.set_widget_at(
                    slot,
                    generation,
                    source_slot,
                    widget,
                    edit.index,
                    edit.value.clone(),
                );
                #[cfg(all(test, feature = "shots"))]
                if admitted && slot == 0 {
                    let len = match &edit.value {
                        ir::Value::Integers(v) => v.len(),
                        ir::Value::Reals(v) => v.len(),
                        _ => 1,
                    };
                    let start = edit.index.unwrap_or(0);
                    let indices = (start..start + len)
                        .map(|index| index as u32)
                        .collect::<Vec<_>>();
                    super::widget_gate::submitted_widget(widget, &indices);
                }
                admitted
            });
            if !admitted {
                cx.state.notice = "This authored widget edit could not be applied.".into();
            }
        }
        view
    } else if mode == crate::library::ViewMode::Kontra {
        let _submission = activity.span(Phase::UiAuthoredLayoutSubmission);
        super::generated::view(
            ui,
            &namespace,
            &face.face,
            face.page,
            &face.assets,
            scale,
            &mut face.values,
            &mut face.input,
        )
    } else {
        let _submission = activity.span(Phase::UiAuthoredLayoutSubmission);
        ir_view::view_state(
            ui,
            &namespace,
            &face.face,
            face.page,
            &face.assets,
            face.presentation,
            scale,
            &mut face.values,
            &mut face.input,
        )
    };
    let mut edits: std::collections::HashMap<
        ir::WidgetRef,
        (
            std::collections::BTreeMap<u32, ir::Value>,
            sampler_core::WidgetInteraction,
        ),
    > = Default::default();
    for edit in face.input.edits.drain(..) {
        let entry = edits.entry(edit.widget).or_default();
        entry.1 = interaction(&edit);
        entry.0.insert(edit.index, edit.value);
    }
    let mut edited_controls = std::collections::HashSet::new();
    for (n, (edits, interaction)) in edits {
        let Some(widget) = face.face.widgets.get(n.0) else {
            continue;
        };
        if let ir::Binding::Control(id) = widget.binding {
            edited_controls.insert(id);
        }
        let source_slot = match face.face.source {
            ir::Source::Ksp { slot } => slot,
            _ => 0,
        };
        #[cfg(all(test, feature = "shots"))]
        let indices = edits.keys().copied().collect::<Vec<_>>();
        let admitted = cx.p.shared.set_widget_batch_at(
            slot,
            generation,
            source_slot,
            widget,
            edits.into_iter().collect(),
            interaction,
        );
        #[cfg(all(test, feature = "shots"))]
        if admitted && slot == 0 {
            super::widget_gate::submitted_widget(widget, &indices);
        }
        if !admitted {
            face.input.values.remove(&n);
        }
    }
    for &(id, was) in &current {
        if let Some(&now) = face.values.get(&id)
            && now != was
            && !edited_controls.contains(&id)
        {
            let _admitted = cx.p.shared.set_control_at(slot, generation, id, now);
            #[cfg(all(test, feature = "shots"))]
            if _admitted && slot == 0 {
                super::widget_gate::submitted_control(id);
            }
        }
    }
    Some(
        col![
            row(bar)
                .id(format!("face-bar-{slot}"))
                .gap(SPACE)
                .align(Align::Center)
                .pad((TIGHT, INSET))
                .w(Len::Pct(100.))
                .shrink(0),
            // Fills the part's room; the fitted view sits centred on both axes.
            row![spacer(), view, spacer()]
                .align(Align::Center)
                .w(Len::Pct(100.))
                .when(room > 0., |e| e.min_h(room))
                .shrink(0)
                .id(format!("face-{slot}"))
        ]
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .shrink(0),
    )
}

/// The empty rack.
pub fn welcome(cx: &Cx) -> El {
    let mut lines = vec![title("Pick an instrument").text_weight(Weight::SEMIBOLD)];
    if !cx.view.multi_status.is_empty() {
        lines.push(
            caption(cx.view.multi_status.clone())
                .fill(secondary())
                .lines(2),
        );
    }
    col![
        spacer(),
        col(lines).gap(SPACE).align(Align::Start),
        spacer()
    ]
    .align(Align::Center)
    .pad(INSET * 3.)
    .flex(1)
    .min_h(0)
}

/// Why `slot` is silent or incomplete, in words a player can act on.
pub fn notices(cx: &Cx, slot: usize) -> Option<El> {
    let v = &cx.view.parts[slot];
    let mut out = Vec::new();
    if let Some(reason) = v.status.strip_prefix("Load failed: ") {
        out.push(banner(
            Role::Danger,
            format!("This instrument could not be loaded: {reason}."),
        ));
    }
    if v.report
        .as_ref()
        .is_some_and(|r| r.missing.iter().any(|m| m.feature == "native interface"))
    {
        out.push(banner(
            Role::Warning,
            "The requested native performance view is unavailable. Showing the script controls.",
        ));
    }
    if let Some(p) = v.report.as_ref().map(|r| r.runtime) {
        if p.capacity_drops > 0 {
            out.push(banner(
                Role::Warning,
                format!(
                    "{} notes were dropped: the part ran out of voices.",
                    p.capacity_drops
                ),
            ));
        }
        if p.ignored_input > 0 {
            out.push(banner(
                Role::Warning,
                format!(
                    "{} MIDI messages this part does not play yet were ignored.",
                    p.ignored_input
                ),
            ));
        }
    }
    (!out.is_empty()).then(|| col(out).gap(0).align(Align::Stretch).shrink(0))
}

/// The instrument volume as the part's volume: the saved level until CC7
/// arrives, then CC7 cubed (`sampler_ir::HostVolume`).
pub fn volume_text(inst: &sampler_ir::Instrument) -> Option<String> {
    let v = inst.host_volume?;
    let db = if v.saved > 0. {
        20. * v.saved.log10()
    } else {
        f64::NEG_INFINITY
    };
    Some(if db.is_finite() {
        format!("CC{} {db:+.1} dB", v.controller)
    } else {
        format!("CC{} off", v.controller)
    })
}

/// The dynamics badge: the picked start (what the next load uses) wins over
/// the last load's value; at the default start it warns when the part waits.
pub fn badge_text(controllers: &str, picked: i16, loaded: u8, needs: bool) -> String {
    match u8::try_from(picked) {
        Ok(start) => format!("{controllers} starts at {start}"),
        Err(_) if needs => format!("Needs {controllers}"),
        Err(_) => format!("{controllers} starts at {loaded}"),
    }
}

/// What the part plays and listens to, as chips for the header's second row:
/// the articulation (with its effective trigger), the instrument volume and
/// the dynamics controller it waits for (one click sets where it starts).
fn performance(ui: &mut Ui, cx: &mut Cx, slot: usize) -> Vec<El> {
    let Some(v) = cx.view.parts.get(slot) else {
        return Vec::new();
    };
    let (inst, report) = (v.instrument.clone(), v.report.clone());
    if inst.is_none() && report.is_none() {
        return Vec::new();
    }
    let mut items = Vec::new();
    if let Some(inst) = inst.as_deref() {
        if let Some(n) = inside::active(cx, slot).filter(|&n| n < inst.articulations.len()) {
            let a = &inst.articulations[n];
            let id = &crate::sound::articulation::identities(&inst.articulations)[n];
            let input = cx.selection.parts[slot].articulation_overlay.input(
                id,
                a,
                inside::mode(cx, slot, inst),
            );
            let text = format!("{} · {}", a.name, inside::input_label(&input));
            items.push(
                row![
                    caption("Articulation").fill(secondary()).lines(1).shrink(0),
                    body(text.clone()).lines(1).flex(1).min_w(0)
                ]
                .gap(SPACE)
                .align(Align::Center)
                .shrink(0)
                .tip(text)
                .named("Articulation")
                .id(format!("perf-art-{slot}")),
            );
        }
        if let Some(text) = volume_text(inst) {
            items.push(
                row![caption("Volume").fill(secondary()), body(text).lines(1)]
                    .gap(SPACE)
                    .align(Align::Center)
                    .shrink(0)
                    .tip("The instrument's saved volume until CC7 arrives; then CC7 cubed")
                    .id(format!("perf-vol-{slot}")),
            );
        }
    }
    let dynamics = report
        .as_ref()
        .map(|r| r.decoded.dynamics.clone())
        .unwrap_or_default();
    let moving: Vec<_> = dynamics
        .iter()
        .filter(|&&(cc, _)| cc != 11)
        .map(|&(cc, _)| format!("CC{cc}"))
        .collect();
    if !moving.is_empty() {
        let now = cx.selection.parts[slot].dynamics;
        let mut picked = now;
        let tabs = [(-1, "Kontakt"), (64, "64"), (127, "127")]
            .into_iter()
            .map(|(value, label)| {
                let (hit, el) = latch(
                    ui,
                    format!("dyn-{slot}-{value}"),
                    label,
                    &format!("Start {} at {label}", moving.join("/")),
                    now == value,
                );
                if hit {
                    picked = value;
                }
                el
            })
            .collect();
        cx.selection.parts[slot].dynamics = picked;
        // The picked start is what the next load uses; the report shows the last load's.
        let loaded = dynamics
            .iter()
            .find(|&&(cc, _)| cc != 11)
            .map_or(0, |d| d.1);
        let needs = report.as_ref().is_some_and(|r| r.decoded.needs_controller);
        let words = badge_text(&moving.join("/"), picked, loaded, needs);
        let badge = if words.starts_with("Needs") {
            caption(words.clone()).fill(Role::Warning).tip(format!(
                "{words}\nNear-silent until the controller moves; set where it starts"
            ))
        } else {
            caption(words.clone()).fill(secondary()).tip(words)
        };
        items.push(
            row![
                badge
                    .lines(1)
                    .flex(1)
                    .min_w(0)
                    .id(format!("perf-needs-{slot}")),
                segmented(tabs).shrink(0)
            ]
            .gap(SPACE)
            .align(Align::Center)
            .flex(1)
            .min_w(CONTROL * 9.),
        );
    }
    items
}

/// The header's second row: the view switch (interface, articulations,
/// mapping, sound, info) then the performance chips, on one wrapping line.
/// The row is `None` when the part has neither.
pub fn header_row(ui: &mut Ui, cx: &mut Cx, slot: usize) -> (inside::View, Option<El>) {
    let (view, tabs) = inside::bar(ui, cx, slot);
    let items: Vec<El> = tabs.into_iter().chain(performance(ui, cx, slot)).collect();
    let row2 = (!items.is_empty()).then(|| {
        row(items)
            .wrap()
            .gap(SPACE * 2.)
            .line_gap(TIGHT)
            .align(Align::Center)
            .pad((INSET, 2.))
            .w(Len::Pct(100.))
            .shrink(0)
            .id(format!("header2-{slot}"))
    });
    (view, row2)
}

/// The picked view's body: its interface, articulations, mapping, sound or info.
pub fn stage(ui: &mut Ui, cx: &mut Cx, slot: usize, view: inside::View) -> El {
    let content = if view == inside::View::Interface
        && let Some(face) = interface(ui, cx, slot, None)
    {
        face
    } else {
        // Every tab reaches the part's bottom edge, like the interface does; taller
        // content still grows the part and scrolls the rack.
        let room = room_height(ui, slot);
        inside::view(ui, cx, slot, view)
            .map(|el| if room > 0. { el.min_h(room) } else { el })
            .unwrap_or_else(|| spacer().h(0))
    };
    col![content]
        .gap(0)
        .align(Align::Stretch)
        .w(Len::Pct(100.))
        .shrink(0)
        .id(format!("stage-{slot}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn projection_fixture(slot: u8) -> ir::Interface {
        let mut widget = ir::Widget::new(
            "$level",
            ir::PageRef(0),
            ir::Rect { x: 9.0, y: 7.0, width: 31.0, height: 17.0 },
            ir::Kind::Knob {
                range: ir::Range {
                    min: 0., max: 100., default: 25., step: Some(1.),
                },
                display: Default::default(),
            },
        );
        widget.source_id = Some(1);
        widget.placement = ir::Placement::Grid { column: 2, row: 2 };
        widget.auto_size = true;
        ir::Interface {
            source: ir::Source::Ksp { slot },
            performance: true,
            pages: vec![ir::Page {
                size: ir::Size { width: 480.0, height: 240.0 },
                height_rows: Some(3),
                ..Default::default()
            }],
            widgets: vec![widget],
            ..Default::default()
        }
    }

    #[test]
    fn shell_only_editor_repaints_do_not_rematerialize_authored_sources() {
        use crate::plugin::{
            SamplerParams,
            ui_activity::{Activity, Count, Phase},
        };
        let mut params = SamplerParams::new();
        params.shared.ui_activity = Arc::new(Activity::new(true));
        let p = Arc::new(params);
        p.shared.ensure_parts(1);
        p.selection.write().unwrap().parts.push(crate::plugin::Part {
            path: "/missing/synthetic-projection.nki".into(),
            view: 3,
            ..Default::default()
        });
        let base = projection_fixture(0);
        let mut current = base.clone();
        current.widgets[0].value_text = Some("Patched".into());
        {
            let mut view = p.shared.view.lock().unwrap();
            view.parts[0].interfaces = vec![base.clone()].into();
            view.parts[0].updates = vec![ir::InterfacePatch::between(&base, &current)].into();
        }
        let mut h = super::super::tests::Harness::new(&p, 1180., 780.);
        h.ui.focus("tab-rack");
        h.idle(4);
        let counts = p.shared.ui_activity.snapshot().unwrap();
        let phases = p.shared.ui_activity.phase_snapshot().unwrap();
        assert_eq!(counts[Count::NativeMaterializations as usize], 1);
        for n in 0..32 {
            p.shared.voices.store(n, std::sync::atomic::Ordering::Relaxed);
            // Key-look-only publications also change this broad revision.
            p.shared.view.lock().unwrap().parts[0].ui_revision += 1;
            h.idle(1);
        }
        let after = p.shared.ui_activity.snapshot().unwrap();
        let after_phases = p.shared.ui_activity.phase_snapshot().unwrap();
        assert_eq!(
            after[Count::NativeMaterializations as usize],
            counts[Count::NativeMaterializations as usize],
        );
        assert_eq!(after[Count::NativeWidgets as usize], counts[Count::NativeWidgets as usize]);
        for phase in [Phase::UiBuildFrame, Phase::UiControlReadback] {
            assert!(after_phases[phase as usize * 3] > phases[phase as usize * 3]);
        }
        assert!(h.ui.scene().unwrap().surface("part-0-epoch-0-script-0-ir-0").is_some());
        p.shared.view.lock().unwrap().parts[0].generation = 1;
        h.idle(2);
        assert_eq!(
            p.shared.ui_activity.snapshot().unwrap()[Count::NativeMaterializations as usize],
            2,
        );
        let scene = h.ui.scene().unwrap();
        assert!(scene.surface("part-0-epoch-1-script-0-ir-0").is_some());
        assert!(scene.surface("part-0-epoch-0-script-0-ir-0").is_none());
    }

    #[test]
    fn raw_projection_retains_unchanged_sources_and_releases_reverted_patches() {
        let bases: Arc<[ir::Interface]> = vec![projection_fixture(0), projection_fixture(1)].into();
        let mut a = bases[0].clone();
        a.widgets[0].rect.x = 123.0;
        let patch_a = ir::InterfacePatch::between(&bases[0], &a);
        let updates: Arc<[ir::InterfacePatch]> = vec![patch_a.clone(), Default::default()].into();
        let mut face = Face::new(
            std::path::Path::new("/missing/projection.nki"),
            7, bases.clone(), 0, Presentation::Vector,
        );
        assert_eq!(face.publish(bases.clone(), updates.clone(), 0), (1, 1));
        assert_eq!(face.sources[0].interface(&bases[0]), &a);
        assert_ne!(
            face.face, a,
            "raw grid/default-axis metadata is not resolved pixels",
        );
        let storage = face.sources[0].raw.as_ref().unwrap().widgets.as_ptr();
        for _ in 0..64 {
            assert_eq!(face.publish(bases.clone(), updates.clone(), 0), (0, 0));
            assert_eq!(face.sources[0].raw.as_ref().unwrap().widgets.as_ptr(), storage);
        }
        // New Arc with equal content is also a no-op, not a new projection.
        assert_eq!(face.publish(bases.clone(), updates.to_vec().into(), 0), (0, 0));
        let mut b = bases[1].clone();
        b.widgets[0].text = "Other source".into();
        assert_eq!(
            face.publish(
                bases.clone(),
                vec![patch_a, ir::InterfacePatch::between(&bases[1], &b)].into(),
                0,
            ),
            (1, 1),
        );
        assert_eq!(face.sources[0].raw.as_ref().unwrap().widgets.as_ptr(), storage);
        assert_eq!(face.sources[1].interface(&bases[1]), &b);
        face.show(1);
        assert_eq!(face.face, ir_view::resolved(&b));
        assert_eq!(face.publish(bases.clone(), Arc::from([]), 0), (0, 0));
        assert!(face.sources.iter().all(|source| source.raw.is_none()));
        assert!(std::ptr::eq(face.sources[1].interface(&bases[1]), &bases[1]));
        assert_eq!(face.face, ir_view::resolved(&bases[1]));
    }

    #[test]
    fn projection_matches_raw_and_resolved_oracles_for_assets_pages_and_widget_count() {
        let mut base = projection_fixture(0);
        base.assets.push(ir::Asset {
            path: "missing-strip.png".into(),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                size: Some(ir::Size { width: 101.0, height: 79.0 }),
                ..Default::default()
            }),
        });
        base.widgets[0].images.push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Strip));
        let bases: Arc<[ir::Interface]> = vec![base.clone()].into();
        let mut face = Face::new(
            std::path::Path::new("/missing/oracle.nki"),
            1, bases.clone(), 0, Presentation::Vector,
        );
        let mut variants = Vec::new();
        let mut current = base.clone();
        if let ir::AssetKind::Image(meta) = &mut current.assets[0].kind {
            meta.size = Some(ir::Size { width: 43.0, height: 29.0 });
        }
        variants.push(current);
        current = base.clone();
        current.assets.clear();
        variants.push(current);
        current = base.clone();
        current.pages[0].height_rows = Some(5);
        current.pages.push(current.pages[0].clone());
        variants.push(current);
        current = base.clone();
        let mut added = current.widgets[0].clone();
        added.name = "$second".into();
        current.widgets.push(added);
        variants.push(current);
        current = base.clone();
        current.widgets.clear();
        variants.push(current);
        current = base.clone();
        current.source = ir::Source::FalconLua;
        variants.push(current);
        for current in variants {
            let patch = ir::InterfacePatch::between(&base, &current);
            face.publish(bases.clone(), vec![patch].into(), 0);
            assert_eq!(face.sources[0].interface(&base), &current);
            assert_eq!(face.face, ir_view::resolved(&current));
            face.publish(bases.clone(), Arc::from([]), 0);
            assert_eq!(face.face, ir_view::resolved(&base), "reversion restores authored sizing");
        }
    }

    #[test]
    fn whole_base_identity_and_source_count_replace_projection_without_losing_local_page() {
        let mut base = projection_fixture(0);
        base.pages.push(base.pages[0].clone());
        let bases: Arc<[ir::Interface]> = vec![base.clone(), projection_fixture(1)].into();
        let mut face = Face::new(
            std::path::Path::new("/missing/identity.nki"),
            7, bases.clone(), 0, Presentation::Vector,
        );
        face.page = ir::PageRef(1);
        face.values.insert(ir::ControlId(9), 31.);
        face.input.values.insert(ir::WidgetRef(0), ir::Value::Text("Retained draft".into()));
        base.widgets[0].text = "New immutable base".into();
        let replaced: Arc<[ir::Interface]> = vec![base.clone(), projection_fixture(1)].into();
        face.publish(replaced.clone(), Arc::from([]), 0);
        assert!(Arc::ptr_eq(&face.from, &replaced));
        assert_eq!(face.face, ir_view::resolved(&base));
        assert_eq!(
            (face.shown, face.page, face.generation, face.presentation),
            (0, ir::PageRef(1), 7, Presentation::Vector),
        );
        assert_eq!(face.values[&ir::ControlId(9)], 31.);
        assert_eq!(
            face.input.values[&ir::WidgetRef(0)],
            ir::Value::Text("Retained draft".into()),
        );
        face.show(1);
        face.input.values.insert(ir::WidgetRef(0), ir::Value::Text("Retired script".into()));
        face.publish(vec![base].into(), Arc::from([]), 0);
        assert_eq!(face.sources.len(), 1);
        assert_eq!((face.shown, face.page), (0, ir::PageRef(0)));
        assert!(
            face.input.values.is_empty(),
            "removed script drafts cannot cross source identity",
        );
    }

    #[test]
    fn native_entry_synchronizes_even_when_replaced_base_has_an_empty_patch() {
        let mut base = projection_fixture(0);
        base.native_ui = Some(ir::NativeUi { entry: "old_entry".into() });
        let mut face = Face::new(
            std::path::Path::new("/missing/native-identity.nki"),
            1,
            vec![base.clone()].into(),
            0,
            Presentation::Bitmap,
        );
        assert_eq!(face.native.as_ref().unwrap().entry(), "old_entry");
        base.native_ui = Some(ir::NativeUi { entry: "new_entry".into() });
        face.publish(vec![base.clone()].into(), Arc::from([]), 0);
        assert_eq!(face.native.as_ref().unwrap().entry(), "new_entry");
        base.native_ui = None;
        face.publish(vec![base].into(), Arc::from([]), 0);
        assert!(face.native.is_none());
    }

    #[test]
    fn native_binding_identity_changes_only_for_connection_metadata() {
        let base = projection_fixture(0);
        let mut source = SourceProjection::default();
        let mut current = base.clone();
        current.widgets[0].text = "Caption only".into();
        assert_eq!(
            source.update(&base, &ir::InterfacePatch::between(&base, &current)),
            (true, false),
        );
        current.widgets[0].name = "$renamed".into();
        assert_eq!(
            source.update(&base, &ir::InterfacePatch::between(&base, &current)),
            (true, true),
        );
        assert_eq!(source.update(&base, &Default::default()), (false, true));
        current = base.clone();
        current.widgets.push(current.widgets[0].clone());
        assert_eq!(
            source.update(&base, &ir::InterfacePatch::between(&base, &current)),
            (true, true),
        );
        assert!(!same_bindings(&current, &base));
        let mut replaced = base.clone();
        replaced.widgets[0].source_id = Some(99);
        assert!(!same_bindings(&base, &replaced));
        replaced = base.clone();
        replaced.widgets[0].value_text = Some("New value".into());
        assert!(same_bindings(&base, &replaced), "equivalent new base preserves Lua drafts");
    }

    #[test]
    fn performance_and_native_intent_choose_sources_without_widget_heuristics() {
        let emit = |source| sampler_ksp::compile(source, 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap().ui(&|_| None).unwrap();
        let auxiliary = emit("on init declare ui_label $a(1,1) declare ui_label $b(1,1) end on");
        let native = emit("on init load_native_ui(\"entry\") end on");
        let performance = emit("on init make_perfview set_ui_color(0123456H) end on");
        assert!(native.widgets.is_empty() && !native.performance);
        assert!(performance.widgets.is_empty() && performance.pages[0].background.color.is_some());
        let mut faces = vec![auxiliary, native, performance];
        assert_eq!(main_face(&faces, &[]), Some(1));
        faces[2].performance = false;
        assert_eq!(main_face(&faces, &[]), Some(1));
        faces[1].native_ui = None;
        let mut larger = faces[0].clone();
        larger.widgets.push(larger.widgets[0].clone());
        faces.push(larger);
        assert_eq!(main_face(&faces, &[]), Some(0), "fallback keeps declaration order too");
        let mut patches = vec![ir::InterfacePatch::default(); faces.len()];
        patches[2].performance = Some(true);
        assert_eq!(main_face(&faces, &patches), Some(2));
        patches[0].performance = Some(true);
        assert_eq!(main_face(&faces, &patches), Some(0), "declaration order breaks ties");
        patches[0].performance = Some(false);
        patches[2].performance = Some(false);
        patches[1].native_ui = Some(Some(ir::NativeUi { entry: "updated".into() }));
        assert_eq!(main_face(&faces, &patches), Some(1));
    }

    #[test]
    fn native_composition_wins_over_an_earlier_classic_performance_source() {
        let classic = projection_fixture(1);
        let mut native = projection_fixture(4);
        native.native_ui = Some(ir::NativeUi { entry: "main".into() });
        assert_eq!(main_face(&[classic, native], &[]), Some(1),
            "the NativeUI request owns the composition even when its controls live in another script");
    }

    #[test]
    fn original_and_vector_keep_the_same_native_composition_and_pointer_identity() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("Resources/native_ui")).unwrap();
        std::fs::write(dir.path().join("Resources/native_ui/main.nui"),
            "local ui=require('native_ui')\nreturn function() return @ui.Rectangle {color=ui.Color(20,40,60)}.frame(width=96,height=48).on_tap_gesture({complete=function() end}) end").unwrap();
        let p = Arc::new(crate::plugin::SamplerParams::new());
        p.selection.write().unwrap().parts = vec![crate::plugin::Part {
            path:dir.path().join("fixture.nki").to_string_lossy().into_owned(), view:1, ..Default::default()
        }];
        let mut raw = projection_fixture(0);
        raw.widgets.clear();
        raw.native_ui = Some(ir::NativeUi {entry:"main".into()});
        p.shared.view.lock().unwrap().parts[0].interfaces = vec![raw].into();
        let mut h = super::super::tests::Harness::new(&p,1180.,780.);
        let native = |h: &super::super::tests::Harness| h.ui.scene().unwrap().surfaces()
            .filter(|s|s.key.as_str().starts_with("nui-") && s.key.as_str().ends_with("-root/component"))
            .map(|s|(s.key.to_string(),s.frame)).collect::<Vec<_>>();
        let deadline = std::time::Instant::now()+std::time::Duration::from_secs(5);
        while native(&h).is_empty() {
            assert!(std::time::Instant::now()<deadline,"NativeUI fixture did not paint");
            h.idle(1); std::thread::sleep(std::time::Duration::from_millis(2));
        }
        h.idle(4);
        let authored = native(&h);
        h.press("face-vector-0");
        h.idle(4);
        assert_eq!(native(&h),authored,"Vector must project the evaluated graph, keeping geometry and live targets");
        assert!(!h.ui.scene().unwrap().surfaces().any(|s|s.key.as_str().contains("-ir-0")),
            "classic placeholder geometry must not accompany the native composition");
    }

    #[test]
    fn publication_preserves_presentation_script_page_and_values() {
        let script = sampler_ksp::compile(
            "on init make_perfview declare ui_knob $k(0,100,1) end on",
            48000,
            sampler_ksp::Limits::LIBRARY,
            &[],
        )
        .unwrap();
        let mut authored = script.ui(&|_| None).unwrap();
        authored.pages.push(authored.pages[0].clone());
        let from: Arc<[ir::Interface]> = vec![authored.clone(), authored.clone()].into();
        let mut face = Face::new(
            std::path::Path::new("/missing/synthetic.nki"),
            7,
            from.clone(),
            1,
            Presentation::Bitmap,
        );
        face.page = ir::PageRef(1);
        face.values.insert(ir::ControlId(9), 31.);
        face.input
            .values
            .insert(ir::WidgetRef(0), ir::Value::Text("Retained draft".into()));
        let mut current = authored.clone();
        current.widgets[0].value_text = Some("Changed".into());
        current.widgets[0].rect.x += 10.0;
        let patch = ir::InterfacePatch::between(&authored, &current);
        face.update(patch.clone());
        assert_eq!(
            (face.shown, face.page, face.presentation),
            (1, ir::PageRef(1), Presentation::Bitmap)
        );
        assert!(Arc::ptr_eq(&face.from, &from));
        assert_eq!(face.values[&ir::ControlId(9)], 31.);
        assert_eq!(
            face.input.values[&ir::WidgetRef(0)],
            ir::Value::Text("Retained draft".into())
        );
        assert_eq!(face.face.widgets[0].value_text.as_deref(), Some("Changed"));
        let first = face.face.clone();
        face.update(patch);
        assert_eq!(face.face, first);
        face.update(Default::default());
        assert_eq!(
            face.face,
            ir_view::resolved(&authored),
            "reverted source properties return to their authored values"
        );
    }

    #[test]
    fn authored_order_publication_reaches_live_and_reopened_faces() {
        let script = sampler_ksp::compile("on init make_perfview declare ui_knob $k(0,100,1) declare ui_label $l(1,1) end on", 48000, sampler_ksp::Limits::LIBRARY, &[]).unwrap();
        let mut authored = script.ui(&|_| None).unwrap();
        authored.pages.push(authored.pages[0].clone());
        let from: Arc<[ir::Interface]> = vec![authored.clone()].into();
        let path = std::path::Path::new("/missing/synthetic.nki");
        let mut face = Face::new(path, 7, from.clone(), 0, Presentation::Vector);
        face.page = ir::PageRef(1);
        face.input.values.insert(ir::WidgetRef(0), ir::Value::Text("Retained draft".into()));
        let mut current = authored.clone();
        current.paint_order = Some(vec![ir::WidgetRef(1), ir::WidgetRef(0)]);
        let patch = ir::InterfacePatch::between(&authored, &current);
        face.update(patch.clone());
        assert_eq!(face.face.paint_order, current.paint_order);
        assert_eq!((face.shown, face.page, face.presentation), (0, ir::PageRef(1), Presentation::Vector));
        assert!(Arc::ptr_eq(&face.from, &from));
        assert_eq!(face.input.values[&ir::WidgetRef(0)], ir::Value::Text("Retained draft".into()));
        let mut reopened = Face::new(path, 7, from, 0, face.presentation);
        reopened.update(patch);
        assert_eq!(reopened.face.paint_order, current.paint_order);
        assert_eq!(reopened.presentation, Presentation::Vector);
        face.update(Default::default());
        assert_eq!(face.face.paint_order, authored.paint_order);
    }

    #[test]
    fn saved_override_wins_and_factory_default_is_original() {
        use crate::library::{Settings, ViewMode};
        let mut settings = Settings::default();
        let mut part = crate::plugin::Part {
            path: "synthetic.nki".into(),
            ..Default::default()
        };
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Original);
        settings.view_mode = ViewMode::Vectorized;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Vectorized);
        settings
            .instrument_views
            .insert(part.path.clone(), ViewMode::Kontra);
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Kontra);
        part.view = 1;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Original);
        let restored =
            serde_json::from_str::<crate::plugin::Part>(&serde_json::to_string(&part).unwrap())
                .unwrap();
        let settings =
            serde_json::from_str::<Settings>(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(resolve_mode(&restored, &settings), ViewMode::Original);
        part.view = 0;
        assert_eq!(resolve_mode(&part, &settings), ViewMode::Kontra);
    }

    #[test]
    fn fit_uses_both_axes_and_explicit_zoom_is_exact() {
        let page = Size::new(600., 800.);
        assert_eq!(scale_to_fit(Size::new(1200., 400.), page, 0.), 0.5);
        assert_eq!(scale_to_fit(Size::new(300., 1600.), page, 0.), 0.5);
        assert_eq!(scale_to_fit(Size::new(1200., 2000.), page, 0.), 2.);
        // Fit is uniform and fractional: the page fills the tighter axis exactly.
        assert_eq!(scale_to_fit(Size::new(900., 1600.), page, 0.), 1.5);
        assert_eq!(scale_to_fit(Size::new(1500., 1000.), page, 0.), 1.25);
        for zoom in [1., 1.5, 2.] {
            assert_eq!(
                scale_to_fit(Size::new(300., 400.), page, zoom),
                f64::from(zoom)
            );
        }
    }
}
