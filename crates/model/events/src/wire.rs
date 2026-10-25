//! Receiver wire value types: plain data that crosses the adapter boundary.
//! No engine state, no `Runtime`, no logic beyond the derived traits and the
//! documented `Input::channel_address` projection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Native,
    Midi1,
    Midi2,
    Clap,
    Vst3,
}

/// Input ownership address, never the transposed playback address. Adapters
/// own wildcard matching and protocol-specific interpretation of IDs. An
/// adapter mapping host tuples onto an internal wire retains the physical
/// tuple separately through [`Runtime::set_input_source`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Input {
    pub protocol: Protocol,
    pub port: u16,
    /// UMP group; zero for transports without group addressing.
    pub group: u8,
    pub channel: u8,
    pub key: u8,
    pub external_id: Option<i32>,
}

pub const RECEIVER_PER_NOTE_CENTER: u32 = 0x8000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelAddress {
    pub protocol: Protocol,
    pub port: u16,
    pub group: u8,
    pub channel: u8,
}

impl Input {
    pub fn channel_address(self) -> ChannelAddress {
        ChannelAddress {
            protocol: self.protocol,
            port: self.port,
            group: self.group,
            channel: self.channel,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Midi1,
    Midi2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub kind: u8,
    pub data: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Controllers {
    Registered,
    Assignable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTaskFamily {
    Init,
    Transport,
    Spawned,
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SourceTaskHostContext {
    /// Actual Runtime-local execution F; the rack maps its installation origin
    /// before admission. This is not an alias for the absolute host rack frame.
    pub frame: u64,
    pub tempo: f64,
    pub playing: bool,
}

/// A task has no triggering physical note. Event means its invocation genuinely
/// came from an event callback; it is not a lease or an expression identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceTaskCreator {
    None,
    Event,
    NonMusicalEvent(NonMusicalTaskOrigin),
}

/// Precise nonmusical callback provenance; never a physical Note/Receiver lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NonMusicalTaskOrigin {
    RestoredWidget,
}

/// An explicit logical destination in the same admitted protocol/port/group.
/// Adapter input/layer selectors require a separately compiled route; this mask
/// never changes the immutable physical source tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExternalTarget {
    Captured,
    Channels(u16),
}

/// Diagnostic counts only: actual generational tokens/physical receipts retain
/// their ordinary APIs and identities through cancellation and host ACK.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExternalAbortReport {
    pub callbacks_cancelled: usize,
    pub external_turns_cancelled: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventValueMemory {
    pub values: usize,
    pub posts: usize,
    pub arena_bytes: usize,
    pub expression_capture_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProgramSelection {
    pub program: u8,
    pub bank: [u8; 2],
}

/// Protocol state is real, but it must not be advertised as a factory catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgramPlaybackCapability {
    NoPresetCatalog,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_address_projects_the_physical_channel_tuple() {
        let input = Input {
            protocol: Protocol::Midi2,
            port: 3,
            group: 2,
            channel: 9,
            key: 60,
            external_id: Some(7),
        };
        assert_eq!(
            input.channel_address(),
            ChannelAddress {
                protocol: Protocol::Midi2,
                port: 3,
                group: 2,
                channel: 9,
            }
        );
    }
}
