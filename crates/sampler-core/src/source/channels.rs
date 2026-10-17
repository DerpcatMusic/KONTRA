//! Width-preserving resident source reads under one cursor phase/commit.
//! Storage pairs are read views, never additional source clocks or envelopes.
use super::{Cursor, storage::SourceResidentView};
use crate::{ChannelCount, Error, resample::Kernel};

/// Checked borrowed channel-major scratch. Construct before any cursor/output mutation.
pub(crate) struct ChannelBlock<'a> {
    pub(crate) values: &'a mut [f32],
    pub(crate) channels: ChannelCount,
    pub(crate) frames: usize,
    pub(crate) stride: usize,
}
impl<'a> ChannelBlock<'a> {
    pub(crate) fn new(
        values: &'a mut [f32],
        channels: ChannelCount,
        frames: usize,
        stride: usize,
    ) -> Result<Self, Error> {
        let needed = (channels.get() - 1)
            .checked_mul(stride)
            .and_then(|n| n.checked_add(frames))
            .ok_or(Error::Capacity)?;
        if stride < frames || needed > values.len() {
            return Err(Error::InvalidInput);
        }
        Ok(Self {
            values,
            channels,
            frames,
            stride,
        })
    }
}
impl Cursor {
    /// Render raw original-channel source frames. All channels sample the same
    /// actual cursor before one advance; the caller owns the source amplitude
    /// stage and advances that amplitude once per produced frame, not per pair.
    /// Source/output shape, row scratch and last history are preflighted in full.
    pub(crate) fn render_channels_raw(
        &mut self,
        source: &SourceResidentView<'_>,
        output: &mut ChannelBlock<'_>,
        row: &mut [f32],
        last: &mut [f32],
        kernel: &Kernel,
    ) -> Result<usize, Error> {
        let channels = source.channels().get();
        if output.channels != source.channels()
            || row.len() != channels
            || last.len() != channels
            || self.starved()
            || self.holding_onset()
        {
            return Err(Error::InvalidInput);
        }
        let mut rendered = 0;
        while rendered < output.frames && !self.done() {
            // The phase and virtual loop/crossfade address belong to this one
            // Cursor. Existing pair kernel arithmetic reads its immutable view;
            // no copy of Cursor is advanced or retained per physical pair.
            for (pair, destination) in row.chunks_mut(2).enumerate() {
                let frames = source.pair(pair).ok_or(Error::NotReady)?;
                let sample = self.sample(frames, kernel).ok_or(Error::NotReady)?;
                destination.copy_from_slice(&sample[..destination.len()]);
            }
            if self.fade_in > 0 {
                self.fade_in -= 1;
                let ramp = 1. - self.fade_in as f32 / self.fade_frames as f32;
                for value in row.iter_mut() {
                    *value *= ramp;
                }
            }
            for (channel, value) in row.iter().copied().enumerate() {
                output.values[channel * output.stride + rendered] = value;
            }
            last.copy_from_slice(row);
            self.last[0] = row[0];
            self.last[1] = if channels > 1 { row[1] } else { 0. };
            self.advance();
            rendered += 1;
        }
        Ok(rendered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Envelope, EnvelopeState, Loop, LoopMode, LoopShape, Pcm, Playback, SourceStorageFamily,
    };
    fn sources() -> Vec<Pcm> {
        (0..3)
            .map(|pair| {
                Pcm::new(
                    48000,
                    (0..64)
                        .map(|i| {
                            let c = pair * 2;
                            [
                                ((i * (c + 1)) as f32 / 13.).sin() * 0.2,
                                ((i * (c + 2)) as f32 / 11.).cos() * 0.2,
                            ]
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                )
                .unwrap()
            })
            .collect()
    }
    #[test]
    fn one_channel_cursor_matches_existing_pair_kernels_through_fractional_wraps() {
        let packs = sources();
        let original = packs.clone();
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), packs).unwrap();
        let view = family.resident_view();
        let playback = Playback {
            transpose_semitones: 7.,
            loop_range: Some(Loop {
                start: 8,
                end: 40,
                mode: LoopMode::Continuous,
                shape: LoopShape::Wrap,
                passes: None,
            }),
            ..Default::default()
        };
        let kernel = Kernel::new(crate::ResampleQuality::Realtime);
        let mut cursor = playback.cursor(64, 48000, 48000).unwrap();
        let mut data = [0.; 6 * 128];
        let mut row = [0.; 6];
        let mut last = [0.; 6];
        let mut block =
            ChannelBlock::new(&mut data, ChannelCount::new(6).unwrap(), 128, 128).unwrap();
        assert_eq!(
            cursor.render_channels_raw(&view, &mut block, &mut row, &mut last, &kernel),
            Ok(128)
        );
        for (pair, pcm) in original.iter().enumerate() {
            let frames = pcm.resident_frames().unwrap();
            let mut reference = playback.cursor(64, 48000, 48000).unwrap();
            let mut out = [[0.; 2]; 128];
            let mut unity = EnvelopeState::new(Envelope::default());
            assert_eq!(
                reference.render(&frames[..], &mut out, &mut unity, 1., [1.; 2], &kernel),
                128
            );
            assert_eq!(cursor.trace_position(), reference.trace_position());
            for (frame, expected) in out.iter().enumerate() {
                for channel in 0..2 {
                    assert_eq!(
                        block.values[(pair * 2 + channel) * 128 + frame].to_bits(),
                        expected[channel].to_bits()
                    );
                }
            }
        }
    }
    #[test]
    fn channel_cursor_shape_refusal_precedes_clock_history_and_output_changes() {
        let family =
            SourceStorageFamily::new_resident(ChannelCount::new(6).unwrap(), sources()).unwrap();
        let mut cursor = Playback::default().cursor(64, 48000, 48000).unwrap();
        let before = cursor.trace_position();
        let kernel = Kernel::new(crate::ResampleQuality::Realtime);
        let mut data = [99.; 4 * 16];
        let mut row = [88.; 6];
        let mut last = [77.; 6];
        let mut block =
            ChannelBlock::new(&mut data, ChannelCount::new(4).unwrap(), 16, 16).unwrap();
        assert_eq!(
            cursor.render_channels_raw(
                &family.resident_view(),
                &mut block,
                &mut row,
                &mut last,
                &kernel
            ),
            Err(Error::InvalidInput)
        );
        assert_eq!(cursor.trace_position(), before);
        assert_eq!(block.values, &[99.; 64]);
        assert_eq!(row, [88.; 6]);
        assert_eq!(last, [77.; 6]);
    }
}
