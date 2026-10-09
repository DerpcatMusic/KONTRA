//! Format-neutral extension instructions shared by source frontends: IEEE-754
//! reals in integer registers, bounded text, subroutines, keyed script state,
//! dense control tables, host values and a bounded effect outbox for services
//! the engine does not own (presentation, logging, instrument edits).
use super::{BehaviorId, BehaviorOwner, Comparison, ControlId, Error, Runtime, ScriptArray};
use crate::behavior::Continuation;

/// Nested subroutine frames per continuation.
pub const CALL_DEPTH: usize = 32;
/// Bytes per text cell; longer text is truncated at a character boundary.
pub const TEXT_CAPACITY: usize = 320;
pub const EFFECT_ARGS: usize = 6;
/// Pending effects retained between drains; later effects are counted and dropped.
pub const EFFECT_CAPACITY: usize = 256;
pub const HOST_VALUES: usize = 32;
/// A store key occupies this many consecutive registers.
pub const STORE_KEY: usize = 4;

/// Reals travel in integer registers as IEEE-754 bit patterns.
pub fn real(bits: i64) -> f64 {
    f64::from_bits(bits as u64)
}
pub fn real_bits(value: f64) -> i64 {
    value.to_bits() as i64
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealBinary {
    Add,
    Subtract,
    Multiply,
    Divide,
    Power,
    Min,
    Max,
}
impl RealBinary {
    pub fn apply(self, left: f64, right: f64) -> f64 {
        match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide => left / right,
            Self::Power => left.powf(right),
            Self::Min => left.min(right),
            Self::Max => left.max(right),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RealUnary {
    Negate,
    Absolute,
    Sqrt,
    Cbrt,
    Exp,
    Exp2,
    Ln,
    Log2,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Round,
    Floor,
    Ceil,
}
impl RealUnary {
    pub fn apply(self, value: f64) -> f64 {
        match self {
            Self::Negate => -value,
            Self::Absolute => value.abs(),
            Self::Sqrt => value.sqrt(),
            Self::Cbrt => value.cbrt(),
            Self::Exp => value.exp(),
            Self::Exp2 => value.exp2(),
            Self::Ln => value.ln(),
            Self::Log2 => value.log2(),
            Self::Log10 => value.log10(),
            Self::Sin => value.sin(),
            Self::Cos => value.cos(),
            Self::Tan => value.tan(),
            Self::Asin => value.asin(),
            Self::Acos => value.acos(),
            Self::Atan => value.atan(),
            Self::Round => value.round(),
            Self::Floor => value.floor(),
            Self::Ceil => value.ceil(),
        }
    }
}

/// Signed-32 operations beyond `IntegerBinary`. Shift counts use the low five bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntegerExtra {
    ShiftLeft,
    /// Arithmetic (sign-propagating) right shift.
    ShiftRight,
    Min,
    Max,
}
impl IntegerExtra {
    pub fn apply(self, left: i32, right: i32) -> i32 {
        match self {
            Self::ShiftLeft => left.wrapping_shl(right as u32),
            Self::ShiftRight => left.wrapping_shr(right as u32),
            Self::Min => left.min(right),
            Self::Max => left.max(right),
        }
    }
}

/// Truncate toward zero, saturating to the signed-32 range; NaN becomes zero.
pub fn real_to_i32(value: f64) -> i32 {
    value as i32
}

/// Stable opaque module/target index for a name (FNV-1a, 24 bits): what
/// `find_mod`, `find_target` and `get_mod_idx` return.
pub fn name_index(name: &str) -> i32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in name.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(0x0100_0193);
    }
    0x0400_0000 | (h & 0x00ff_ffff) as i32
}

