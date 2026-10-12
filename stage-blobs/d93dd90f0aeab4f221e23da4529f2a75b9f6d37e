use std::ops::Range;

use crate::bus::BusLayout;

/// Maximum number of audio buses tracked in either direction for one block.
///
/// This is a public topology limit, not a truncation point. Format adapters
/// must reject or filter layouts beyond it before activation; the process
/// path stays bounded and allocation-free.
pub const MAX_AUDIO_BUSES: usize = 32;

/// Return whether a declared layout can be represented exactly by
/// [`BusRouting`].
///
/// Adapters call this while registering or selecting layouts so an oversized
/// topology is refused before the audio thread. The per-bus channel bound
/// follows the snapshot's compact `u16` storage.
#[must_use]
pub fn bus_layout_fits_routing(layout: &BusLayout) -> bool {
    layout.inputs.len() <= MAX_AUDIO_BUSES
        && layout.outputs.len() <= MAX_AUDIO_BUSES
        && layout
            .inputs
            .iter()
            .chain(&layout.outputs)
            .all(|bus| u16::try_from(bus.channels.channel_count()).is_ok())
}

/// Return whether every declared layout can be represented exactly by
/// [`BusRouting`].
#[must_use]
pub fn bus_layouts_fit_routing(layouts: &[BusLayout]) -> bool {
    layouts.iter().all(bus_layout_fits_routing)
}

/// What the current adapter can truthfully say about one bus this block.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum BusActivation {
    /// The host enabled or connected the bus for this block.
    Active,
    /// The host explicitly disabled, omitted, or disconnected the bus.
    Inactive,
    /// The format exposes no process-time bus activation signal.
    #[default]
    Unknown,
}

impl BusActivation {
    /// Return the activation as a boolean when the format can report it.
    #[must_use]
    pub const fn known(self) -> Option<bool> {
        match self {
            Self::Active => Some(true),
            Self::Inactive => Some(false),
            Self::Unknown => None,
        }
    }
}

/// One declared bus's range in Moose's flattened channel array.
///
/// A dynamic format such as CLAP may omit a disabled bus from the flattened
/// host buffer. That bus remains present as a zero-width range so indices stay
/// aligned; its activation is `Unknown` without separate host route metadata.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BusRoute {
    channel_start: u32,
    channel_count: u32,
    activation: BusActivation,
}

impl BusRoute {
    #[must_use]
    pub const fn channel_start(self) -> usize {
        self.channel_start as usize
    }

    #[must_use]
    pub const fn channel_count(self) -> usize {
        self.channel_count as usize
    }

    #[must_use]
    pub fn channel_range(self) -> Range<usize> {
        self.channel_start()..self.channel_start().saturating_add(self.channel_count())
    }

    #[must_use]
    pub const fn activation(self) -> BusActivation {
        self.activation
    }
}

/// Bounded, allocation-free snapshot of the host's audio-bus routing for one
/// process block.
///
/// Bus order matches [`crate::BusLayout`]. Channel ranges index the existing
/// flattened [`crate::AudioBuffer`] inputs and outputs. Adapters use
/// [`BusActivation::Unknown`] when their format has no truthful process-time
/// activation signal; it is never inferred from sample values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BusRouting {
    input_channels: [u16; MAX_AUDIO_BUSES],
    output_channels: [u16; MAX_AUDIO_BUSES],
    input_activation: u64,
    output_activation: u64,
    input_count: u8,
    output_count: u8,
}

impl Default for BusRouting {
    fn default() -> Self {
        Self::new()
    }
}

impl BusRouting {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            input_channels: [0; MAX_AUDIO_BUSES],
            output_channels: [0; MAX_AUDIO_BUSES],
            input_activation: 0,
            output_activation: 0,
            input_count: 0,
            output_count: 0,
        }
    }

    /// Append an input bus. Returns `false` after the bus bound or when one
    /// bus exceeds 65,535 channels.
    pub fn push_input(&mut self, channels: u32, activation: BusActivation) -> bool {
        let index = usize::from(self.input_count);
        let Ok(channels) = u16::try_from(channels) else {
            return false;
        };
        if index == MAX_AUDIO_BUSES {
            return false;
        }
        self.input_channels[index] = channels;
        set_activation(&mut self.input_activation, index, activation);
        self.input_count += 1;
        true
    }

    /// Append an output bus. Returns `false` after the bus bound or when one
    /// bus exceeds 65,535 channels.
    pub fn push_output(&mut self, channels: u32, activation: BusActivation) -> bool {
        let index = usize::from(self.output_count);
        let Ok(channels) = u16::try_from(channels) else {
            return false;
        };
        if index == MAX_AUDIO_BUSES {
            return false;
        }
        self.output_channels[index] = channels;
        set_activation(&mut self.output_activation, index, activation);
        self.output_count += 1;
        true
    }

    #[must_use]
    pub const fn input_count(&self) -> usize {
        self.input_count as usize
    }

    #[must_use]
    pub const fn output_count(&self) -> usize {
        self.output_count as usize
    }

    pub fn inputs(&self) -> impl Iterator<Item = BusRoute> + '_ {
        (0..self.input_count()).filter_map(|index| self.input(index))
    }

    pub fn outputs(&self) -> impl Iterator<Item = BusRoute> + '_ {
        (0..self.output_count()).filter_map(|index| self.output(index))
    }

    #[must_use]
    pub fn input(&self, index: usize) -> Option<BusRoute> {
        route_at(
            &self.input_channels,
            self.input_count(),
            self.input_activation,
            index,
        )
    }

    #[must_use]
    pub fn output(&self, index: usize) -> Option<BusRoute> {
        route_at(
            &self.output_channels,
            self.output_count(),
            self.output_activation,
            index,
        )
    }

    /// Update one input bus's host activation without changing its range.
    pub fn set_input_activation(&mut self, index: usize, activation: BusActivation) {
        if index < usize::from(self.input_count) {
            set_activation(&mut self.input_activation, index, activation);
        }
    }

    /// Update one output bus's host activation without changing its range.
    pub fn set_output_activation(&mut self, index: usize, activation: BusActivation) {
        if index < usize::from(self.output_count) {
            set_activation(&mut self.output_activation, index, activation);
        }
    }
}

fn route_at(
    channels: &[u16; MAX_AUDIO_BUSES],
    count: usize,
    activations: u64,
    index: usize,
) -> Option<BusRoute> {
    if index >= count {
        return None;
    }
    let start = channels[..index]
        .iter()
        .fold(0_u32, |sum, channels| sum + u32::from(*channels));
    Some(BusRoute {
        channel_start: start,
        channel_count: u32::from(channels[index]),
        activation: match (activations >> (index * 2)) & 0b11 {
            1 => BusActivation::Active,
            2 => BusActivation::Inactive,
            _ => BusActivation::Unknown,
        },
    })
}

fn set_activation(bits: &mut u64, index: usize, activation: BusActivation) {
    let shift = index * 2;
    let value = match activation {
        BusActivation::Unknown => 0,
        BusActivation::Active => 1,
        BusActivation::Inactive => 2,
    };
    *bits = (*bits & !(0b11 << shift)) | (value << shift);
}
