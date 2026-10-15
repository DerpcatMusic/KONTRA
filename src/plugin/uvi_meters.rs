//! Coherent raw stereo publication, separate from native UI meter values.
use super::PartShared;
use crate::support::MutexExt;
use sampler_uvi::meters::{Availability, Observation};
use std::sync::atomic::{AtomicU64, Ordering};

const LIMIT: usize = 512;

pub(super) struct Cell {
    request: sampler_ui_ir::UviMeterRequest,
    epoch: u64,
    stereo: AtomicU64,
    metadata: AtomicU64,
}

impl Cell {
    fn new(request: sampler_ui_ir::UviMeterRequest, epoch: u64) -> Self {
        Self {
            request,
            epoch,
            stereo: AtomicU64::new(0),
            metadata: AtomicU64::new(0),
        }
    }
    fn publish(&self, observation: Observation) {
        let observation = if observation.availability == Availability::RawStereoObserved
            && (observation
                .physical_bus
                .is_none_or(|bus| u32::try_from(bus).is_err())
                || observation
                    .stereo_peak
                    .is_none_or(|peak| peak.iter().any(|value| !value.is_finite() || *value < 0.)))
        {
            Observation::unavailable(Availability::NotObserved)
        } else {
            observation
        };
        let metadata = observation
            .physical_bus
            .and_then(|bus| u32::try_from(bus).ok())
            .map_or(0, |bus| (u64::from(bus) + 1) << 8)
            | u64::from(observation.availability as u8);
        if let Some([left, right]) = observation.stereo_peak {
            // Both channels form one atomic observation; never tear L/R from
            // different callback publications into a fabricated mono input.
            self.stereo.store(
                u64::from(left.to_bits()) | (u64::from(right.to_bits()) << 32),
                Ordering::Release,
            );
        }
        self.metadata.store(metadata, Ordering::Release);
    }
    fn read(&self) -> Observation {
        let metadata = self.metadata.load(Ordering::Acquire);
        let stereo = self.stereo.load(Ordering::Acquire);
        if self.metadata.load(Ordering::Acquire) != metadata {
            return Observation::unavailable(Availability::NotObserved);
        }
        let availability = Availability::from_code(metadata as u8);
        Observation {
            availability,
            physical_bus: (metadata >> 8)
                .checked_sub(1)
                .and_then(|bus| usize::try_from(bus).ok()),
            stereo_peak: (availability == Availability::RawStereoObserved).then(|| {
                [
                    f32::from_bits(stereo as u32),
                    f32::from_bits((stereo >> 32) as u32),
                ]
            }),
        }
    }
}

impl PartShared {
    pub(crate) fn uvi_widget_meters(
        &self,
        face: &sampler_ui_ir::Interface,
        epoch: u64,
    ) -> std::collections::HashMap<sampler_ui_ir::WidgetRef, Observation> {
        let mut cells = self.uvi_meters.lock_unpoisoned();
        if self.generation.load(Ordering::Acquire) != epoch {
            return Default::default();
        }
        // Scan the large authored widget graph once, then work only on meter
        // requests. Do not rescan every hidden widget for each retained cell.
        let requests: Vec<_> = face
            .widgets
            .iter()
            .enumerate()
            .filter_map(|(index, widget)| {
                widget
                    .uvi_meter_request
                    .map(|request| (sampler_ui_ir::WidgetRef(index), request))
            })
            .collect();
        cells.retain(|cell| {
            cell.epoch == epoch && requests.iter().any(|(_, request)| *request == cell.request)
        });
        let observations = requests
            .into_iter()
            .map(|(index, request)| {
                let position = cells
                    .iter()
                    .position(|cell| cell.request == request)
                    .or_else(|| {
                        if cells.len() >= LIMIT {
                            return None;
                        }
                        cells.push(Cell::new(request, epoch));
                        Some(cells.len() - 1)
                    });
                (
                    index,
                    position.map_or_else(
                        || Observation::unavailable(Availability::InterestLimit),
                        |position| cells[position].read(),
                    ),
                )
            })
            .collect();
        self.uvi_meter_interest
            .store(cells.len(), Ordering::Release);
        observations
    }