/// Text cell in the program's script instance: a fixed cell or an array element
/// selected by a register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRef {
    Cell(u32),
    Element { array: ScriptArray, index: u16 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextPart {
    /// Index into the program's text constants.
    Constant(u16),
    Text(TextRef),
    /// Decimal signed-32 register.
    Integer(u16),
    /// Real register.
    Real(u16),
    /// The text constant `base + register`, from a table of `count` consecutive
    /// constants (group names); out of range appends nothing.
    Table {
        base: u16,
        count: u16,
        index: u16,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Midi {
        action: crate::MidiAction,
        args: u16,
        local: u16,
        text: Option<TextRef>,
    },
    MidiFilename {
        text: TextRef,
    },
    WaitMidi {
        local: u16,
    },
    /// lhs := lhs op rhs.
    Real {
        lhs: u16,
        rhs: u16,
        operation: RealBinary,
    },
    RealUnary {
        local: u16,
        operation: RealUnary,
    },
    /// lhs := 0 or 1. Any comparison with NaN is false except NotEqual.
    CompareReal {
        lhs: u16,
        rhs: u16,
        comparison: Comparison,
    },
    IntegerToReal {
        local: u16,
    },
    RealToInteger {
        local: u16,
    },
    Integer {
        lhs: u16,
        rhs: u16,
        operation: IntegerExtra,
    },
    /// lhs := uniform integer in the inclusive range of lhs and rhs, either order.
    Random {
        lhs: u16,
        rhs: u16,
    },
    /// Push the return position and jump within the same program.
    Call {
        target: u32,
    },
    /// Pop a frame; returning from the outermost frame finishes the callback.
    Return,
    TextClear {
        text: TextRef,
    },
    TextAppend {
        text: TextRef,
        part: TextPart,
    },
    /// local := 0 or 1, comparing bytes lexicographically.
    CompareText {
        lhs: TextRef,
        rhs: TextRef,
        local: u16,
        comparison: Comparison,
    },
    /// local := the index of the text among the `count` consecutive text
    /// constants from `base` (ASCII case-insensitive), or -1.
    TextFind {
        text: TextRef,
        base: u16,
        count: u16,
        local: u16,
    },
    /// local := [`name_index`] of the text (find_mod, find_target).
    TextIndex {
        text: TextRef,
        local: u16,
    },
    /// Keyed integer state of the script instance; the key is `STORE_KEY`
    /// registers starting at `key`, each a signed-32 value. A read of a missing
    /// key leaves `local` unchanged; a write beyond capacity is counted and dropped.
    Store {
        key: u16,
        local: u16,
        write: bool,
    },
    /// [`Op::Store`] on the plan's store shared by every script instance
    /// (KSP `pgs_set_key_val`); see `Prepared::with_shared_store`.
    SharedStore {
        key: u16,
        local: u16,
        write: bool,
    },
    /// Integer control selected by a dense index into the instance's control
    /// table. Missing entries are no-ops; writes clamp to the control's domain.
    ControlAt {
        index: u16,
        local: u16,
        write: bool,
    },
    /// Value supplied by the host through `Runtime::set_host_value`.
    FileName {
        ui: u16,
        format: u16,
        text: TextRef,
    },
    /// The UI identity of the shared program entry.
    ReadCallbackUiId {
        local: u16,
    },
    ReadWidgetDropCount {
        ui: i32,
        kind: u8,
        local: u16,
    },
    ReadWidgetDropText {
        ui: i32,
        kind: u8,
        index: u16,
        text: TextRef,
    },
    ReadWidgetEventParameter {
        local: u16,
    },
    ReadWidgetInteraction {
        local: u16,
        field: u8,
    },
    ReadHost {
        local: u16,
        slot: u8,
    },
    /// Engine time since construction in units of `micros` microseconds,
    /// wrapped to signed 32 bits.
    ReadClock {
        local: u16,
        micros: u32,
    },
    ReadTimer {
        local: u16,
    },
    ResetTimer,
    /// Address registers are parameter, physical group, slot and generic.
    TextProperty {
        key: u16,
        text: TextRef,
        write: bool,
    },
    TimeConversion {
        local: u16,
        ticks_to_micros: bool,
    },
    EngineParameter {
        address: u16,
        local: u16,
        write: bool,
    },
    ZoneParameter {
        zone: u16,
        parameter: u16,
        selectors: [Option<i32>; 3],
        local: u16,
    },
    EngineDisplay {
        address: u16,
        value: Option<u16>,
        text: TextRef,
    },
    EngineLookup {
        group: u16,
        owner: u16,
        target: bool,
        text: TextRef,
        local: u16,
    },
    Purge {
        group: u16,
        local: u16,
        write: bool,
    },
    /// Queue a frontend-defined service request with `count` registers from `args`.
    Emit {
        service: u16,
        args: u16,
        count: u8,
        text: Option<TextRef>,
    },
}

impl Op {
    /// Register count the operation needs.
    pub(crate) fn locals(&self) -> usize {
        let reg = |t: &TextRef| match *t {
            TextRef::Cell(_) => 0,
            TextRef::Element { index, .. } => usize::from(index) + 1,
        };
        match self {
            Self::Midi {
                action,
                args,
                local,
                text,
            } => (usize::from(*args) + action.arguments())
                .max(usize::from(*local) + 1)
                .max(text.as_ref().map_or(0, reg)),
            Self::MidiFilename { text } => reg(text),
            Self::Real { lhs, rhs, .. }
            | Self::CompareReal { lhs, rhs, .. }
            | Self::Integer { lhs, rhs, .. }
            | Self::Random { lhs, rhs } => usize::from(*lhs.max(rhs)) + 1,
            Self::RealUnary { local, .. }
            | Self::WaitMidi { local }
            | Self::IntegerToReal { local }
            | Self::RealToInteger { local }
            | Self::ReadWidgetDropCount { local, .. }
            | Self::ReadCallbackUiId { local }
            | Self::ReadWidgetEventParameter { local }
            | Self::ReadWidgetInteraction { local, .. }
            | Self::ReadHost { local, .. }
            | Self::ReadClock { local, .. }
            | Self::ReadTimer { local } => usize::from(*local) + 1,
            Self::ReadWidgetDropText { index, text, .. } => {
                (usize::from(*index) + 1).max(reg(text))
            }
            Self::FileName { ui, format, text } => {
                (usize::from(*ui.max(format)) + 1).max(reg(text))
            }
            Self::TextProperty { key, text, .. } => (usize::from(*key) + 4).max(reg(text)),
            Self::TimeConversion { local, .. } => usize::from(*local) + 1,
            Self::EngineParameter { address, local, .. } => {
                (usize::from(*address) + 4).max(usize::from(*local) + 1)
            }
            Self::ZoneParameter {
                zone,
                parameter,
                local,
                ..
            } => usize::from((*zone).max(*parameter).max(*local)) + 1,
            Self::EngineDisplay {
                address,
                value,
                text,
            } => (usize::from(*address) + 4)
                .max(value.map_or(0, |v| usize::from(v) + 1))
                .max(reg(text)),
            Self::EngineLookup {
                group,
                owner,
                text,
                local,
                ..
            } => usize::from(*group.max(owner).max(local)) + 1 + reg(text),
            Self::Purge { group, local, .. } => usize::from(*group.max(local)) + 1,
            Self::Call { .. } | Self::Return | Self::ResetTimer => 0,
            Self::TextClear { text } => reg(text),
            Self::TextAppend { text: t, part } => reg(t).max(match part {
                TextPart::Constant(_) => 0,
                TextPart::Text(r) => reg(r),
                TextPart::Integer(l) | TextPart::Real(l) => usize::from(*l) + 1,
                TextPart::Table { index, .. } => usize::from(*index) + 1,
            }),
            Self::TextFind { text, local, .. } | Self::TextIndex { text, local } => {
                reg(text).max(usize::from(*local) + 1)
            }
            Self::CompareText {
                lhs, rhs, local, ..
            } => reg(lhs).max(reg(rhs)).max(usize::from(*local) + 1),
            Self::Store { key, local, .. } | Self::SharedStore { key, local, .. } => {
                (usize::from(*key) + STORE_KEY).max(usize::from(*local) + 1)
            }
            Self::ControlAt { index, local, .. } => usize::from(*index.max(local)) + 1,
            Self::Emit {
                args,
                count,
                text: t,
                ..
            } => (usize::from(*args) + usize::from(*count)).max(t.as_ref().map_or(0, reg)),
        }
    }

    /// (text cells, text constants) the operation addresses.
    pub(crate) fn texts(&self) -> Result<(usize, usize), Error> {
        let cell = |t: &TextRef| match *t {
            TextRef::Cell(c) => usize::try_from(c)
                .ok()
                .and_then(|c| c.checked_add(1))
                .ok_or(Error::Capacity),
            TextRef::Element { array, .. } => array.end(),
        };
        Ok(match self {
            Self::Midi {
                text: Some(text), ..
            }
            | Self::MidiFilename { text }
            | Self::TextClear { text } => (cell(text)?, 0),
            Self::TextAppend { text, part } => match part {
                TextPart::Constant(c) => (cell(text)?, usize::from(*c) + 1),
                TextPart::Text(r) => (cell(text)?.max(cell(r)?), 0),
                TextPart::Integer(_) | TextPart::Real(_) => (cell(text)?, 0),
                TextPart::Table { base, count, .. } => {
                    (cell(text)?, usize::from(*base) + usize::from(*count))
                }
            },
            Self::TextFind {
                text, base, count, ..
            } => (cell(text)?, usize::from(*base) + usize::from(*count)),
            Self::ReadWidgetDropText { text, .. }
            | Self::FileName { text, .. }
            | Self::TextIndex { text, .. }
            | Self::TextProperty { text, .. }
            | Self::EngineLookup { text, .. }
            | Self::EngineDisplay { text, .. } => (cell(text)?, 0),
            Self::CompareText { lhs, rhs, .. } => (cell(lhs)?.max(cell(rhs)?), 0),
            Self::Emit { text: Some(t), .. } => (cell(t)?, 0),
            _ => (0, 0),
        })
    }
}

/// Fixed-capacity UTF-8 text.
#[cfg_attr(feature = "cache", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "cache", serde(try_from = "String", into = "String"))]
#[derive(Clone, Copy)]
pub struct Text {
    len: u16,
    bytes: [u8; TEXT_CAPACITY],
}
impl Default for Text {
    fn default() -> Self {
        Self {
            len: 0,
            bytes: [0; TEXT_CAPACITY],
        }
    }
}
impl Text {
    /// Producer-boundary conversion; never silently truncate a file path/edit.
    pub fn try_new(text: &str) -> Result<Self, Error> {
        if text.len() > TEXT_CAPACITY {
            Err(Error::Capacity)
        } else {
            Ok(Self::new(text))
        }
    }
    pub fn new(text: &str) -> Self {
        let mut t = Self::default();
        t.push(text);
        t
    }
    pub fn as_str(&self) -> &str {
        // Only whole characters are ever appended.
        std::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or_default()
    }
    pub fn clear(&mut self) {
        self.len = 0;
    }
    /// Append whole characters while they fit.
    pub fn push(&mut self, text: &str) {
        let start = usize::from(self.len);
        let mut end = text.len().min(TEXT_CAPACITY - start);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        self.bytes[start..start + end].copy_from_slice(&text.as_bytes()[..end]);
        self.len += end as u16;
    }
}
impl std::fmt::Write for Text {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.push(s);
        Ok(())
    }
}
impl std::fmt::Debug for Text {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(self.as_str(), f)
    }
}
impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

