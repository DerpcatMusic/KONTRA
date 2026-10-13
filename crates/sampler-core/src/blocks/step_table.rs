//! 32-step table stepper (BArp32Step slot 6, 0x140a768e0): emits the raw table
//! value at the integer part of a binary64 phase and advances it by a step.
//! Free-run wraps by the table length; one-shot holds the last entry. The
//! step comes from [`crate::blocks::lfo::native_phase_step`]. Not modelled
//! because they are unproved: the end-of-call phase correction (the unit of
//! object field +0x5c), the per-lookup sequence mutation (byte +0xdd), and how
//! stored arp fields become table, length and mode.

pub const STEPS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StepTable {
    pub table: [f32; STEPS],
    /// Active length, 1..=32.
    pub len: usize,
    pub one_shot: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct StepState {
    pub phase: f64,
    pub step: f64,
}

impl StepTable {
    pub fn valid(&self) -> bool {
        (1..=STEPS).contains(&self.len)
    }

    /// Fill `out` with one value per output, then advance.
    pub fn render(&self, s: &mut StepState, out: &mut [f32]) {
        let len = self.len as f64;
        for o in out {
            *o = self.table[(s.phase as usize).min(self.len - 1)];
            s.phase += s.step;
            if s.phase >= len {
                if self.one_shot {
                    s.step = 0.0;
                    s.phase = len - 1.0;
                } else {
                    s.phase -= len;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(one_shot: bool) -> StepTable {
        let mut table = [0.0; STEPS];
        for (i, t) in table.iter_mut().enumerate() {
            *t = i as f32 * 0.5;
        }
        StepTable { table, len: 4, one_shot }
    }

    #[test]
    fn free_run_wraps_and_one_shot_holds_the_last_entry() {
        let mut out = [0.0; 10];
        let mut s = StepState { phase: 0.0, step: 1.0 };
        table(false).render(&mut s, &mut out);
        assert_eq!(out, [0., 0.5, 1., 1.5, 0., 0.5, 1., 1.5, 0., 0.5]);
        let mut s = StepState { phase: 0.0, step: 1.0 };
        table(true).render(&mut s, &mut out);
        assert_eq!(out, [0., 0.5, 1., 1.5, 1.5, 1.5, 1.5, 1.5, 1.5, 1.5]);
        assert_eq!(s.step, 0.0);
    }

    #[test]
    fn fractional_steps_repeat_entries_and_split_calls_agree() {
        let t = table(false);
        let mut whole = [0.0; 12];
        t.render(&mut StepState { phase: 0.0, step: 0.25 }, &mut whole);
        assert_eq!(&whole[..8], &[0., 0., 0., 0., 0.5, 0.5, 0.5, 0.5]);
        let mut s = StepState { phase: 0.0, step: 0.25 };
        let (mut a, mut b) = ([0.0; 5], [0.0; 7]);
        t.render(&mut s, &mut a);
        t.render(&mut s, &mut b);
        assert_eq!([a.as_slice(), b.as_slice()].concat(), whole);
        assert!(!StepTable { len: 0, ..t }.valid() && !StepTable { len: 33, ..t }.valid());
    }
}
