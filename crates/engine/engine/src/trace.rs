//! Bounded opt-in events for offline diagnostics; compiled out of normal builds.

pub use kontra_trace::{RouteUnit, RouteValue, TraceEvent, VoiceEvent};

pub(crate) use kontra_trace::Trace;

/// Records `event` on voice slot `id`.
pub(crate) fn voice(
    trace: &mut Trace,
    frame: u64,
    id: usize,
    slot: &crate::Slot,
    event: VoiceEvent,
) {
    trace.push(TraceEvent::Voice {
        frame,
        slot: id,
        zone: slot.zone,
        event,
        position: slot.voice.position(),
        step: slot.voice.step(),
        gain: slot.voice.current_gain(),
    });
}

impl crate::Engine {
    pub(crate) fn trace_route(
        &self,
        zone: &crate::Zone,
        route: &kontra_ir::Mod,
        source: Option<f32>,
        value: impl Into<f64>,
        unit: RouteUnit,
    ) {
        let Some(trace) = &self.trace else { return };
        let mut controllers = trace.routes.controllers();
        let value = value.into();
        while controllers != 0 {
            let cc = controllers.trailing_zeros() as u8;
            controllers &= controllers - 1;
            let source_kind = kontra_plan::route_source(route.kind);
            let group = self.groups.get(zone.group as usize);
            let direct = group.is_some_and(|g| source_kind.is_some_and(|s| g.reads(s, cc)));
            let related =
                group.is_some_and(|g| source_kind.is_some_and(|s| g.reads_modulated(s, cc)));
            let callback = (self.running.kind == crate::Kind::Controller)
                .then_some((self.running.slot, self.running.callback_id));
            if trace.routes.is_onset() && !related {
                continue;
            }
            if related
                || callback.is_some()
                || matches!(source_kind, Some(kontra_ir::Source::Script(_)))
            {
                trace.routes.push(kontra_trace::RouteSample {
                    frame: self.now.0,
                    controller: cc,
                    mod_slot: route.slot,
                    target: route.target,
                    kind: route.kind,
                    source,
                    value,
                    unit,
                    direct_controller_source: direct,
                    controller_can_affect: related,
                    script_callback: callback,
                });
            }
        }
    }
}
