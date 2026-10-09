//! Dedicated effect panels. UI work and response preparation stay off audio.
use super::theme::*;
use moose::mui::mui::prelude::*;
use sampler_core::{Biquad, ControlId, EngineParameterLaw, FilterKind};

// ponytail: replace these fixture types with sampler_core exports when W15's registry is READY.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterScope { Voice, Group(u32), Bus(u32), Plan }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParameterAddress { pub scope: ParameterScope, pub node: u32, pub parameter: u32 }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterUnit { Linear, Normalized, Percent, Decibels, Hertz, Seconds, Semitones, Octaves, Frames }
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ParameterLaw { Linear, Native(EngineParameterLaw) }
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParameterDisplay { pub group: String, pub order: u32 }
#[derive(Clone, Debug)]
pub struct ParameterDescriptor {
    pub address: ParameterAddress,
    pub control: ControlId,
    pub name: String,
    pub unit: ParameterUnit,
    pub range: [f64; 2],
    pub default: f64,
    pub law: ParameterLaw,
    pub display: ParameterDisplay,
}

/// Roles come from the shared model, never from a format or a display label.
pub struct FilterModel {
    pub rate: u32,
    pub kind: FilterKind,
    pub cutoff: ParameterAddress,
    pub q: ParameterAddress,
}

#[derive(Default)]
pub struct State;

pub fn filter(
    _ui: &mut Ui,
    _state: &mut State,
    _model: &FilterModel,
    _descriptors: &[ParameterDescriptor],
    _read: impl Fn(ControlId) -> Option<f64>,
    _write: impl FnMut(ControlId, f64),
) -> El {
    caption("Select an effect in the rack or Sound effect list.").fill(secondary())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;

    fn descriptors() -> Vec<ParameterDescriptor> {
        let address = |parameter| ParameterAddress { scope: ParameterScope::Group(7), node: 19, parameter };
        // Deliberately reverse registration order and use names that do not identify their roles.
        vec![
            ParameterDescriptor { address: address(2), control: ControlId(902), name: "Width".into(),
                unit: ParameterUnit::Linear, range: [0.1, 12.], default: 0.707,
                law: ParameterLaw::Linear, display: ParameterDisplay { group: "Filter".into(), order: 2 } },
            ParameterDescriptor { address: address(1), control: ControlId(901), name: "Frequency".into(),
                unit: ParameterUnit::Hertz, range: [20., 20000.], default: 1000.,
                law: ParameterLaw::Native(EngineParameterLaw::Exponential { low: 20., high: 20000. }),
                display: ParameterDisplay { group: "Filter".into(), order: 1 } },
        ]
    }

    struct Fixture {
        ui: Ui,
        state: State,
        descriptors: Vec<ParameterDescriptor>,
        values: RefCell<BTreeMap<ControlId, f64>>,
        writes: RefCell<Vec<(ControlId, f64)>>,
        size: Size,
    }
    impl Fixture {
        fn new(width: f64, height: f64) -> Self {
            let descriptors = descriptors();
            let values = RefCell::new(descriptors.iter().map(|d| (d.control, d.default)).collect());
            let mut f = Self { ui: ui(), state: State::default(), descriptors, values, writes: RefCell::default(), size: Size::new(width, height) };
            f.idle(4);
            f
        }
        fn tick(&mut self, input: Input) {
            let model = FilterModel { rate: 48000, kind: FilterKind::LowPass,
                cutoff: self.descriptors[1].address, q: self.descriptors[0].address };
            let panel = filter(&mut self.ui, &mut self.state, &model, &self.descriptors,
                |id| self.values.borrow().get(&id).copied(),
                |id, value| { self.values.borrow_mut().insert(id, value); self.writes.borrow_mut().push((id, value)); });
            let root = col![section_bar("Sound · Effects", vec![]), rule(), panel.flex(1).min_h(0)]
                .gap(SPACE).pad(INSET).fill(Role::Surface).id("effect-fixture");
            self.ui.frame(root, Some(self.size), input, 1. / 60.).unwrap();
        }
        fn idle(&mut self, count: usize) { for _ in 0..count { self.tick(Input::default()); } }
        fn pointer(&mut self, at: Point, down: bool) {
            self.tick(Input { pointer: PointerInput { pos: Some(at), buttons: if down { Buttons::PRIMARY } else { Buttons::default() }, ..Default::default() }, ..Default::default() });
        }
    }

    #[test]
    fn filter_graph_and_controls_share_the_supplied_lanes_at_both_sizes() {
        for (w, h) in [(900., 600.), (1180., 900.)] {
            let mut f = Fixture::new(w, h);
            let scene = f.ui.scene().unwrap();
            let graph = scene.surface("effect-filter-graph").expect("dedicated filter graph").frame;
            let cutoff = scene.surface("effect-control-901").expect("descriptor cutoff").frame;
            let q = scene.surface("effect-control-902").expect("descriptor Q").frame;
            assert!(cutoff.x < q.x, "display.order overrides registration order");
            for r in [graph, cutoff, q] {
                assert!(r.x >= INSET && r.y >= INSET && r.x + r.size.width <= w - INSET + 0.5 && r.y + r.size.height <= h - INSET + 0.5, "{r:?} stays in its panel");
            }
            let at = super::super::tests::center(&f.ui, "effect-filter-handle");
            f.pointer(at, true);
            f.pointer(Point::new(at.x + 80., at.y - 30.), true);
            f.pointer(Point::new(at.x + 80., at.y - 30.), false);
            f.idle(2);
            assert!(f.values.borrow()[&ControlId(901)] > 1000., "rightward handle movement raises frequency");
            assert!(f.values.borrow()[&ControlId(902)] > 0.707, "upward handle movement raises Q");
            let at = super::super::tests::center(&f.ui, "effect-control-901");
            let before = f.values.borrow()[&ControlId(901)];
            f.tick(Input { pointer: PointerInput { pos: Some(at), ..Default::default() }, wheel: Vec2::new(0., -60.), ..Default::default() });
            f.idle(2);
            assert!(f.values.borrow()[&ControlId(901)] > before, "the shared knob driver reaches the same cutoff lane");
            assert!(f.writes.borrow().iter().all(|(id, value)| [ControlId(901), ControlId(902)].contains(id) && value.is_finite()));
        }
    }

    #[test]
    #[cfg(feature = "shots")]
    fn filter_panel_shots() {
        let Some(out) = std::env::var_os("KONTRA_FILTER_SHOTS").map(std::path::PathBuf::from) else { return };
        std::fs::create_dir_all(&out).unwrap();
        for (w, h) in [(900, 600), (1180, 900)] {
            let mut f = Fixture::new(w as f64, h as f64);
            f.idle(20);
            moose::core::screenshot::save_png(&out.join(format!("filter-{w}.png")), &super::super::tests::pixels(&f.ui, w, h), w.into(), h.into());
        }
    }
}
