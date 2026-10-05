//! Sample-time segmentation and deterministic resident voice rendering.
use super::{Error, Frame, Runtime, VoiceId};

impl Runtime {
    /// Events at the exclusive block end stay pending until the next render (including
    /// an empty block). Overflow is rejected before any output/state mutation.
    pub fn render(&mut self, output: &mut [Frame]) -> Result<(), Error> {
        let end = self
            .now
            .checked_add(output.len() as u64)
            .ok_or(Error::ClockOverflow)?;
        output.fill([0.0; 2]);
        self.apply_due();
        let mut offset = 0;
        while self.now < end {
            self.apply_due();
            let boundary = self.commands.first().map_or(end, |c| c.at.min(end));
            let len = (boundary - self.now) as usize;
            let segment = &mut output[offset..offset + len];
            self.render_segment(segment);
            for frame in segment {
                if !frame.iter().all(|x| x.is_finite()) {
                    *frame = [0.0; 2];
                    self.nonfinite_frames = self.nonfinite_frames.saturating_add(1);
                }
            }
            self.now = boundary;
            offset += len;
        }
        debug_assert_eq!(self.now, end);
        Ok(())
    }

    fn render_segment(&mut self, output: &mut [Frame]) {
        // Skip empty slots a word at a time. Ascending set bits preserve the
        // original slot-order sum even after holes and generational slot reuse.
        for word in 0..self.voice_activity.len() {
            let mut occupied = self.voice_activity[word];
            while occupied != 0 {
                let begin = occupied.trailing_zeros() as usize;
                let end = begin + (occupied >> begin).trailing_ones() as usize;
                occupied = if end == 64 {
                    0
                } else {
                    occupied & (u64::MAX << end)
                };
                // Dense runs retain the simple contiguous slot loop; sparse
                // pools skip the untouched Voice storage between those runs.
                for i in word * 64 + begin..word * 64 + end {
                    self.render_voice(i, output);
                }
            }
        }
    }

    #[inline]
    fn render_voice(&mut self, i: usize, segment: &mut [Frame]) {
        let Some(v) = &mut self.voices.slots[i].value else {
            return;
        };
        if !v.started {
            return;
        }
        // Retention invariant: live voice -> counted family -> counted note
        // -> expression owner. Each owner retires only after its dependents.
        let f = self.families.get(v.family.0).unwrap();
        let n = self.notes.get(f.note.0).unwrap();
        let expression = self.expressions.get(n.expression.0).unwrap();
        let gains = expression.value.gains();
        v.cursor = v.cursor.with_step(v.base_step * expression.pitch_ratio);
        // Prepared playback bounds and the cursor's contiguous spans stay
        // within immutable PCM; looping never changes asset ownership.
        let pcm = &self.plans.get(n.plan.0).unwrap().prepared.pcm[v.sample].frames;
        v.cursor
            .render(pcm, segment, &mut v.envelope, v.gain, gains, self.kernel);
        if v.cursor.done() || v.envelope.done() {
            self.end_voice(VoiceId(self.voices.id(i)));
        }
    }
}