/// Shortest decimal that round-trips, with at least one fractional digit.
pub fn write_real(out: &mut impl std::fmt::Write, value: f64) {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 1e15 {
        let _ = write!(out, "{value:.1}");
    } else {
        let _ = write!(out, "{value}");
    }
}

/// Bounded sorted key-value state. Capacity is reserved off audio.
#[derive(Debug, Default)]
pub(crate) struct Store {
    entries: Vec<([i32; STORE_KEY], i64)>,
    capacity: usize,
    pub dropped: u64,
}
impl Clone for Store {
    fn clone(&self) -> Self {
        let mut entries = Vec::with_capacity(self.capacity);
        entries.extend_from_slice(&self.entries);
        Self {
            entries,
            capacity: self.capacity,
            dropped: 0,
        }
    }
}
impl Store {
    pub(crate) fn new(
        mut entries: Vec<([i32; STORE_KEY], i64)>,
        capacity: usize,
    ) -> Result<Self, Error> {
        entries.sort_by_key(|e| e.0);
        entries.dedup_by_key(|e| e.0);
        let capacity = capacity.max(entries.len());
        entries
            .try_reserve_exact(capacity - entries.len())
            .map_err(|_| Error::Capacity)?;
        Ok(Self {
            entries,
            capacity,
            dropped: 0,
        })
    }
    pub fn get(&self, key: [i32; STORE_KEY]) -> Option<i64> {
        self.entries
            .binary_search_by_key(&key, |e| e.0)
            .ok()
            .map(|i| self.entries[i].1)
    }
    fn set(&mut self, key: [i32; STORE_KEY], value: i64) {
        match self.entries.binary_search_by_key(&key, |e| e.0) {
            Ok(i) => self.entries[i].1 = value,
            // ponytail: O(n) insertion shift; a fixed hash table if stores grow large.
            Err(i) if self.entries.len() < self.capacity => self.entries.insert(i, (key, value)),
            Err(_) => self.dropped += 1,
        }
    }
}

/// One script instance's mutable state, owned by its live plan generation.
#[derive(Debug, Default)]
pub(crate) struct ScriptBank {
    pub cells: Box<[i64]>,
    pub captured_cells: Option<Box<[u64]>>,
    pub dirty_cells: Option<Box<[u64]>>,
    pub texts: Box<[Text]>,
    pub store: Store,
    pub controls: Box<[Option<ControlId>]>,
    pub text_properties: Vec<([i32; STORE_KEY], Text)>,
    pub persistence_callback: Option<(BehaviorId, Option<crate::Outcome>)>,
}

#[inline]
pub(super) fn captures_cell(mask: Option<&[u64]>, cell: usize) -> bool {
    mask.is_none_or(|mask| {
        mask.get(cell / 64)
            .is_some_and(|word| word & (1 << (cell % 64)) != 0)
    })
}

#[inline]
pub(super) fn mark_captured_cell(
    mask: Option<&[u64]>,
    dirty: Option<&mut [u64]>,
    cell: usize,
    changed: bool,
) -> bool {
    let captured = changed && captures_cell(mask, cell);
    if captured && let Some(dirty) = dirty {
        dirty[cell / 64] |= 1 << (cell % 64);
    }
    captured
}

impl ScriptBank {
    pub fn mark_captured_cell(&mut self, cell: usize, changed: bool) -> bool {
        mark_captured_cell(
            self.captured_cells.as_deref(),
            self.dirty_cells.as_deref_mut(),
            cell,
            changed,
        )
    }
}

/// Immutable preparation keeps compact shared strings; only a live generation
/// needs fixed-capacity mutable text buffers for allocation-free script writes.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScriptInitial {
    pub cells: Box<[i64]>,
    pub texts: Box<[std::sync::Arc<str>]>,
    pub store: Store,
    pub controls: Box<[Option<ControlId>]>,
    pub text_properties: Vec<([i32; STORE_KEY], std::sync::Arc<str>)>,
}

impl ScriptInitial {
    pub fn bank(&self) -> ScriptBank {
        let mut text_properties = Vec::with_capacity(self.store.capacity);
        text_properties.extend(
            self.text_properties
                .iter()
                .map(|(key, text)| (*key, Text::new(text))),
        );
        ScriptBank {
            cells: self.cells.clone(),
            captured_cells: None,
            dirty_cells: None,
            texts: self.texts.iter().map(|text| Text::new(text)).collect(),
            store: self.store.clone(),
            controls: self.controls.clone(),
            text_properties,
            persistence_callback: None,
        }
    }
}

