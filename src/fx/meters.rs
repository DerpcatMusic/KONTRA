//! Optional instrument-output meter taps, shared with an epoch's editor scene.
//! Values are absolute linear sample peaks over the last render window; no
//! undocumented native meter ballistics or dB calibration is approximated.
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering},
};

pub(crate) const METER_OUTPUTS: usize = 17; // sixteen Kontakt buses, then main
const DISCONNECTED: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MeterAttachment {
    pub group: i32,
    pub slot: i32,
    pub channel: u8,
    pub generic: i8,
}
impl MeterAttachment {
    pub(crate) fn output(self) -> Option<u8> {
        if self.group != -1 || self.slot != -1 || self.channel >= 2 {
            return None;
        }
        match self.generic {
            -1 => Some(16),
            0..=15 => Some(self.generic as u8),
            _ => None,
        }
    }
    fn address(self) -> Option<u8> {
        Some(self.output()? * 2 + self.channel)
    }
}

pub(crate) struct OutputMeters {
    readers: [[AtomicU16; 2]; METER_OUTPUTS],
    values: [[AtomicU32; 2]; METER_OUTPUTS],
    revision: AtomicU64,
}
impl Default for OutputMeters {
    fn default() -> Self {
        Self {
            readers: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU16::new(0))),
            values: std::array::from_fn(|_| std::array::from_fn(|_| AtomicU32::new(0))),
            revision: AtomicU64::new(0),
        }
    }
}
impl OutputMeters {
    pub(crate) fn watched(&self, output: usize) -> bool {
        self.readers[output]
            .iter()
            .any(|n| n.load(Ordering::Relaxed) != 0)
    }
    pub(crate) fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    pub(crate) fn subscribe(self: &Arc<Self>, target: MeterAttachment) -> Option<Arc<MeterReader>> {
        let address = target.address()?;
        self.readers[(address / 2) as usize][(address % 2) as usize]
            .fetch_add(1, Ordering::Relaxed);
        Some(Arc::new(MeterReader {
            owner: self.clone(),
            address: AtomicU8::new(address),
        }))
    }
    pub(crate) fn publish(&self, peaks: &[[f32; 2]; METER_OUTPUTS]) {
        let mut changed = false;
        for (output, (readers, values)) in self.readers.iter().zip(&self.values).enumerate() {
            for (channel, (reader, value)) in readers.iter().zip(values).enumerate() {
                if reader.load(Ordering::Relaxed) == 0 {
                    continue;
                }
                let bits = peaks[output][channel].to_bits();
                changed |= value.swap(bits, Ordering::Relaxed) != bits;
            }
        }
        if changed {
            self.revision.fetch_add(1, Ordering::Release);
        }
    }
    pub(crate) fn clear(&self) {
        self.publish(&[[0.; 2]; METER_OUTPUTS]);
    }
}

pub(crate) struct MeterReader {
    owner: Arc<OutputMeters>,
    address: AtomicU8,
}
impl MeterReader {
    pub(crate) fn level(&self) -> Option<f32> {
        let a = self.address.load(Ordering::Acquire);
        (a != DISCONNECTED).then(|| {
            f32::from_bits(
                self.owner.values[(a / 2) as usize][(a % 2) as usize].load(Ordering::Relaxed),
            )
        })
    }
    pub(crate) fn reattach(&self, target: Option<MeterAttachment>) {
        let next = target
            .and_then(MeterAttachment::address)
            .unwrap_or(DISCONNECTED);
        let old = self.address.load(Ordering::Relaxed);
        if next == old {
            return;
        }
        if next != DISCONNECTED {
            self.owner.readers[(next / 2) as usize][(next % 2) as usize]
                .fetch_add(1, Ordering::Relaxed);
        }
        self.address.store(next, Ordering::Release);
        if old != DISCONNECTED {
            self.owner.readers[(old / 2) as usize][(old % 2) as usize]
                .fetch_sub(1, Ordering::Relaxed);
        }
    }
}
impl Drop for MeterReader {
    fn drop(&mut self) {
        self.reattach(None);
    }
}