    /// Audio thread, after render and before existing physical output drains.
    /// It never allocates, waits for a UI mutex or applies display ballistics.
    pub(crate) fn refresh_uvi_meters(
        &self,
        epoch: u64,
        read: impl Fn(sampler_ui_ir::UviMeterRequest) -> Observation,
    ) {
        if self.uvi_meter_interest.load(Ordering::Acquire) == 0
            || self.generation.load(Ordering::Acquire) != epoch
        {
            return;
        }
        if let Ok(cells) = self.uvi_meters.try_lock_unpoisoned() {
            // A loader may change generation between the fast check above
            // and this lock. Never publish the old core into new UI cells.
            if self.generation.load(Ordering::Acquire) != epoch {
                return;
            }
            for cell in cells.iter() {
                if cell.epoch == epoch {
                    cell.publish(read(cell.request));
                }
            }
        }
        // No scalar/native UI value or repaint revision is fabricated here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publication_preserves_stereo_and_unavailable_does_not_reuse_old_activity() {
        let request = sampler_ui_ir::UviMeterRequest {
            element_id: 1,
            element_node: 2,
            scope: sampler_ui_ir::UviMeterScope::Program,
            bus_id: 0,
            use_output: true,
            display_as_mono: true,
        };
        let cell = Cell::new(request, 0);
        assert_eq!(cell.read().availability, Availability::NotObserved);
        cell.publish(Observation {
            availability: Availability::RawStereoObserved,
            physical_bus: Some(3),
            stereo_peak: Some([0.25, 0.75]),
        });
        assert_eq!(
            cell.read(),
            Observation {
                availability: Availability::RawStereoObserved,
                physical_bus: Some(3),
                stereo_peak: Some([0.25, 0.75])
            }
        );
        cell.publish(Observation::unavailable(Availability::StalePlan));
        assert_eq!(cell.read().stereo_peak, None);
        assert_eq!(cell.read().physical_bus, None);
        cell.publish(Observation {
            availability: Availability::RawStereoObserved,
            physical_bus: Some(3),
            stereo_peak: None,
        });
        assert_eq!(cell.read().availability, Availability::NotObserved);
        cell.publish(Observation {
            availability: Availability::RawStereoObserved,
            physical_bus: Some(3),
            stereo_peak: Some([f32::NAN, 1.]),
        });
        assert_eq!(cell.read().stereo_peak, None);
    }

    #[test]
    fn interest_is_bounded_and_audio_skips_absent_consumers_without_heap_work() {
        let shared = PartShared::default();
        assert_eq!(
            crate::plugin::tests::allocations(
                || shared.refresh_uvi_meters(0, |_| panic!("no registered interest"))
            ),
            0
        );
        let mut face = sampler_ui_ir::Interface::default();
        for index in 0..=LIMIT {
            let mut widget = sampler_ui_ir::Widget::new(
                "fixture",
                sampler_ui_ir::PageRef(0),
                sampler_ui_ir::Rect::new(0, 0, 10, 10),
                sampler_ui_ir::Kind::LevelMeter {
                    orientation: sampler_ui_ir::Orientation::Vertical,
                },
            );
            widget.uvi_meter_request = Some(sampler_ui_ir::UviMeterRequest {
                element_id: index as u32,
                element_node: index as u32,
                scope: sampler_ui_ir::UviMeterScope::Program,
                bus_id: 0,
                use_output: true,
                display_as_mono: false,
            });
            face.widgets.push(widget);
        }
        let observations = shared.uvi_widget_meters(&face, 0);
        assert_eq!(observations.len(), LIMIT + 1);
        assert_eq!(
            observations
                .values()
                .filter(|o| o.availability == Availability::InterestLimit)
                .count(),
            1
        );
        let reads = std::cell::Cell::new(0);
        assert_eq!(
            crate::plugin::tests::allocations(|| shared.refresh_uvi_meters(0, |_| {
                reads.set(reads.get() + 1);
                Observation::unavailable(Availability::NoPhysicalBus)
            })),
            0
        );
        assert_eq!(reads.get(), LIMIT);
        let first = face.widgets[0].clone();
        face.widgets.clear();
        assert!(shared.uvi_widget_meters(&face, 0).is_empty());
        assert_eq!(
            crate::plugin::tests::allocations(
                || shared.refresh_uvi_meters(0, |_| panic!("pruned interest"))
            ),
            0
        );
        face.widgets.push(first);
        shared.uvi_widget_meters(&face, 0);
        shared.refresh_uvi_meters(0, |_| Observation {
            availability: Availability::RawStereoObserved,
            physical_bus: Some(0),
            stereo_peak: Some([0.25, 0.75]),
        });
        assert!(
            shared.uvi_widget_meters(&face, 0)[&sampler_ui_ir::WidgetRef(0)]
                .stereo_peak
                .is_some()
        );
        shared.generation.store(1, Ordering::Release);
        assert_eq!(
            shared.uvi_widget_meters(&face, 1)[&sampler_ui_ir::WidgetRef(0)].availability,
            Availability::NotObserved,
            "same request tuple in a replacement owner cannot inherit old peaks"
        );
        assert_eq!(
            crate::plugin::tests::allocations(
                || shared.refresh_uvi_meters(0, |_| panic!("old generation"))
            ),
            0
        );
        let guard = shared.uvi_meters.lock_unpoisoned();
        assert_eq!(
            crate::plugin::tests::allocations(
                || shared.refresh_uvi_meters(1, |_| panic!("UI owns registry"))
            ),
            0
        );
        drop(guard);
    }
}