/// Initial non-integer resources of one script instance.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScriptResources {
    pub texts: Vec<String>,
    pub text_properties: Vec<([i32; STORE_KEY], String)>,
    pub store: Vec<([i32; STORE_KEY], i64)>,
    /// Entries the store may hold, including the initial ones.
    pub store_capacity: usize,
    /// Dense control table for `Op::ControlAt`.
    pub controls: Vec<Option<ControlId>>,
}
impl ScriptResources {
    pub(crate) fn apply(self, bank: &mut ScriptInitial) -> Result<(), Error> {
        bank.texts = self
            .texts
            .into_iter()
            .map(|text| std::sync::Arc::from(Text::new(&text).as_str()))
            .collect();
        bank.store = Store::new(self.store, self.store_capacity)?;
        bank.controls = self.controls.into_boxed_slice();
        bank.text_properties = Vec::with_capacity(self.text_properties.len());
        bank.text_properties.extend(
            self.text_properties
                .into_iter()
                .map(|(key, text)| (key, std::sync::Arc::from(Text::new(&text).as_str()))),
        );
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Effect {
    pub plan: crate::PlanId,
    pub instance: Option<crate::ScriptInstanceId>,
    pub service: u16,
    pub args: [i64; EFFECT_ARGS],
    pub count: u8,
    pub text: Option<Text>,
}
impl Effect {
    pub fn args(&self) -> &[i64] {
        &self.args[..usize::from(self.count)]
    }
}

/// Subroutine return positions retained by a suspended continuation.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Frames {
    pub(super) returns: [u32; CALL_DEPTH],
    pub(super) depth: u8,
}
impl Default for Frames {
    fn default() -> Self {
        Self {
            returns: [0; CALL_DEPTH],
            depth: 0,
        }
    }
}
impl Frames {
    pub(crate) fn callers(&self) -> &[u32] {
        &self.returns[..usize::from(self.depth)]
    }
}

pub(crate) struct OpState {
    pub effects: std::collections::VecDeque<Effect>,
    pub dropped_effects: u64,
    /// Text appends cut short at [`TEXT_CAPACITY`].
    pub truncated_texts: u64,
    pub host: [i64; HOST_VALUES],
    pub random: u64,
    timer_origin: std::time::Instant,
    engine_outcomes: std::collections::VecDeque<crate::EngineParameterOutcome>,
    dropped_engine_outcomes: u64,
}
impl Default for OpState {
    fn default() -> Self {
        Self {
            timer_origin: std::time::Instant::now(),
            engine_outcomes: std::collections::VecDeque::with_capacity(
                crate::engine_parameters::ENGINE_OUTCOME_CAPACITY,
            ),
            dropped_engine_outcomes: 0,
            effects: std::collections::VecDeque::with_capacity(EFFECT_CAPACITY),
            dropped_effects: 0,
            truncated_texts: 0,
            host: {
                let mut values = [0; HOST_VALUES];
                for (slot, value) in [
                    (8, 500000),
                    (9, 250000),
                    (10, 125000),
                    (11, 333333),
                    (12, 166667),
                    (13, 83333),
                    (14, 2000000),
                    (16, 4),
                    (17, 4),
                    (19, 120000),
                    (24, 2),
                ] {
                    values[slot] = value;
                }
                values
            },
            random: 0x9e37_79b9_7f4a_7c15,
        }
    }
}

fn i32_of(value: i64) -> Result<i32, Error> {
    i32::try_from(value).map_err(|_| Error::ArithmeticOverflow)
}

impl Runtime {
    pub fn take_engine_parameter_outcome(&mut self) -> Option<crate::EngineParameterOutcome> {
        self.ops.engine_outcomes.pop_front()
    }
    pub fn dropped_engine_parameter_outcomes(&self) -> u64 {
        self.ops.dropped_engine_outcomes
    }
    fn callback_ui_id(&self, id: BehaviorId) -> Result<i32, Error> {
        let c = self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(c.owner)?;
        Ok(self.plans.get(plan.0).unwrap().prepared.programs[c.program].ui_id)
    }

    fn record_engine_outcome(
        &mut self,
        id: BehaviorId,
        plan: crate::PlanId,
        address: Option<crate::EngineParameterAddress>,
        write: bool,
        result: Result<(), Error>,
    ) {
        if self.ops.engine_outcomes.len() == crate::engine_parameters::ENGINE_OUTCOME_CAPACITY {
            self.ops.dropped_engine_outcomes += 1;
        } else {
            self.ops
                .engine_outcomes
                .push_back(crate::EngineParameterOutcome {
                    plan,
                    program: self.behaviors.get(id.0).unwrap().program,
                    address,
                    write,
                    result,
                });
        }
    }
    /// Pending effects in emission order. Returning false stops and keeps the rest.
    pub fn drain_effects(&mut self, mut accept: impl FnMut(&Effect) -> bool) {
        while let Some(effect) = self.ops.effects.front() {
            if !accept(effect) {
                return;
            }
            self.ops.effects.pop_front();
        }
    }

    /// Effects dropped because the outbox was full.
    pub fn dropped_effects(&self) -> u64 {
        self.ops.dropped_effects
    }

    /// Runtime text appends that did not fit a text cell and were cut short.
    pub fn truncated_texts(&self) -> u64 {
        self.ops.truncated_texts
    }

    pub fn set_host_value(&mut self, slot: usize, value: i64) -> Result<(), Error> {
        *self.ops.host.get_mut(slot).ok_or(Error::InvalidInput)? = value;
        Ok(())
    }

    pub fn seed_random(&mut self, seed: u64) {
        self.ops.random = seed | 1;
    }

    pub fn script_text(
        &self,
        plan: crate::PlanId,
        instance: crate::ScriptInstanceId,
        cell: u32,
    ) -> Result<Text, Error> {
        self.script_bank(plan, instance)?
            .texts
            .get(cell as usize)
            .copied()
            .ok_or(Error::InvalidInput)
    }

    pub fn script_store(
        &self,
        plan: crate::PlanId,
        instance: crate::ScriptInstanceId,
        key: [i32; STORE_KEY],
    ) -> Result<Option<i64>, Error> {
        Ok(self.script_bank(plan, instance)?.store.get(key))
    }

    fn script_bank(
        &self,
        plan: crate::PlanId,
        instance: crate::ScriptInstanceId,
    ) -> Result<&ScriptBank, Error> {
        self.plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .scripts
            .get(usize::from(instance.0))
            .ok_or(Error::InvalidInput)
    }

