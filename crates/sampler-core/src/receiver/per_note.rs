//! Transport-owned MIDI2 note-number controller cohorts. No voice/sample clock,
//! expression cache, allocation, or callback belongs to the update path.
use crate::Error;

pub const CENTER: u32 = 0x8000_0000;
/// Receiver default retained from the product's existing PN-bend contract;
/// this is device policy, not a claim that the protocol mandates this default.
const DEFAULT_RANGE: f64 = 48.0;
const CHANNELS: usize = 16;
const KEYS: usize = 128;

#[derive(Clone, Copy)]
pub struct KeyId(usize);
#[derive(Clone, Copy)]
pub struct ChannelId(usize);
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyState {
    pub cohort: u64,
    pub bend: u32,
}
impl Default for KeyState {
    fn default() -> Self { Self { cohort: 1, bend: CENTER } }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snapshot {
    pub cohort: u64,
    pub pitch: f64,
}

pub struct PerNoteState {
    groups: [Option<u8>; 16],
    keys: Vec<KeyState>,
    ranges: Vec<f64>,
}
impl PerNoteState {
    /// All enabled address/key rows are priced and reserved before returning.
    /// Values can therefore precede their first NoteOn without an RT map grow.
    pub fn required_bytes(group_mask:u16)->Result<usize,Error> {
        let channels=(group_mask.count_ones() as usize).checked_mul(CHANNELS).ok_or(Error::Capacity)?;
        let keys=channels.checked_mul(KEYS).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<KeyState>(keys).map_err(|_|Error::Capacity)?.size()
            .checked_add(std::alloc::Layout::array::<f64>(channels).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)
    }
    pub fn retained_bytes(&self)->Result<usize,Error> {
        std::alloc::Layout::array::<KeyState>(self.keys.capacity()).map_err(|_|Error::Capacity)?.size()
            .checked_add(std::alloc::Layout::array::<f64>(self.ranges.capacity()).map_err(|_|Error::Capacity)?.size()).ok_or(Error::Capacity)
    }
    pub fn new(group_mask: u16) -> Result<Self, Error> {
        Self::required_bytes(group_mask)?;
        let mut groups = [None;16]; let mut count = 0usize;
        for (group, row) in groups.iter_mut().enumerate() {
            if group_mask & (1 << group) != 0 { *row = Some(count as u8); count += 1; }
        }
        let channels = count.checked_mul(CHANNELS).ok_or(Error::Capacity)?;
        let key_count = channels.checked_mul(KEYS).ok_or(Error::Capacity)?;
        std::alloc::Layout::array::<KeyState>(key_count).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<f64>(channels).map_err(|_| Error::Capacity)?;
        let mut keys = Vec::new(); keys.try_reserve_exact(key_count).map_err(|_| Error::Capacity)?;
        keys.resize(key_count, KeyState::default());
        let mut ranges = Vec::new(); ranges.try_reserve_exact(channels).map_err(|_| Error::Capacity)?;
        ranges.resize(channels, DEFAULT_RANGE);
        // Price actual retained capacities, not just requested logical lengths.
        std::alloc::Layout::array::<KeyState>(keys.capacity()).map_err(|_| Error::Capacity)?;
        std::alloc::Layout::array::<f64>(ranges.capacity()).map_err(|_| Error::Capacity)?;
        Ok(Self { groups, keys, ranges })
    }

    pub fn channel(&self, group: u8, channel: u8) -> Result<ChannelId, Error> {
        if channel >= 16 { return Err(Error::InvalidInput); }
        let group = self.groups.get(usize::from(group)).copied().flatten().ok_or(Error::InvalidInput)?;
        Ok(ChannelId(usize::from(group) * CHANNELS + usize::from(channel)))
    }
    pub fn key(&self, group: u8, channel: u8, key: u8) -> Result<(KeyId, KeyState), Error> {
        if key >= 128 { return Err(Error::InvalidInput); }
        let channel = self.channel(group, channel)?;
        let id = KeyId(channel.0 * KEYS + usize::from(key));
        let value = self.keys.get(id.0).copied().ok_or(Error::InvalidInput)?;
        Ok((id, value))
    }
    pub fn range(&self, channel: ChannelId) -> f64 { self.ranges[channel.0] }
    pub fn snapshot(&self, group: u8, channel: u8, key: u8) -> Result<Snapshot, Error> {
        let (_, state) = self.key(group, channel, key)?;
        let channel = self.channel(group, channel)?;
        Ok(Self::project(state, self.range(channel)))
    }
    pub fn project(state: KeyState, sensitivity: f64) -> Snapshot {
        Snapshot { cohort: state.cohort, pitch: normalized(state.bend) * sensitivity }
    }
    pub fn managed(state: KeyState, detach: bool, reset: bool) -> Result<KeyState, Error> {
        Ok(KeyState { cohort: if detach { state.cohort.checked_add(1).ok_or(Error::Capacity)? } else { state.cohort },
            bend: if reset { CENTER } else { state.bend } })
    }
    pub fn commit_key(&mut self, id: KeyId, state: KeyState) { self.keys[id.0] = state; }
    pub fn commit_range(&mut self, id: ChannelId, range: f64) { self.ranges[id.0] = range; }
}

/// Exact centre and signed endpoints; preserve all32 incoming bits until this
/// declared normalized projection. No 7/14-bit MIDI1 translation is involved.
pub fn normalized(value: u32) -> f64 {
    let centered = f64::from(value) - f64::from(CENTER);
    centered / if centered < 0.0 { f64::from(CENTER) } else { f64::from(CENTER - 1) }
}
