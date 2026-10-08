use super::{ControlRange, Parameter, Planar, Processor, control::PreparedParameter};
use crate::Error;

/// Number of completed stages on the named side of the voice amplifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum VoiceSendPosition {
    BeforeAmplitude(usize),
    AfterAmplitude(usize),
}
#[derive(Clone, Copy, Debug)]
pub struct VoiceSendTap {
    pub position: VoiceSendPosition,
    pub bus: usize,
    pub gain: Parameter,
    pub bypass: Parameter,
}
impl VoiceSendTap {
    pub(super) fn valid(self, pre: &[Processor], post: &[Processor]) -> bool {
        let (stages, after) = match self.position {
            VoiceSendPosition::BeforeAmplitude(n) => (pre, n),
            VoiceSendPosition::AfterAmplitude(n) => (post, n),
        };
        after <= stages.len() && self.gain.valid() && self.bypass.valid()
            && self.bypass.bounds().iter().all(|v| (0.0..=1.0).contains(v))
            && !matches!(self.gain, Parameter::Expression { .. })
            && !matches!(self.bypass, Parameter::Expression { .. })
            // Never split the body of a parallel mix/rack branch.
            && stages.iter().enumerate().all(|(index, stage)| {
                let count = match stage {
                    Processor::Mix { count, .. } | Processor::Branch { count, .. } => usize::from(*count),
                    _ => 0,
                };
                after <= index || after > index + count
            })
    }
    pub(super) fn compile(self, bindings: &mut Vec<ControlRange>) -> PreparedTap {
        PreparedTap {
            position: self.position,
            bus: self.bus,
            gain: self.gain.compile(bindings),
            bypass: self.bypass.compile(bindings),
        }
    }
}
pub(super) struct PreparedTap {
    pub position: VoiceSendPosition,
    pub bus: usize,
    pub gain: PreparedParameter,
    pub bypass: PreparedParameter,
}
pub(crate) struct TapFeed {
    pub samples: Planar,
}
impl Default for TapFeed {
    fn default() -> Self {
        Self {
            samples: [[0.0; super::BLOCK]; 2],
        }
    }
}

impl super::VoiceChain {
    /// Control-thread construction. A tap observes this voice's signal without
    /// changing its dry chain. Bus targets are checked when runtime state is built.
    pub fn with_taps(mut self, mut taps: Vec<VoiceSendTap>) -> Result<Self, Error> {
        if taps.iter().any(|tap| !tap.valid(&self.pre, &self.post)) {
            return Err(Error::InvalidInput);
        }
        taps.sort_by_key(|tap| tap.position);
        self.taps = taps.into_boxed_slice();
        Ok(self)
    }
}

impl super::PreparedVoiceChain {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn process_section<const TRACE: bool>(
        &self,
        before: bool,
        states: &mut [super::ProcessorState],
        block: &mut Planar,
        len: usize,
        held: usize,
        at: u64,
        context: &mut super::RenderContext<'_>,
    ) -> bool {
        if held == len { return false; }
        if held > 0 {
            for channel in block.iter_mut() { channel.copy_within(held..len, 0); }
        }
        let len = len - held;
        let at = at + held as u64;
        let stages = if before { &self.pre } else { &self.post };
        let mut first = 0;
        let mut fault = false;
        for (tap_index, tap) in self.taps.iter().enumerate() {
            let after = match (before, tap.position) {
                (true, VoiceSendPosition::BeforeAmplitude(n))
                | (false, VoiceSendPosition::AfterAmplitude(n)) => n,
                _ => continue,
            };
            fault |= super::process::<TRACE>(
                &stages[first..after],
                &mut states[first..after],
                block,
                len,
                context.parameters,
                at,
                context.delay,
                &mut context.filters,
                if TRACE { context.trace.as_mut().map(|t| crate::trace::Section { recorder: &mut *t.recorder,
                    graph: t.graph, nodes: if before { &t.nodes.pre[first..after] } else { &t.nodes.post[first..after] }, identity: t.identity }) } else { None },
            );
            first = after;
            let feed = &mut context.feeds[tap.bus].samples;
            let mut tapped = [[0.; super::BLOCK]; 2];
            let mut applied = 0.;
            for i in 0..len {
                let gain = tap.gain.value(context.parameters, at + i as u64, None)
                    * (1.0 - tap.bypass.value(context.parameters, at + i as u64, None));
                if TRACE { applied += gain / len.max(1) as f64; }
                for c in 0..2 {
                    let value = block[c][i] * gain;
                    fault |= !value.is_finite();
                    feed[c][held + i] += value;
                    if TRACE { tapped[c][i] = value; }
                }
            }
            if TRACE { if let Some(t) = context.trace.as_mut() {
                t.record(t.nodes.taps[tap_index], block, &tapped, len, [applied; 2], context.parameters);
            } }
        }
        fault |= super::process::<TRACE>(
                &stages[first..],
                &mut states[first..],
                block,
                len,
                context.parameters,
                at,
                context.delay,
                &mut context.filters,
                if TRACE { context.trace.as_mut().map(|t| crate::trace::Section { recorder: &mut *t.recorder,
                    graph: t.graph, nodes: if before { &t.nodes.pre[first..] } else { &t.nodes.post[first..] }, identity: t.identity }) } else { None },
            );
        if held > 0 {
            for channel in block.iter_mut() {
                channel.copy_within(..len, held);
                channel[..held].fill(0.);
            }
        }
        fault
    }
}