    fn behavior_bank(&self, id: BehaviorId) -> Result<&ScriptBank, Error> {
        let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(c.owner)?;
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[c.program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        generation
            .scripts
            .get(usize::from(instance.0))
            .ok_or(Error::InvalidInput)
    }

    fn behavior_bank_mut(&mut self, id: BehaviorId) -> Result<&mut ScriptBank, Error> {
        let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(c.owner)?;
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let instance = generation.prepared.programs[c.program]
            .script_instance
            .ok_or(Error::InvalidInput)?;
        // ponytail: text/store writes conservatively invalidate snapshots; filter their
        // addresses too if measured text-heavy callbacks still copy large arrays.
        generation.script_revision = generation.script_revision.wrapping_add(1);
        generation
            .scripts
            .get_mut(usize::from(instance.0))
            .ok_or(Error::InvalidInput)
    }

    fn reg(&self, id: BehaviorId, local: u16) -> Result<i64, Error> {
        self.behavior_locals
            .get(id.0.index * self.behavior_stride + usize::from(local))
            .copied()
            .ok_or(Error::InvalidInput)
    }

    fn set_reg(&mut self, id: BehaviorId, local: u16, value: i64) -> Result<(), Error> {
        *self
            .behavior_locals
            .get_mut(id.0.index * self.behavior_stride + usize::from(local))
            .ok_or(Error::InvalidInput)? = value;
        Ok(())
    }

    fn engine_address(
        &mut self,
        id: BehaviorId,
        register: u16,
    ) -> Result<Option<crate::EngineParameterAddress>, Error> {
        let raw = self.reg(id, register)? as i32;
        let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
        let plan = self.behavior_plan(c.owner)?;
        let parameter = self.plans.get(plan.0).unwrap().prepared.programs[c.program]
            .engine_symbols
            .iter()
            .find(|(value, _)| *value == raw)
            .map(|(_, parameter)| *parameter);
        Ok(match parameter {
            Some(parameter) => Some(crate::EngineParameterAddress {
                parameter,
                group: self.reg(id, register + 1)? as i32,
                slot: self.reg(id, register + 2)? as i32,
                generic: self.reg(id, register + 3)? as i32,
            }),
            None => None,
        })
    }
    pub(super) fn text_cell(&self, id: BehaviorId, text: TextRef) -> Result<usize, Error> {
        Ok(match text {
            TextRef::Cell(cell) => cell as usize,
            TextRef::Element { array, index } => array.cell(self.reg(id, index)?)? as usize,
        })
    }

    fn continuation(&mut self, id: BehaviorId) -> Result<&mut Continuation, Error> {
        self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)
    }

