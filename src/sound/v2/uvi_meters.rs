//! Raw exact-bus observations. No native meter display conversion lives here.
use super::{Part, V2Core};
use sampler_uvi::meters::{Availability, Binding, Observation};
use std::sync::Arc;

impl Part {
    pub(super) fn install_uvi_meter_buses(
        &mut self,
        bindings: Arc<[Binding]>,
        faces: &[sampler_ui_ir::Interface],
    ) {
        self.uvi_meter_buses = bindings;
        self.uvi_meter_plan =
            (!self.uvi_meter_buses.is_empty()).then(|| self.runtime.active_plan());
        self.uvi_meter_inputs = false;
        self.uvi_meter_started_at = self.runtime.now();
        for request in faces
            .iter()
            .flat_map(|face| &face.widgets)
            .filter_map(|widget| widget.uvi_meter_request)
        {
            if !request.use_output
                && let Ok(bus) = sampler_uvi::meters::resolve(&self.uvi_meter_buses, request)
                && self.runtime.enable_bus_input_peak(bus, true).is_ok()
            {
                self.uvi_meter_inputs = true;
            }
        }
    }

    fn uvi_meter_observation(&self, request: sampler_ui_ir::UviMeterRequest) -> Observation {
        let bus = match sampler_uvi::meters::resolve(&self.uvi_meter_buses, request) {
            Ok(bus) => bus,
            Err(reason) => return Observation::unavailable(reason),
        };
        if self.uvi_meter_plan != Some(self.runtime.active_plan()) {
            return Observation::unavailable(Availability::StalePlan);
        }
        if self.runtime.now() == self.uvi_meter_started_at {
            return Observation {
                availability: Availability::NotObserved,
                physical_bus: Some(bus),
                stereo_peak: None,
            };
        }
        let tap = if request.use_output {
            sampler_core::BusPeakTap::Output
        } else {
            sampler_core::BusPeakTap::Input
        };
        match self.runtime.bus_peak(bus, tap) {
            Ok(stereo_peak) => Observation {
                availability: Availability::RawStereoObserved,
                physical_bus: Some(bus),
                stereo_peak: Some(stereo_peak),
            },
            Err(_) if !request.use_output && bus < self.runtime.bus_count() => Observation {
                availability: Availability::InputNotEnabled,
                physical_bus: Some(bus),
                stereo_peak: None,
            },
            Err(_) => Observation::unavailable(Availability::NoPhysicalBus),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sampler_core::*;
    use sampler_ui_ir as ui;

    fn plan() -> Prepared {
        Prepared::new(
            48000,
            vec![Pcm::new(48000, vec![[1., -0.5]; 16].into_boxed_slice()).unwrap()],
            vec![Region {
                sample: 0,
                key_low: 60,
                key_high: 60,
                root_key: None,
                velocity_low: 0.,
                velocity_high: 1.,
                gain: 1.,
                envelope: Envelope::default(),
                playback: Playback::default(),
            }],
            1,
        )
        .unwrap()
        .with_buses(
            vec![Bus {
                processors: vec![Processor::Gain(0.25)],
                sends: vec![BusSend {
                    bus: None,
                    gain: 1.,
                }],
                tail_frames: 0,
            }],
            vec![Some(0)],
        )
        .unwrap()
    }

    #[test]
    fn uvi_raw_bridge_observes_before_drain_and_keeps_missing_and_generation_states_explicit_without_heap()
     {
        let prepared = plan();
        let limits = Limits::for_plan(&prepared, 4, 4);
        let (runtime, mut control) = Runtime::with_plan_updates(prepared, limits, 2, 1).unwrap();
        let request = ui::UviMeterRequest {
            element_id: 400,
            element_node: 20,
            scope: ui::UviMeterScope::Layer { ordinal: 3 },
            bus_id: 0,
            use_output: false,
            display_as_mono: true,
        };
        let mut face = ui::Interface::default();
        face.source = ui::Source::FalconLua;
        let mut meter = ui::Widget::new(
            "authored",
            ui::PageRef(0),
            ui::Rect::new(0, 0, 20, 100),
            ui::Kind::LevelMeter {
                orientation: ui::Orientation::Vertical,
            },
        );
        meter.uvi_meter_request = Some(request);
        face.widgets.push(meter);
        let mut part = Part::new(runtime, super::super::MixTree::instrument("fixture")).unwrap();
        part.install_uvi_meter_buses(
            vec![Binding {
                element_node: 20,
                scope: sampler_uvi::meters::Scope::Layer,
                physical_bus: Some(0),
                unavailable: Availability::NotObserved,
            }]
            .into(),
            &[face.clone()],
        );
        let mut core = V2Core::with_parts(1, 48000.);
        core.parts[0] = Some(Box::new(part));
        assert_eq!(
            core.uvi_meter_observation(0, request).availability,
            Availability::NotObserved
        );
        let atoms = crate::plugin::PartShared::default();
        assert_eq!(
            atoms.uvi_widget_meters(&face, 0)[&ui::WidgetRef(0)].availability,
            Availability::NotObserved
        );
        let mut output = request;
        output.use_output = true;
        let mut absent = request;
        absent.element_node = 21;
        assert_eq!(
            core.uvi_meter_observation(0, absent).availability,
            Availability::OwnershipUnavailable
        );
        let mut audio = [[0.; 2]; 4];
        let input = Input {
            protocol: Protocol::Clap,
            port: 0,
            group: 0,
            channel: 0,
            key: 60,
            external_id: Some(1),
        };
        let heap = crate::plugin::tests::allocations(|| {
            let part = core.parts[0].as_mut().unwrap();
            part.runtime.trigger(input, 60, 1.).unwrap();
            part.runtime.render(&mut audio).unwrap();
            assert_eq!(
                core.uvi_meter_observation(0, request).stereo_peak,
                Some([1., 0.5])
            );
            assert_eq!(
                core.uvi_meter_observation(0, output).stereo_peak,
                Some([0.25, 0.125])
            );
            atoms.refresh_uvi_meters(0, |request| core.uvi_meter_observation(0, request));
            core.finish_uvi_meter_observation(0);
            assert_eq!(
                core.uvi_meter_observation(0, request).stereo_peak,
                Some([0., 0.])
            );
            assert_eq!(
                core.uvi_meter_observation(0, output).stereo_peak,
                Some([0.25, 0.125])
            );
            core.parts[0]
                .as_mut()
                .unwrap()
                .runtime
                .take_bus_peaks(|_, peak| assert_eq!(peak, [0.25, 0.125]));
        });
        assert_eq!(heap, 0);
        assert_eq!(
            atoms.uvi_widget_meters(&face, 0)[&ui::WidgetRef(0)].stereo_peak,
            Some([1., 0.5]),
            "publication survives independent input/output drains; no mono averaging"
        );
        let replacement = control.submit(Box::new(plan())).unwrap();
        assert_eq!(
            core.parts[0]
                .as_mut()
                .unwrap()
                .runtime
                .poll_plan_update()
                .unwrap(),
            Some(replacement)
        );
        assert_eq!(
            core.uvi_meter_observation(0, request).availability,
            Availability::StalePlan
        );
        atoms.refresh_uvi_meters(0, |request| core.uvi_meter_observation(0, request));
        assert_eq!(
            atoms.uvi_widget_meters(&face, 0)[&ui::WidgetRef(0)].stereo_peak,
            None
        );
        assert!(
            atoms.uvi_widget_meters(&face, 1).is_empty(),
            "stale UI generation cannot read the new owner"
        );
    }
}

impl V2Core {
    pub(crate) fn uvi_meter_observation(
        &self,
        slot: usize,
        request: sampler_ui_ir::UviMeterRequest,
    ) -> Observation {
        self.parts.get(slot).and_then(Option::as_deref).map_or_else(
            || Observation::unavailable(Availability::OwnershipUnavailable),
            |part| part.uvi_meter_observation(request),
        )
    }

    /// Called only after all UVI readbacks and before the mixer output drain.
    pub(crate) fn finish_uvi_meter_observation(&mut self, slot: usize) {
        if let Some(part) = self.parts.get_mut(slot).and_then(Option::as_deref_mut)
            && part.uvi_meter_inputs
            && part.uvi_meter_plan == Some(part.runtime.active_plan())
        {
            part.runtime.take_bus_input_peaks(|_, _| {});
        }
    }
}
