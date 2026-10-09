//! Optional, bounded asynchronous timestamp collection. Never waits for the GPU.
//! The host must request both timestamp features before creating its device.
use super::Error;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
const SLOTS: usize = 4;
#[derive(Clone, Copy, Debug)]
pub struct GpuTiming {
    pub frame: u64,
    pub milliseconds: f64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Recording(u64),
    Resolved(u64),
    Pending(u64),
}
struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    ready: Arc<AtomicU8>,
    state: State,
}
/// Opaque ticket: the slot cannot be recycled until its map completes.
#[derive(Clone, Copy)]
pub struct TimingTicket {
    slot: usize,
    frame: u64,
}
pub struct GpuTimer {
    slots: Vec<Slot>,
    period: f64,
    frame: u64,
    pub dropped_samples: u64,
    pub failed_samples: u64,
}
impl GpuTimer {
    pub fn required_features() -> wgpu::Features {
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS
    }
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Self, Error> {
        if !device.features().contains(Self::required_features()) {
            return Err(Error::Unsupported(
                "timestamp features were not enabled on the device",
            ));
        }
        let slots = (0..SLOTS)
            .map(|_| Slot {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("MUI frame timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("MUI timestamp resolve"),
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                read: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("MUI timestamp readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                ready: Arc::new(AtomicU8::new(0)),
                state: State::Idle,
            })
            .collect();
        Ok(Self {
            slots,
            period: f64::from(queue.get_timestamp_period()),
            frame: 0,
            dropped_samples: 0,
            failed_samples: 0,
        })
    }
    pub fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) -> Option<TimingTicket> {
        let frame = self.frame;
        self.frame = self.frame.wrapping_add(1);
        let Some(i) = self.slots.iter().position(|s| s.state == State::Idle) else {
            self.dropped_samples += 1;
            return None;
        };
        let s = &mut self.slots[i];
        s.state = State::Recording(frame);
        encoder.write_timestamp(&s.queries, 0);
        Some(TimingTicket { slot: i, frame })
    }
    /// May be in the same encoder or a later encoder on the SAME queue. A
    /// multi-submission bracket includes any intervening queue idle time.
    pub fn finish(&mut self, encoder: &mut wgpu::CommandEncoder, ticket: TimingTicket) {
        let s = &mut self.slots[ticket.slot];
        if s.state != State::Recording(ticket.frame) {
            return;
        }
        encoder.write_timestamp(&s.queries, 1);
        encoder.resolve_query_set(&s.queries, 0..2, &s.resolve, 0);
        encoder.copy_buffer_to_buffer(&s.resolve, 0, &s.read, 0, 16);
        s.state = State::Resolved(ticket.frame);
    }
    /// Call only after the encoder containing `finish` has been submitted.
    pub fn submitted(&mut self, ticket: TimingTicket) {
        let s = &mut self.slots[ticket.slot];
        let State::Resolved(frame) = s.state else {
            return;
        };
        if frame != ticket.frame {
            return;
        }
        s.state = State::Pending(frame);
        s.ready.store(0, Ordering::Release);
        let ready = s.ready.clone();
        s.read
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                ready.store(if result.is_ok() { 1 } else { 2 }, Ordering::Release);
            });
    }
    pub fn abort(&mut self, ticket: TimingTicket) {
        let s = &mut self.slots[ticket.slot];
        if s.state == State::Recording(ticket.frame) || s.state == State::Resolved(ticket.frame) {
            s.state = State::Idle;
        }
    }
    /// Pump completion callbacks without waiting. At most four samples are
    /// appended. Reuse the output Vec and drain it into a bounded history.
    pub fn collect(&mut self, device: &wgpu::Device, out: &mut Vec<GpuTiming>) {
        let _ = device.poll(wgpu::PollType::Poll);
        for s in &mut self.slots {
            let State::Pending(frame) = s.state else {
                continue;
            };
            match s.ready.load(Ordering::Acquire) {
                0 => continue,
                1 => {
                    let Ok(bytes) = s.read.slice(..).get_mapped_range() else {
                        continue;
                    };
                    let start =
                        u64::from_le_bytes(bytes[0..8].try_into().expect("eight timestamp bytes"));
                    let end =
                        u64::from_le_bytes(bytes[8..16].try_into().expect("eight timestamp bytes"));
                    if end >= start {
                        out.push(GpuTiming {
                            frame,
                            milliseconds: (end - start) as f64 * self.period / 1e6,
                        });
                    } else {
                        self.failed_samples += 1;
                    }
                    drop(bytes);
                    s.read.unmap();
                }
                _ => self.failed_samples += 1,
            }
            s.state = State::Idle;
            s.ready.store(0, Ordering::Release);
        }
    }
}
