//! Worker-prepared, integer source latency before the shared rack mixer.

use super::{Block, MAX_BLOCK};
use crate::fx::OUTS;

type Frame = [[f32; 2]; OUTS + 1];

/// One slot's main stereo source and independent Kontakt direct outputs.
/// Prepare capacity off audio; changing the delay never resizes storage.
pub struct SourceDelay {
    history: Vec<Frame>,
    delay: usize,
    position: usize,
    direct: [Block; OUTS],
    direct_fed: u8,
}

impl SourceDelay {
    pub fn new(max_delay_frames: usize) -> Result<Self, std::collections::TryReserveError> {
        let mut history = Vec::new();
        history.try_reserve_exact(max_delay_frames)?;
        history.resize(max_delay_frames, [[0.0; 2]; OUTS + 1]);
        Ok(Self {
            history,
            delay: 0,
            position: 0,
            direct: [[[0.0; MAX_BLOCK]; 2]; OUTS],
            direct_fed: 0,
        })
    }

    pub fn max_delay(&self) -> usize {
        self.history.len()
    }
    pub fn delay(&self) -> usize {
        self.delay
    }
    pub fn memory_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.history.capacity() * std::mem::size_of::<Frame>()
    }

    /// Reject an unprepared length without changing state. A changed length
    /// clears old audio; setting the current length preserves its queued tail.
    pub fn set_delay(&mut self, frames: usize) -> bool {
        if frames > self.max_delay() {
            return false;
        }
        if frames != self.delay {
            self.clear();
            self.delay = frames;
        }
        true
    }

    /// Reset on activation or source replacement, without freeing storage.
    pub fn clear(&mut self) {
        self.history.fill([[0.0; 2]; OUTS + 1]);
        self.position = 0;
        self.direct_fed = 0;
        for route in &mut self.direct {
            route.iter_mut().for_each(|c| c.fill(0.0));
        }
    }

    pub(super) fn process<'a>(
        &mut self,
        left: &mut [f32],
        right: &mut [f32],
        direct: impl Iterator<Item = (usize, &'a [f32], &'a [f32])>,
    ) {
        let n = left.len().min(right.len()).min(MAX_BLOCK);
        self.direct_fed = 0;
        for route in &mut self.direct {
            route.iter_mut().for_each(|c| c[..n].fill(0.0));
        }
        for (out, l, r) in direct {
            let Some([to_l, to_r]) = self.direct.get_mut(out) else {
                continue;
            };
            for (to, from) in to_l[..n].iter_mut().zip(l) {
                *to = *from;
            }
            for (to, from) in to_r[..n].iter_mut().zip(r) {
                *to = *from;
            }
            self.direct_fed |= 1 << out;
        }
        if self.delay == 0 {
            return;
        }
        self.direct_fed = 0;
        let capacity = self.history.len();
        for i in 0..n {
            let read = (self.position + capacity - self.delay) % capacity;
            let mut input = [[0.0; 2]; OUTS + 1];
            input[0] = [left[i], right[i]];
            for (out, route) in self.direct.iter().enumerate() {
                input[out + 1] = [route[0][i], route[1][i]];
            }
            let delayed = self.history[read];
            self.history[self.position] = input;
            self.position = (self.position + 1) % capacity;
            [left[i], right[i]] = delayed[0];
            for (out, route) in self.direct.iter_mut().enumerate() {
                [route[0][i], route[1][i]] = delayed[out + 1];
                if route[0][i] != 0.0 || route[1][i] != 0.0 {
                    self.direct_fed |= 1 << out;
                }
            }
        }
    }

    pub(super) fn direct_outs(&self, n: usize) -> impl Iterator<Item = (usize, &[f32], &[f32])> {
        let n = n.min(MAX_BLOCK);
        self.direct
            .iter()
            .enumerate()
            .filter(move |(out, _)| self.direct_fed & (1 << out) != 0)
            .map(move |(out, [l, r])| (out, &l[..n], &r[..n]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_main_and_all_direct_impulses_survive_fragmentation_and_absent_routes() {
        for piece in [1, 256, MAX_BLOCK] {
            let mut delay = SourceDelay::new(513).unwrap();
            assert!(delay.set_delay(513));
            let mut output = Vec::new();
            let mut offset = 0;
            while offset < 1024 {
                let end = (offset + piece).min(1024);
                while offset < end {
                    let n = (end - offset).min(MAX_BLOCK);
                    let (mut left, mut right) = ([0.0; MAX_BLOCK], [0.0; MAX_BLOCK]);
                    let mut direct = [[[0.0; MAX_BLOCK]; 2]; OUTS];
                    if offset == 0 {
                        left[0] = 0.5;
                        right[0] = -0.25;
                        for (route, data) in direct.iter_mut().enumerate() {
                            data[0][0] = (route + 1) as f32;
                            data[1][0] = -(route as f32 + 0.5);
                        }
                    }
                    delay.process(
                        &mut left[..n],
                        &mut right[..n],
                        direct
                            .iter()
                            .enumerate()
                            .filter(|_| offset == 0)
                            .map(|(route, [l, r])| (route, &l[..n], &r[..n])),
                    );
                    for i in 0..n {
                        let mut frame = [[0.0; 2]; OUTS + 1];
                        frame[0] = [left[i], right[i]];
                        for (route, l, r) in delay.direct_outs(n) {
                            frame[route + 1] = [l[i], r[i]];
                        }
                        output.push(frame);
                    }
                    offset += n;
                }
            }
            for (i, frame) in output.iter().enumerate() {
                if i == 513 {
                    assert_eq!(frame[0], [0.5, -0.25]);
                    for route in 0..OUTS {
                        assert_eq!(
                            frame[route + 1],
                            [(route + 1) as f32, -(route as f32 + 0.5)]
                        );
                    }
                } else {
                    assert_eq!(*frame, [[0.0; 2]; OUTS + 1], "piece={piece}, frame={i}");
                }
            }
        }
    }

    #[test]
    fn delay_changes_are_bounded_atomic_and_clear_old_audio() {
        let mut delay = SourceDelay::new(4).unwrap();
        assert!(delay.set_delay(4));
        delay.process(&mut [0.5], &mut [-0.25], std::iter::empty());
        assert!(!delay.set_delay(5));
        assert_eq!(delay.delay(), 4);
        assert!(delay.set_delay(4));
        let (mut left, mut right) = ([0.0; 4], [0.0; 4]);
        delay.process(&mut left, &mut right, std::iter::empty());
        assert_eq!(left, [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(right, [0.0, 0.0, 0.0, -0.25]);
        delay.clear();
        delay.process(&mut [1.0], &mut [1.0], std::iter::empty());
        assert!(delay.set_delay(0));
        let (mut left, mut right) = ([-0.0, 0.25], [0.5, -0.0]);
        delay.process(
            &mut left,
            &mut right,
            std::iter::once((2, &[0.0, -0.0][..], &[-0.0, 0.0][..])),
        );
        assert_eq!(left.map(f32::to_bits), [-0.0f32, 0.25].map(f32::to_bits));
        assert_eq!(right.map(f32::to_bits), [0.5f32, -0.0].map(f32::to_bits));
        let outs: Vec<_> = delay.direct_outs(2).collect();
        assert_eq!(
            outs.len(),
            1,
            "zero delay retains even an explicitly fed silent direct route"
        );
        assert_eq!(
            outs[0].1.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            [0.0f32, -0.0].map(f32::to_bits)
        );
        assert!(delay.set_delay(4));
        delay.process(&mut left, &mut right, std::iter::empty());
        assert_eq!(left, [0.0; 2]);
        assert_eq!(right, [0.0; 2]);
    }

    #[cfg(feature = "plugin")]
    #[test]
    fn prepared_delay_processing_changes_and_clears_do_not_allocate() {
        let mut delay = SourceDelay::new(1024).unwrap();
        let bytes = delay.memory_bytes();
        assert_eq!(
            crate::test_support::allocations(|| {
                assert!(delay.set_delay(1024));
                for _ in 0..16 {
                    delay.process(
                        &mut [0.25; MAX_BLOCK],
                        &mut [0.5; MAX_BLOCK],
                        std::iter::once((7, &[1.0; MAX_BLOCK][..], &[2.0; MAX_BLOCK][..])),
                    );
                }
                assert!(delay.set_delay(0));
                delay.clear();
                assert!(delay.set_delay(512));
            }),
            0
        );
        assert_eq!(delay.memory_bytes(), bytes);
    }
}