    /// true means finished, matching `behavior_step`.
    pub(crate) fn op_step(
        &mut self,
        id: BehaviorId,
        owner: BehaviorOwner,
        op: Op,
    ) -> Result<bool, Error> {
        match op {
            Op::Midi {
                action,
                args,
                local,
                text,
            } => {
                let mut values = [0; 5];
                for (index, value) in values.iter_mut().take(action.arguments()).enumerate() {
                    *value = i32_of(self.reg(id, args + index as u16)?)?;
                }
                let text = match text {
                    Some(text) => {
                        let cell = self.text_cell(id, text)?;
                        Some(
                            *self
                                .behavior_bank(id)?
                                .texts
                                .get(cell)
                                .ok_or(Error::InvalidInput)?,
                        )
                    }
                    None => None,
                };
                let plan = self.behavior_plan(owner)?;
                let value = if action.asynchronous() {
                    self.request_midi(id, plan, action, values, text)?
                } else {
                    self.plans
                        .get_mut(plan.0)
                        .ok_or(Error::StaleHandle)?
                        .midi_object
                        .apply(action, &values, text)?
                };
                self.set_reg(id, local, i64::from(value))?;
            }
            Op::WaitMidi { local } => {
                let job = i32_of(self.reg(id, local)?)?;
                let plan = self.behavior_plan(owner)?;
                if self
                    .plans
                    .get(plan.0)
                    .ok_or(Error::StaleHandle)?
                    .midi_object
                    .jobs
                    .iter()
                    .any(|j| j.id == job)
                {
                    let c = self.behaviors.get_mut(id.0).ok_or(Error::StaleHandle)?;
                    if !c.disable_wait {
                        c.waiting = true;
                        c.async_wait = Some(job);
                        // An admitted async wait also ends continuous preemption.
                        c.yielded_at = None;
                        return Ok(true);
                    }
                }
            }
            Op::MidiFilename { text } => {
                let plan = self.behavior_plan(owner)?;
                let value = self
                    .plans
                    .get(plan.0)
                    .ok_or(Error::StaleHandle)?
                    .midi_object
                    .filename;
                let cell = self.text_cell(id, text)?;
                self.behavior_bank_mut(id)?.texts[cell].push(value.as_str());
            }
            Op::Real {
                lhs,
                rhs,
                operation,
            } => {
                let value = operation.apply(real(self.reg(id, lhs)?), real(self.reg(id, rhs)?));
                self.set_reg(id, lhs, real_bits(value))?;
            }
            Op::RealUnary { local, operation } => {
                let value = operation.apply(real(self.reg(id, local)?));
                self.set_reg(id, local, real_bits(value))?;
            }
            Op::CompareReal {
                lhs,
                rhs,
                comparison,
            } => {
                let (l, r) = (real(self.reg(id, lhs)?), real(self.reg(id, rhs)?));
                let result = match comparison {
                    Comparison::Equal => l == r,
                    Comparison::NotEqual => l != r,
                    Comparison::Less => l < r,
                    Comparison::LessEqual => l <= r,
                    Comparison::Greater => l > r,
                    Comparison::GreaterEqual => l >= r,
                };
                self.set_reg(id, lhs, i64::from(result))?;
            }
            Op::IntegerToReal { local } => {
                let value = f64::from(i32_of(self.reg(id, local)?)?);
                self.set_reg(id, local, real_bits(value))?;
            }
            Op::RealToInteger { local } => {
                let value = real_to_i32(real(self.reg(id, local)?));
                self.set_reg(id, local, i64::from(value))?;
            }
            Op::Integer {
                lhs,
                rhs,
                operation,
            } => {
                let value =
                    operation.apply(i32_of(self.reg(id, lhs)?)?, i32_of(self.reg(id, rhs)?)?);
                self.set_reg(id, lhs, i64::from(value))?;
            }
            Op::Random { lhs, rhs } => {
                let (a, b) = (i32_of(self.reg(id, lhs)?)?, i32_of(self.reg(id, rhs)?)?);
                let (low, high) = (i64::from(a.min(b)), i64::from(a.max(b)));
                // xorshift64*; modulo bias is below 2^-31 for 32-bit spans.
                let mut x = self.ops.random;
                x ^= x >> 12;
                x ^= x << 25;
                x ^= x >> 27;
                self.ops.random = x;
                let draw = x.wrapping_mul(0x2545_f491_4f6c_dd1d);
                let span = (high - low + 1) as u64;
                self.set_reg(id, lhs, low + (draw % span) as i64)?;
            }
            Op::Call { target } => {
                let c = self.continuation(id)?;
                let depth = usize::from(c.frames.depth);
                if depth == CALL_DEPTH {
                    return Err(Error::Capacity);
                }
                c.frames.returns[depth] = u32::try_from(c.pc).map_err(|_| Error::Capacity)?;
                c.frames.depth += 1;
                c.pc = target as usize;
            }
            Op::Return => {
                let c = self.continuation(id)?;
                if c.frames.depth == 0 {
                    c.outcome = Some(super::Outcome::Finished);
                    return Ok(true);
                }
                c.frames.depth -= 1;
                c.pc = c.frames.returns[usize::from(c.frames.depth)] as usize;
            }
            Op::TextClear { text } => {
                let cell = self.text_cell(id, text)?;
                self.behavior_bank_mut(id)?
                    .texts
                    .get_mut(cell)
                    .ok_or(Error::InvalidInput)?
                    .clear();
            }
            Op::TextAppend { text, part } => {
                let cell = self.text_cell(id, text)?;
                let constant = match part {
                    TextPart::Constant(index) => Some(usize::from(index)),
                    TextPart::Table { base, count, index } => {
                        let at = self.reg(id, index)?;
                        u16::try_from(at)
                            .ok()
                            .filter(|at| *at < count)
                            .map(|at| usize::from(base) + usize::from(at))
                    }
                    _ => None,
                };
                if let Some(index) = constant {
                    let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
                    let plan = self.behavior_plan(c.owner)?;
                    let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
                    let program = &generation.prepared.programs[c.program];
                    let source = program.texts.get(index).ok_or(Error::InvalidInput)?;
                    let instance = program.script_instance.ok_or(Error::InvalidInput)?;
                    generation.script_revision = generation.script_revision.wrapping_add(1);
                    let target = generation
                        .scripts
                        .get_mut(usize::from(instance.0))
                        .and_then(|bank| bank.texts.get_mut(cell))
                        .ok_or(Error::InvalidInput)?;
                    let before = target.len;
                    // v1 pushes the borrowed constant directly into mutable text storage.
                    target.push(source);
                    if usize::from(target.len - before) < source.len() {
                        self.ops.truncated_texts += 1;
                    }
                    return Ok(false);
                }
                let mut piece = Text::default();
                match part {
                    TextPart::Text(source) => {
                        let source = self.text_cell(id, source)?;
                        piece = *self
                            .behavior_bank(id)?
                            .texts
                            .get(source)
                            .ok_or(Error::InvalidInput)?;
                    }
                    TextPart::Integer(local) => {
                        use std::fmt::Write;
                        let _ = write!(piece, "{}", self.reg(id, local)?);
                    }
                    TextPart::Real(local) => write_real(&mut piece, real(self.reg(id, local)?)),
                    TextPart::Constant(_) | TextPart::Table { .. } => {}
                }
                let target = self
                    .behavior_bank_mut(id)?
                    .texts
                    .get_mut(cell)
                    .ok_or(Error::InvalidInput)?;
                let before = target.len;
                target.push(piece.as_str());
                if usize::from(target.len - before) < piece.as_str().len() {
                    self.ops.truncated_texts += 1;
                }
            }
            Op::CompareText {
                lhs,
                rhs,
                local,
                comparison,
            } => {
                let (l, r) = (self.text_cell(id, lhs)?, self.text_cell(id, rhs)?);
                let bank = self.behavior_bank(id)?;
                let (l, r) = (
                    bank.texts.get(l).ok_or(Error::InvalidInput)?,
                    bank.texts.get(r).ok_or(Error::InvalidInput)?,
                );
                let order = l.as_str().cmp(r.as_str()) as i64;
                self.set_reg(id, local, i64::from(comparison.apply(order, 0)))?;
            }
            Op::TextFind {
                text,
                base,
                count,
                local,
            } => {
                let cell = self.text_cell(id, text)?;
                let name = *self
                    .behavior_bank(id)?
                    .texts
                    .get(cell)
                    .ok_or(Error::InvalidInput)?;
                let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
                let plan = self.behavior_plan(c.owner)?;
                let table = &self.plans.get(plan.0).unwrap().prepared.programs[c.program].texts;
                let found = table
                    .get(usize::from(base)..usize::from(base) + usize::from(count))
                    .ok_or(Error::InvalidInput)?
                    .iter()
                    .position(|t| t.eq_ignore_ascii_case(name.as_str()));
                self.set_reg(id, local, found.map_or(-1, |i| i as i64))?;
            }
            Op::TextIndex { text, local } => {
                let cell = self.text_cell(id, text)?;
                let index = name_index(
                    self.behavior_bank(id)?
                        .texts
                        .get(cell)
                        .ok_or(Error::InvalidInput)?
                        .as_str(),
                );
                self.set_reg(id, local, i64::from(index))?;
            }
            Op::TimeConversion {
                local,
                ticks_to_micros,
            } => {
                let quarter = self.ops.host[8].max(1);
                let value = i128::from(self.reg(id, local)?);
                let value = if ticks_to_micros {
                    value * i128::from(quarter) / 960
                } else {
                    value * 960 / i128::from(quarter)
                };
                self.set_reg(
                    id,
                    local,
                    value.clamp(i128::from(i32::MIN), i128::from(i32::MAX)) as i64,
                )?;
            }
            Op::TextProperty { key, text, write } => {
                let mut k = [0; STORE_KEY];
                for (i, v) in k.iter_mut().enumerate() {
                    *v = self.reg(id, key + i as u16)? as i32;
                }
                let cell = self.text_cell(id, text)?;
                let bank = self.behavior_bank_mut(id)?;
                if write {
                    let value = *bank.texts.get(cell).ok_or(Error::InvalidInput)?;
                    if let Some((_, v)) = bank.text_properties.iter_mut().find(|(key, _)| *key == k)
                    {
                        *v = value;
                    } else if bank.text_properties.len() < bank.text_properties.capacity() {
                        bank.text_properties.push((k, value));
                    } else {
                        return Err(Error::Capacity);
                    }
                } else {
                    let value = bank
                        .text_properties
                        .iter()
                        .find(|(key, _)| *key == k)
                        .map_or(Text::default(), |(_, v)| *v);
                    *bank.texts.get_mut(cell).ok_or(Error::InvalidInput)? = value;
                }
            }
            Op::ZoneParameter {
                zone,
                parameter,
                selectors,
                local,
            } => {
                let parameter = self.reg(id, parameter)?;
                let parameter = selectors
                    .iter()
                    .position(|selector| selector.map(i64::from) == Some(parameter))
                    .map(|index| {
                        [
                            crate::ZoneParameter::Group,
                            crate::ZoneParameter::LowKey,
                            crate::ZoneParameter::HighKey,
                        ][index]
                    });
                let plan = self.behavior_plan(owner)?;
                let value = parameter
                    .and_then(|parameter| {
                        let zone = u32::try_from(self.reg(id, zone).ok()?).ok()?;
                        self.plans
                            .get(plan.0)?
                            .prepared
                            .source_zone_parameter(zone, parameter)
                    })
                    .unwrap_or(0);
                self.set_reg(id, local, i64::from(value))?;
            }
            Op::EngineParameter {
                address,
                local,
                write,
            } => {
                let plan = self.behavior_plan(owner)?;
                let address = self.engine_address(id, address)?;
                if let Some(address) = address {
                    if write {
                        let result = self.set_engine_parameter_in(
                            plan,
                            address,
                            self.reg(id, local)? as i32,
                        );
                        self.record_engine_outcome(id, plan, Some(address), true, result);
                        if let Err(error) = result
                            && error != Error::InvalidInput
                        {
                            return Err(error);
                        }
                    } else {
                        let result = self.engine_parameter_in(plan, address);
                        self.record_engine_outcome(
                            id,
                            plan,
                            Some(address),
                            false,
                            result.map(|_| ()),
                        );
                        let value = match result {
                            Ok(v) => v,
                            Err(Error::InvalidInput) => 0,
                            Err(error) => return Err(error),
                        };
                        self.set_reg(id, local, value.into())?;
                    }
                } else {
                    self.record_engine_outcome(id, plan, None, write, Err(Error::InvalidInput));
                    if !write {
                        self.set_reg(id, local, 0)?;
                    }
                }
            }
            Op::EngineDisplay {
                address,
                value,
                text,
            } => {
                let cell = self.text_cell(id, text)?;
                let mut result = Text::default();
                if let Some(address) = self.engine_address(id, address)? {
                    let plan = self.behavior_plan(owner)?;
                    let value = match value {
                        Some(v) => self.reg(id, v)? as i32,
                        None => {
                            let read_result = self.engine_parameter_in(plan, address);
                            self.record_engine_outcome(
                                id,
                                plan,
                                Some(address),
                                false,
                                read_result.map(|_| ()),
                            );
                            match read_result {
                                Ok(value) => value,
                                Err(Error::InvalidInput) => {
                                    result = Text::new("?");
                                    0
                                }
                                Err(error) => return Err(error),
                            }
                        }
                    };
                    if result.as_str().is_empty() {
                        crate::engine_parameters::display(address.parameter, value, &mut result);
                    }
                }
                *self
                    .behavior_bank_mut(id)?
                    .texts
                    .get_mut(cell)
                    .ok_or(Error::InvalidInput)? = result;
            }
            Op::EngineLookup {
                group,
                owner: lookup_owner,
                target,
                text,
                local,
            } => {
                let group = self.reg(id, group)? as i32;
                let lookup_owner = self.reg(id, lookup_owner)? as i32;
                let cell = self.text_cell(id, text)?;
                let name = *self
                    .behavior_bank(id)?
                    .texts
                    .get(cell)
                    .ok_or(Error::InvalidInput)?;
                let plan = self.behavior_plan(owner)?;
                let found = self
                    .plans
                    .get(plan.0)
                    .unwrap()
                    .prepared
                    .engine_lookups
                    .iter()
                    .find(|l| {
                        l.group == group
                            && l.owner == lookup_owner
                            && l.target == target
                            && l.name.eq_ignore_ascii_case(name.as_str())
                    })
                    .map_or(-1, |l| l.index);
                self.set_reg(id, local, found.into())?;
            }
            Op::Purge {
                group,
                local,
                write,
            } => {
                let plan = self.behavior_plan(owner)?;
                let group = self.reg(id, group)?;
                if write {
                    self.write_param(
                        plan,
                        crate::ParamScope::Group,
                        group,
                        crate::ModTarget::Attenuate,
                        if self.reg(id, local)? == 0 { 0 } else { 1000 },
                        false,
                    )?;
                } else {
                    let value = self.read_param(
                        plan,
                        crate::ParamScope::Group,
                        group,
                        crate::ModTarget::Attenuate,
                    )?;
                    self.set_reg(id, local, i64::from(value != 0))?;
                }
            }
            Op::Store { key, local, write } => {
                let mut k = [0; STORE_KEY];
                for (i, slot) in k.iter_mut().enumerate() {
                    *slot = i32_of(self.reg(id, key + i as u16)?)?;
                }
                if write {
                    let value = self.reg(id, local)?;
                    self.behavior_bank_mut(id)?.store.set(k, value);
                } else if let Some(value) = self.behavior_bank(id)?.store.get(k) {
                    self.set_reg(id, local, value)?;
                }
            }
            Op::SharedStore { key, local, write } => {
                let mut k = [0; STORE_KEY];
                for (i, slot) in k.iter_mut().enumerate() {
                    *slot = i32_of(self.reg(id, key + i as u16)?)?;
                }
                let plan = self.behavior_plan(owner)?;
                if write {
                    let value = self.reg(id, local)?;
                    self.plans
                        .get_mut(plan.0)
                        .unwrap()
                        .script
                        .shared
                        .set(k, value);
                } else if let Some(value) = self.plans.get(plan.0).unwrap().script.shared.get(k) {
                    self.set_reg(id, local, value)?;
                }
            }
            Op::ControlAt {
                index,
                local,
                write,
            } => {
                let index = self.reg(id, index)?;
                let control = usize::try_from(index)
                    .ok()
                    .and_then(|i| self.behavior_bank(id).ok()?.controls.get(i).copied())
                    .flatten();
                let Some(control) = control else {
                    return Ok(false);
                };
                let plan = self.behavior_plan(owner)?;
                if write {
                    let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
                    let definition =
                        generation.prepared.controls[generation.prepared.control_index(control)?];
                    let super::ControlDomain::Integer { min, max } = definition.domain else {
                        return Err(Error::InvalidInput);
                    };
                    let value = super::ControlValue::Integer(self.reg(id, local)?.clamp(min, max));
                    self.edit_controls_now(
                        plan,
                        None,
                        &[super::ControlWrite { id: control, value }],
                    )?;
                } else {
                    let super::ControlValue::Integer(value) = self.control_value(plan, control)?
                    else {
                        return Err(Error::InvalidInput);
                    };
                    self.set_reg(id, local, value)?;
                }
            }
            Op::FileName { ui, format, text } => {
                let ui = self.reg(id, ui)? as i32;
                let format = self.reg(id, format)?;
                let callback = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
                let plan = self.behavior_plan(owner)?;
                let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
                let instance = generation.prepared.programs[callback.program]
                    .script_instance
                    .ok_or(Error::InvalidInput)?;
                let definition = generation
                    .prepared
                    .widgets
                    .iter()
                    .find(|w| w.instance == instance && w.ui_id == ui)
                    .ok_or(Error::InvalidInput)?;
                let crate::WidgetStorage::FileSelection { offset } = definition.storage else {
                    return Err(Error::InvalidInput);
                };
                let path = generation.scripts[instance.0 as usize].texts[offset as usize];
                let filename = path.as_str().rsplit('/').next().unwrap_or("");
                let result = Text::new(match format {
                    0 => filename
                        .rsplit_once('.')
                        .filter(|(stem, _)| !stem.is_empty())
                        .map_or(filename, |(stem, _)| stem),
                    1 => filename,
                    2 => path.as_str(),
                    _ => return Err(Error::InvalidInput),
                });
                let cell = self.text_cell(id, text)?;
                self.behavior_bank_mut(id)?.texts[cell] = result;
            }
            Op::ReadCallbackUiId { local } => {
                let ui = self.callback_ui_id(id)?;
                self.set_reg(id, local, i64::from(ui))?;
            }
            Op::ReadWidgetDropCount { ui, kind, local } => {
                let ui = if ui == i32::MIN {
                    self.callback_ui_id(id)?
                } else {
                    ui
                };
                if kind >= 3 {
                    return Err(Error::InvalidInput);
                }
                let count = if let Some(drop) = self.callback_drop_storage(id, ui)? {
                    self.behavior_bank(id)?.cells[drop.counts as usize + kind as usize]
                } else {
                    0
                };
                self.set_reg(id, local, count)?;
            }
            Op::ReadWidgetDropText {
                ui,
                kind,
                index,
                text,
            } => {
                let ui = if ui == i32::MIN {
                    self.callback_ui_id(id)?
                } else {
                    ui
                };
                if kind >= 3 {
                    return Err(Error::InvalidInput);
                }
                let index =
                    usize::try_from(self.reg(id, index)?).map_err(|_| Error::InvalidInput)?;
                let path = if let Some(drop) = self.callback_drop_storage(id, ui)? {
                    let bank = self.behavior_bank_mut(id)?;
                    if index >= bank.cells[drop.counts as usize + kind as usize] as usize {
                        Text::default()
                    } else {
                        bank.texts[drop.texts as usize
                            + kind as usize * crate::WIDGET_DROP_CAPACITY as usize
                            + index]
                    }
                } else {
                    Text::default()
                };
                let cell = self.text_cell(id, text)?;
                self.behavior_bank_mut(id)?.texts[cell] = path;
            }
            Op::ReadWidgetEventParameter { local } => {
                let index =
                    usize::try_from(self.reg(id, local)?).map_err(|_| Error::InvalidInput)?;
                let parameters = match self.behaviors.get(id.0).ok_or(Error::StaleHandle)?.context {
                    crate::behavior::PlanContext::Control(e) => e.interaction.event_par,
                    _ => [0; 4],
                };
                let value = *parameters.get(index).ok_or(Error::InvalidInput)?;
                self.set_reg(id, local, i64::from(value))?;
            }
            Op::ReadWidgetInteraction { local, field } => {
                let interaction = match self.behaviors.get(id.0).ok_or(Error::StaleHandle)?.context
                {
                    crate::behavior::PlanContext::Control(e) => e.interaction,
                    _ => crate::WidgetInteraction::default(),
                };
                let value = match field {
                    0 => i64::from(interaction.index),
                    1 => i64::from(interaction.cursor),
                    2 => i64::from(interaction.modifiers & 1 != 0),
                    3 => i64::from(interaction.modifiers & 2 != 0),
                    4 => i64::from(interaction.modifiers & 4 != 0),
                    5 => i64::from(interaction.event),
                    6 => i64::from(interaction.mouse_over),
                    _ => return Err(Error::InvalidInput),
                };
                self.set_reg(id, local, value)?;
            }
            Op::ReadHost { local, slot } => {
                let result = self
                    .behaviors
                    .get(id.0)
                    .ok_or(Error::StaleHandle)?
                    .async_result;
                let value = match (slot, result) {
                    (6, Some((job, _))) => i64::from(job),
                    (7, Some((_, status))) => i64::from(status),
                    _ => *self
                        .ops
                        .host
                        .get(usize::from(slot))
                        .ok_or(Error::InvalidInput)?,
                };
                self.set_reg(id, local, value)?;
            }
            Op::ReadClock { local, micros } => {
                let elapsed = u128::from(self.now) * 1_000_000
                    / u128::from(self.rate)
                    / u128::from(micros.max(1));
                self.set_reg(id, local, i64::from(elapsed as u32 as i32))?;
            }
            Op::ReadTimer { local } => {
                self.set_reg(
                    id,
                    local,
                    i64::from(self.ops.timer_origin.elapsed().as_micros() as u32 as i32),
                )?;
            }
            Op::ResetTimer => self.ops.timer_origin = std::time::Instant::now(),
            Op::Emit {
                service,
                args,
                count,
                text,
            } => {
                if self.ops.effects.len() == EFFECT_CAPACITY {
                    self.ops.dropped_effects += 1;
                    return Ok(false);
                }
                let mut values = [0; EFFECT_ARGS];
                for (i, value) in values.iter_mut().take(usize::from(count)).enumerate() {
                    *value = self.reg(id, args + i as u16)?;
                }
                let text = match text {
                    Some(text) => {
                        let cell = self.text_cell(id, text)?;
                        Some(
                            *self
                                .behavior_bank(id)?
                                .texts
                                .get(cell)
                                .ok_or(Error::InvalidInput)?,
                        )
                    }
                    None => None,
                };
                let c = *self.behaviors.get(id.0).ok_or(Error::StaleHandle)?;
                let plan = self.behavior_plan(c.owner)?;
                let instance =
                    self.plans.get(plan.0).unwrap().prepared.programs[c.program].script_instance;
                self.ops.effects.push_back(Effect {
                    plan,
                    instance,
                    service,
                    args: values,
                    count,
                    text,
                });
            }
        }
        Ok(false)
    }
}

#[cfg(test)]
mod initial_text_tests {
    use super::*;
    #[test]
    fn shared_initial_texts_make_independent_mutable_banks() {
        let mut initial = ScriptInitial::default();
        ScriptResources {
            texts: vec!["seed".into()],
            text_properties: vec![([1, 0, 0, 0], "named".into())],
            store_capacity: 4,
            ..Default::default()
        }
        .apply(&mut initial)
        .unwrap();
        let clone = initial.clone();
        assert!(std::sync::Arc::ptr_eq(&initial.texts[0], &clone.texts[0]));
        assert!(std::sync::Arc::ptr_eq(
            &initial.text_properties[0].1,
            &clone.text_properties[0].1
        ));
        let mut first = initial.bank();
        let second = clone.bank();
        first.texts[0].push(" changed");
        first.text_properties[0].1.clear();
        assert_eq!(second.texts[0].as_str(), "seed");
        assert_eq!(second.text_properties[0].1.as_str(), "named");
        assert!(first.text_properties.capacity() >= 4);
        assert_eq!(&*initial.texts[0], "seed");
    }
}

#[cfg(feature = "cache")]
impl From<Text> for String {
    fn from(value: Text) -> Self {
        value.as_str().to_owned()
    }
}
#[cfg(feature = "cache")]
impl TryFrom<String> for Text {
    type Error = Error;
    fn try_from(value: String) -> Result<Self, Error> {
        Self::try_new(&value)
    }
}
