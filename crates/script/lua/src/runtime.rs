//! Lock-free runtime swap channel between a running [`crate::LuaMachine`]
//! (on the audio thread) and the host's control thread (`Rack::tick`).
//!
//! All items are fixed-size `Copy` structs using [`kontra_vm::Text`] so
//! enqueueing requests and polling replies on the audio thread never allocates
//! on the Rust heap and never blocks.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use kontra_protocol::Ring;
use kontra_vm::Text;

/// Ring capacity for runtime swap requests and replies: sized well above a
/// full 4-layer Augmented Orchestra swap burst (~7,000 oscillators) so zero
/// requests drop even when all four layers reload simultaneously.
pub const RUNTIME_RING_CAP: usize = 16_384;

/// Maximum inline WAV cue/slice start offsets returned in one [`RuntimeReply`].
pub const MAX_INLINE_SLICES: usize = 32;

/// Oscillator parameter written via `Element:setParameter` at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OscParamKind {
    BaseNote,
    CoarseTune,
    FineTune,
    Pitch,
    Gain,
    Reverse,
    SampleStart,
    LoopStart,
    LoopEnd,
    Bypass,
}

impl OscParamKind {
    /// Decodes a compact integer code passed from `prelude.lua`.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::BaseNote),
            1 => Some(Self::CoarseTune),
            2 => Some(Self::FineTune),
            3 => Some(Self::Pitch),
            4 => Some(Self::Gain),
            5 => Some(Self::Reverse),
            6 => Some(Self::SampleStart),
            7 => Some(Self::LoopStart),
            8 => Some(Self::LoopEnd),
            9 => Some(Self::Bypass),
            _ => None,
        }
    }
}

/// Keygroup parameter written via `Element:setParameter` at runtime.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeygroupParamKind {
    LowKey,
    HighKey,
    LowVelocity,
    HighVelocity,
    Tune,
    CoarseTune,
    FineTune,
    Gain,
    Pan,
}

impl KeygroupParamKind {
    /// Decodes a compact integer code passed from `prelude.lua`.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Self::LowKey),
            1 => Some(Self::HighKey),
            2 => Some(Self::LowVelocity),
            3 => Some(Self::HighVelocity),
            4 => Some(Self::Tune),
            5 => Some(Self::CoarseTune),
            6 => Some(Self::FineTune),
            7 => Some(Self::Gain),
            8 => Some(Self::Pan),
            _ => None,
        }
    }
}

/// Element whose insert effect chain is mutated by `insertEffect`/`removeEffect`/`moveEffect`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectTarget {
    Keygroup(u32),
    Bus(u32),
    Program,
}

impl EffectTarget {
    #[must_use]
    pub const fn from_parts(kind: u8, index: u32) -> Option<Self> {
        match kind {
            0 => Some(Self::Keygroup(index)),
            1 => Some(Self::Bus(index)),
            2 => Some(Self::Program),
            _ => None,
        }
    }
}

/// Dynamic insert chain operation emitted by a Lua script.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum EffectOp {
    Insert { index: u16, effect_type: Text },
    Remove { index: u16 },
    Move { from: u16, to: u16 },
}

/// One off-audio-thread runtime mutation requested by a UVI Lua script.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum RuntimeRequest {
    LoadSample {
        id: u32,
        group: u32,
        slot: u16,
        path: Text,
        root: Option<u8>,
        sample_start: Option<u64>,
        reverse: Option<bool>,
        coarse_tune: Option<f32>,
        fine_tune: Option<f32>,
        gain: Option<f32>,
    },
    LoadMapping {
        id: u32,
        group: u32,
        slot: u16,
        path: Text,
    },
    OscParam {
        group: u32,
        slot: u16,
        param: OscParamKind,
        value: f64,
    },
    KeygroupParam {
        group: u32,
        param: KeygroupParamKind,
        value: f64,
    },
    PlaybackOptions {
        id: u32,
        group: u32,
        slot: u16,
        sample_start: Option<u64>,
        loop_start: Option<u64>,
        loop_end: Option<u64>,
        loop_mode: Option<u8>,
        reverse: Option<bool>,
    },
    EffectChain {
        target: EffectTarget,
        op: EffectOp,
    },
}

/// Completion reply pushed by the control thread after the updated `Plan` or
/// effect mutation has been dispatched to the audio thread.
#[derive(Clone, Copy, Debug)]
pub struct RuntimeReply {
    pub id: u32,
    pub ok: bool,
    pub frames: u64,
    pub sample_rate: u32,
    pub num_zones: u16,
    pub num_slices: u16,
    pub slice_starts: [u32; MAX_INLINE_SLICES],
}

impl RuntimeReply {
    /// Constructs a simple completion reply with no slice markers.
    #[must_use]
    pub const fn simple(id: u32, ok: bool, frames: u64, sample_rate: u32, num_zones: u16) -> Self {
        Self {
            id,
            ok,
            frames,
            sample_rate,
            num_zones,
            num_slices: 0,
            slice_starts: [0; MAX_INLINE_SLICES],
        }
    }
}

/// Bidirectional lock-free channel connecting a [`crate::LuaMachine`] on the
/// audio thread to the host control thread (`Rack`).
pub struct RuntimeChannel {
    connected: AtomicBool,
    next_id: AtomicU32,
    requests: Ring<RuntimeRequest>,
    replies: Ring<RuntimeReply>,
}

impl std::fmt::Debug for RuntimeChannel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeChannel")
            .field("connected", &self.is_connected())
            .finish()
    }
}

impl RuntimeChannel {
    /// Allocates a new disconnected channel (off the audio thread).
    #[must_use]
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            connected: AtomicBool::new(false),
            next_id: AtomicU32::new(1),
            requests: Ring::new(RUNTIME_RING_CAP),
            replies: Ring::new(RUNTIME_RING_CAP),
        })
    }

    /// Marks the control-thread handler live (called after `Engine::new`
    /// finishes `onInit` and the engine is installed in a `Rack`).
    pub fn connect(&self) {
        self.connected.store(true, Ordering::Release);
    }

    /// Marks the control-thread handler disconnected.
    pub fn disconnect(&self) {
        self.connected.store(false, Ordering::Release);
    }

    /// Whether a live host control thread is draining requests on this channel.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Allocates a monotonic non-zero request ID (RT-safe).
    #[must_use]
    pub fn alloc_id(&self) -> u32 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        if id == 0 {
            self.next_id.fetch_add(1, Ordering::Relaxed)
        } else {
            id
        }
    }

    /// Enqueues a request from the audio thread. Returns `false` if full.
    #[must_use]
    pub fn push_request(&self, req: RuntimeRequest) -> bool {
        self.requests.send(req).is_ok()
    }

    /// Drains the oldest pending request on the control thread.
    #[must_use]
    pub fn pop_request(&self) -> Option<RuntimeRequest> {
        self.requests.recv()
    }

    /// Pushes a completion reply from the control thread. Returns `false` if full.
    #[must_use]
    pub fn push_reply(&self, reply: RuntimeReply) -> bool {
        self.replies.send(reply).is_ok()
    }

    /// Polls the oldest completion reply on the audio thread.
    #[must_use]
    pub fn pop_reply(&self) -> Option<RuntimeReply> {
        self.replies.recv()
    }
}
