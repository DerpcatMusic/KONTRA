//! Bounded physical-zone routing over immutable fully lowered templates.
//! Integrates only after worker builds the full ordinary region/DSP bank.
use crate::{BehaviorId, Error, ScriptInstanceId, ZoneParameter};
use std::{mem::size_of, ops::Range};
/// Absolute retained-job ceiling, not the default size of every source.
/// This is KONTRA's resource policy; native Kontakt's limit is unverified.
pub const MAX_ZONE_BATCH_JOBS: usize = 65_536;

/// One group/low-key/high-key write for every known authoritative physical ID,
/// preserving the existing256-job allowance for small nonempty sources.
/// Repeated writes retain distinct async identities and consume the same budget.
/// Oversized sources fail preparation rather than silently truncating a batch.
pub fn source_zone_job_capacity(known_zones: usize) -> Result<usize, Error> {
    if known_zones == 0 {
        return Ok(0);
    }
    let jobs = known_zones.checked_mul(3).ok_or(Error::Capacity)?.max(256);
    if jobs > MAX_ZONE_BATCH_JOBS {
        return Err(Error::Capacity);
    }
    Ok(jobs)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceZoneHome {
    pub group: u32,
    pub low_key: u8,
    pub high_key: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceZoneIdentity {
    pub id: Option<u32>,
    pub home: SourceZoneHome,
    pub region: Option<usize>,
}
pub(crate) struct SourceZoneCatalog {
    rows: Box<[SourceZoneIdentity]>,
    lookup: Box<[(u32, usize)]>,
    pub bytes: usize,
}
impl SourceZoneCatalog {
    pub fn new(
        rows: Vec<SourceZoneIdentity>,
        regions: usize,
        max_zones: usize,
        max_bytes: usize,
    ) -> Result<Self, Error> {
        if rows.len() > max_zones {
            return Err(Error::Capacity);
        }
        if rows.iter().any(|r| {
            r.home.low_key > r.home.high_key
                || r.home.high_key > 127
                || r.home.group > i32::MAX as u32
                || r.region.is_some_and(|r| r >= regions)
        }) {
            return Err(Error::InvalidInput);
        }
        let known = rows.iter().filter(|r| r.id.is_some()).count();
        let bytes = add(
            size_of::<Self>(),
            add(
                mul(rows.len(), size_of::<SourceZoneIdentity>())?,
                mul(known, size_of::<(u32, usize)>())?,
            )?,
        )?;
        if bytes > max_bytes {
            return Err(Error::Capacity);
        }
        let mut lookup = Vec::with_capacity(known);
        lookup.extend(
            rows.iter()
                .enumerate()
                .filter_map(|(i, r)| r.id.map(|id| (id, i))),
        );
        // A shared identity addresses its first zone, as Kontakt's zone lookup does.
        lookup.sort_unstable();
        lookup.dedup_by_key(|r| r.0);
        Ok(Self {
            rows: rows.into_boxed_slice(),
            lookup: lookup.into_boxed_slice(),
            bytes,
        })
    }
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn identity(&self, index: usize) -> Option<u32> {
        self.rows.get(index)?.id
    }
    pub fn find(&self, id: u32) -> Option<&SourceZoneIdentity> {
        let at = self.lookup.binary_search_by_key(&id, |r| r.0).ok()?;
        Some(&self.rows[self.lookup[at].1])
    }
}

#[derive(Clone, Debug)]
pub struct ZoneRouteVariant {
    pub group: u32,
    pub region: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct ZoneRouting {
    pub source_zone: u32,
    pub group: u32,
    pub low_key: u8,
    pub high_key: u8,
    pub variants: Vec<ZoneRouteVariant>,
}
#[derive(Clone, Copy, Debug)]
pub struct ZoneRoutingLimits {
    pub zones: usize,
    pub variants: usize,
    pub candidates: usize,
    pub jobs: usize,
    /// Prepared routing metadata PLUS one generation's mutable routing storage.
    pub bytes: usize,
}
#[derive(Clone, Copy)]
pub(crate) struct CandidateIdentity {
    pub region: usize,
    pub key: u8,
    pub initial: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Home {
    variant: usize,
    low: u8,
    high: u8,
}
struct Zone {
    source: u32,
    variants: Range<usize>,
    initial: Home,
    initial_keys: [u64; 2],
}
struct Variant {
    group: u32,
    region: Option<usize>,
    candidates: [u32; 128],
}
pub(crate) struct PreparedRouting {
    zones: Box<[Zone]>,
    variants: Box<[Variant]>,
    region_zones: Box<[u32]>,
    initial_words: Box<[u64]>,
    jobs: usize,
    pub bytes: usize,
    mode: u8,
    pub partial_initial_homes: usize,
    pub partial_initial_keys: usize,
    pub unavailable_destinations: usize,
    pub catalog_only_initial_homes: usize,
}
const ABSENT: u32 = u32::MAX;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRoutingRefusal {
    Capacity,
    UnknownZone,
    UnknownGroup,
    UnavailableTemplate,
    InvalidKey,
    UnsupportedKeyRange,
    Policy,
    Cancelled,
}
#[derive(Clone, Copy, Debug)]
pub struct ZoneRoutingReport {
    pub zones: usize,
    pub templates: usize,
    pub metadata_bytes: usize,
    pub partial_initial_homes: usize,
    pub partial_initial_keys: usize,
    pub unavailable_destinations: usize,
    pub catalog_only_initial_homes: usize,
    pub job_capacity: usize,
    pub pending_jobs: usize,
    pub refusals: [u64; 8],
    pub last_refusal: Option<ZoneRoutingRefusal>,
}
#[derive(Clone, Copy)]
struct Change {
    zone: usize,
    parameter: ZoneParameter,
    value: i32,
}
/// Exact immutable callback captured at acceptance. There is no later lookup
/// that could add an unreserved callback from a different instance/context.
#[derive(Clone, Copy)]
pub(crate) struct ReservedCallback {
    pub program: usize,
    pub context: crate::behavior::PlanContext,
}
#[derive(Clone, Copy)]
pub(crate) struct CancelledReservations {
    pub jobs: usize,
    pub callbacks: usize,
}
#[derive(Clone, Copy)]
struct Job {
    id: i32,
    owner: BehaviorId,
    instance: ScriptInstanceId,
    change: Change,
    completed: Option<bool>,
    refusal: Option<ZoneRoutingRefusal>,
    completion: Option<ReservedCallback>,
}
#[derive(Clone, Copy)]
pub(crate) struct Reservation {
    owner: BehaviorId,
    instance: ScriptInstanceId,
    change: Change,
}
#[derive(Clone, Copy)]
pub(crate) struct Completion {
    pub id: i32,
    pub instance: ScriptInstanceId,
    pub success: bool,
    pub refusal: Option<ZoneRoutingRefusal>,
    pub completion: Option<ReservedCallback>,
}
// Tagged references keep every index bounded by its configured slot count.
// A compressed binary radix tree has at most N-1 branch nodes for N leaves.
const LEAF: u32 = 1 << 31;
type IndexKey = [u64; 3];
fn id_key(id: i32) -> IndexKey {
    [0, 0, id as u64]
}
fn owner_key(owner: BehaviorId) -> IndexKey {
    [owner.0.runtime, owner.0.index as u64, owner.0.generation]
}
fn key_bit(key: IndexKey, bit: u16) -> usize {
    ((key[usize::from(bit) / 64] >> (63 - bit % 64)) & 1) as usize
}
#[derive(Clone, Copy)]
#[repr(C)]
struct IndexBranch {
    children: [u32; 2],
    bit: u16,
}
struct SlotIndex {
    branches: Box<[IndexBranch]>,
    root: u32,
    free: u32,
    #[cfg(test)]
    steps: std::sync::atomic::AtomicUsize,
}
fn exact_vec<T>(capacity: usize) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::Capacity)?;
    if values.capacity() != capacity {
        return Err(Error::Capacity);
    }
    Ok(values)
}
impl SlotIndex {
    fn new(capacity: usize) -> Result<Self, Error> {
        let count = capacity.saturating_sub(1);
        let mut branches = exact_vec(count)?;
        branches.extend((0..count).map(|i| IndexBranch {
            children: [
                if i + 1 < count {
                    (i + 1) as u32
                } else {
                    ABSENT
                },
                ABSENT,
            ],
            bit: 0,
        }));
        Ok(Self {
            branches: branches.into_boxed_slice(),
            root: ABSENT,
            free: if count == 0 { ABSENT } else { 0 },
            #[cfg(test)]
            steps: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    #[inline]
    fn step(&self) {
        #[cfg(test)]
        self.steps
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn leaf(&self, key: IndexKey) -> u32 {
        let mut node = self.root;
        while node != ABSENT && node & LEAF == 0 {
            self.step();
            let branch = self.branches[node as usize];
            node = branch.children[key_bit(key, branch.bit)];
        }
        self.step();
        if node == ABSENT { ABSENT } else { node & !LEAF }
    }
    fn find(&self, key: IndexKey, key_of: impl Fn(u32) -> IndexKey) -> u32 {
        let slot = self.leaf(key);
        if slot != ABSENT && key_of(slot) == key {
            slot
        } else {
            ABSENT
        }
    }
    fn insert(&mut self, key: IndexKey, slot: u32, key_of: impl Fn(u32) -> IndexKey) {
        let old = self.leaf(key);
        if old == ABSENT {
            self.root = slot | LEAF;
            return;
        }
        let other = key_of(old);
        let word = (0..3)
            .find(|&i| key[i] != other[i])
            .expect("unique index key");
        let bit = (word * 64 + (key[word] ^ other[word]).leading_zeros() as usize) as u16;
        let mut node = self.root;
        let mut parent = ABSENT;
        let mut direction = 0;
        while node & LEAF == 0 && self.branches[node as usize].bit < bit {
            self.step();
            parent = node;
            let branch = self.branches[node as usize];
            direction = key_bit(key, branch.bit);
            node = branch.children[direction];
        }
        let branch = self.free;
        assert_ne!(branch, ABSENT, "one branch per additional indexed leaf");
        self.free = self.branches[branch as usize].children[0];
        let side = key_bit(key, bit);
        let mut children = [node; 2];
        children[side] = slot | LEAF;
        self.branches[branch as usize] = IndexBranch { children, bit };
        if parent == ABSENT {
            self.root = branch;
        } else {
            self.branches[parent as usize].children[direction] = branch;
        }
    }
    fn remove(&mut self, key: IndexKey, key_of: impl Fn(u32) -> IndexKey) -> u32 {
        let mut node = self.root;
        let mut parent = ABSENT;
        let mut grandparent = ABSENT;
        let mut direction = 0;
        let mut parent_direction = 0;
        while node != ABSENT && node & LEAF == 0 {
            self.step();
            grandparent = parent;
            parent = node;
            parent_direction = direction;
            let branch = self.branches[node as usize];
            direction = key_bit(key, branch.bit);
            node = branch.children[direction];
        }
        self.step();
        if node == ABSENT || key_of(node & !LEAF) != key {
            return ABSENT;
        }
        if parent == ABSENT {
            self.root = ABSENT;
        } else {
            let sibling = self.branches[parent as usize].children[direction ^ 1];
            if grandparent == ABSENT {
                self.root = sibling;
            } else {
                self.branches[grandparent as usize].children[parent_direction] = sibling;
            }
            self.branches[parent as usize].children[0] = self.free;
            self.free = parent;
        }
        node & !LEAF
    }
}
#[derive(Clone, Copy)]
struct JobSlot {
    job: Option<Job>,
    // The pending head holds its owner's tail. Once ready, neither link is live.
    // A vacant slot uses next_pending as its free-list link.
    next_pending: u32,
    pending_tail: u32,
    ready_position: u32,
}
struct Jobs {
    slots: Box<[JobSlot]>,
    by_id: SlotIndex,
    by_owner: SlotIndex,
    ready: Vec<u32>,
    free: u32,
    len: usize,
    #[cfg(test)]
    heap_steps: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    pending_visits: std::sync::atomic::AtomicUsize,
}
impl Jobs {
    fn storage_bytes(capacity: usize) -> Result<usize, Error> {
        add(
            mul(capacity, add(size_of::<JobSlot>(), size_of::<u32>())?)?,
            mul(capacity.saturating_sub(1), 2 * size_of::<IndexBranch>())?,
        )
    }
    fn new(capacity: usize) -> Result<Self, Error> {
        debug_assert!(capacity <= MAX_ZONE_BATCH_JOBS);
        let mut slots = exact_vec(capacity)?;
        slots.extend((0..capacity).map(|i| JobSlot {
            job: None,
            next_pending: if i + 1 < capacity {
                (i + 1) as u32
            } else {
                ABSENT
            },
            pending_tail: ABSENT,
            ready_position: ABSENT,
        }));
        Ok(Self {
            slots: slots.into_boxed_slice(),
            by_id: SlotIndex::new(capacity)?,
            by_owner: SlotIndex::new(capacity)?,
            ready: exact_vec(capacity)?,
            free: if capacity == 0 { ABSENT } else { 0 },
            len: 0,
            #[cfg(test)]
            heap_steps: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            pending_visits: std::sync::atomic::AtomicUsize::new(0),
        })
    }
    fn len(&self) -> usize {
        self.len
    }
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn capacity(&self) -> usize {
        self.slots.len()
    }
    fn job(&self, slot: u32) -> &Job {
        self.slots[slot as usize].job.as_ref().unwrap()
    }
    fn find(&self, id: i32) -> u32 {
        if id <= 0 {
            return ABSENT;
        }
        self.by_id
            .find(id_key(id), |slot| id_key(self.job(slot).id))
    }
    fn pending(&self, owner: BehaviorId) -> u32 {
        self.by_owner
            .find(owner_key(owner), |slot| owner_key(self.job(slot).owner))
    }
    fn take_pending(&mut self, owner: BehaviorId) -> u32 {
        let slots = &self.slots;
        self.by_owner.remove(owner_key(owner), |slot| {
            owner_key(slots[slot as usize].job.unwrap().owner)
        })
    }
    #[inline]
    fn visit_pending(&self) {
        #[cfg(test)]
        self.pending_visits
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn push(&mut self, job: Job) {
        let slot = self.free;
        assert_ne!(slot, ABSENT);
        let head = self.pending(job.owner);
        self.free = self.slots[slot as usize].next_pending;
        self.slots[slot as usize] = JobSlot {
            job: Some(job),
            next_pending: ABSENT,
            pending_tail: slot,
            ready_position: ABSENT,
        };
        let slots = &self.slots;
        self.by_id.insert(id_key(job.id), slot, |i| {
            id_key(slots[i as usize].job.unwrap().id)
        });
        if head == ABSENT {
            self.by_owner.insert(owner_key(job.owner), slot, |i| {
                owner_key(slots[i as usize].job.unwrap().owner)
            });
        } else {
            let tail = self.slots[head as usize].pending_tail;
            self.slots[tail as usize].next_pending = slot;
            self.slots[head as usize].pending_tail = slot;
        }
        self.len += 1;
    }
    fn earlier(&self, a: u32, b: u32) -> bool {
        #[cfg(test)]
        self.heap_steps
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The generation's shared async allocator is strictly monotonic; its
        // IDs are also acceptance order, including gaps for non-routing jobs.
        self.job(a).id < self.job(b).id
    }
    fn heap_swap(&mut self, a: usize, b: usize) {
        self.ready.swap(a, b);
        self.slots[self.ready[a] as usize].ready_position = a as u32;
        self.slots[self.ready[b] as usize].ready_position = b as u32;
    }
    fn sift_up(&mut self, mut position: usize) {
        while position > 0 {
            let parent = (position - 1) / 2;
            if !self.earlier(self.ready[position], self.ready[parent]) {
                break;
            }
            self.heap_swap(position, parent);
            position = parent;
        }
    }
    fn mark_ready(&mut self, slot: u32, success: bool, refusal: Option<ZoneRoutingRefusal>) {
        let cell = &mut self.slots[slot as usize];
        let job = cell.job.as_mut().unwrap();
        assert!(job.completed.is_none());
        job.completed = Some(success);
        job.refusal = refusal;
        cell.ready_position = self.ready.len() as u32;
        self.ready.push(slot); // <= live slots, exact capacity reserved off audio
        self.sift_up(self.ready.len() - 1);
    }
    fn remove_ready(&mut self, slot: u32) {
        let position = self.slots[slot as usize].ready_position as usize;
        let last = self.ready.pop().unwrap();
        if position < self.ready.len() {
            self.ready[position] = last;
            self.slots[last as usize].ready_position = position as u32;
            if position > 0 && self.earlier(last, self.ready[(position - 1) / 2]) {
                self.sift_up(position);
            } else {
                let mut at = position;
                while 2 * at + 1 < self.ready.len() {
                    let mut child = 2 * at + 1;
                    if child + 1 < self.ready.len()
                        && self.earlier(self.ready[child + 1], self.ready[child])
                    {
                        child += 1;
                    }
                    if !self.earlier(self.ready[child], self.ready[at]) {
                        break;
                    }
                    self.heap_swap(at, child);
                    at = child;
                }
            }
        }
        self.slots[slot as usize].ready_position = ABSENT;
    }
    fn release_slot(&mut self, slot: u32) {
        self.slots[slot as usize] = JobSlot {
            job: None,
            next_pending: self.free,
            pending_tail: ABSENT,
            ready_position: ABSENT,
        };
        self.free = slot;
        self.len -= 1;
    }
    fn delivered(&mut self, id: i32) -> Result<(), Error> {
        // Runtime acknowledges the polled heap head. Preserve arbitrary-ready
        // acknowledgement as well, using the bounded ID index only as fallback.
        let slot = self
            .ready
            .first()
            .copied()
            .filter(|&slot| self.job(slot).id == id)
            .unwrap_or_else(|| self.find(id));
        if slot == ABSENT || self.job(slot).completed.is_none() {
            return Err(Error::InvalidInput);
        }
        self.remove_ready(slot);
        let slots = &self.slots;
        let removed = self
            .by_id
            .remove(id_key(id), |i| id_key(slots[i as usize].job.unwrap().id));
        debug_assert_eq!(removed, slot);
        self.release_slot(slot);
        Ok(())
    }
    fn cancel_all(&mut self) -> CancelledReservations {
        let mut cancelled = CancelledReservations {
            jobs: self.len,
            callbacks: 0,
        };
        // Visit live jobs only, returning each branch/slot to its existing free
        // list. No capacity-sized reset, allocation, free or stale owner leaf.
        while !self.is_empty() {
            let slot = self.by_id.leaf([0; 3]);
            let job = *self.job(slot);
            if job.completed.is_none() {
                self.take_pending(job.owner);
            }
            cancelled.callbacks += usize::from(job.completion.is_some());
            let slots = &self.slots;
            self.by_id.remove(id_key(job.id), |i| {
                id_key(slots[i as usize].job.unwrap().id)
            });
            self.release_slot(slot);
        }
        self.ready.clear(); // Copy indices, no destructor or capacity change
        debug_assert_eq!(self.by_owner.root, ABSENT);
        cancelled
    }
}
pub(crate) struct State {
    homes: Box<[Home]>,
    words: Box<[u64]>,
    jobs: Jobs,
    scratch: Box<[Home]>,
    dirty: Box<[bool]>,
    valid: Box<[bool]>,
    touched: Vec<usize>,
    refusals: [u64; 8],
    last_refusal: Option<ZoneRoutingRefusal>,
}
fn add(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_add(b).ok_or(Error::Capacity)
}
fn mul(a: usize, b: usize) -> Result<usize, Error> {
    a.checked_mul(b).ok_or(Error::Capacity)
}

/// Shared by both callback allocators and all fanout transaction preflights.
/// With no reserved headroom, arithmetic/ordering is the existing behavior.
pub(crate) fn callback_id_preflight(
    last: i32,
    reserved: usize,
    needed: usize,
) -> Result<(), Error> {
    let count = reserved
        .checked_add(needed)
        .and_then(|n| i32::try_from(n).ok())
        .ok_or(Error::Capacity)?;
    last.checked_add(count).ok_or(Error::Capacity).map(|_| ())
}
pub(crate) fn allocate_callback_id(last: &mut i32, reserved: usize) -> Result<i32, Error> {
    callback_id_preflight(*last, reserved, 1)?;
    *last += 1;
    Ok(*last)
}

impl PreparedRouting {
    /// Worker only. Candidate identities MUST describe the final, sorted bank.
    /// Each routable variant has candidates only at keys admitted by ordinary source-step validation.
    pub fn new(
        mode: u8,
        mut routes: Vec<ZoneRouting>,
        candidates: &[CandidateIdentity],
        regions: usize,
        catalog: &SourceZoneCatalog,
        limits: ZoneRoutingLimits,
    ) -> Result<Self, Error> {
        if !matches!(mode, 2 | 3) {
            return Err(Error::InvalidInput);
        }
        if limits.jobs > MAX_ZONE_BATCH_JOBS {
            return Err(Error::Capacity);
        }
        let n = routes
            .iter()
            .try_fold(0usize, |n, r| add(n, r.variants.len()))?;
        if routes.len() > limits.zones
            || n > limits.variants
            || candidates.len() > limits.candidates
            || candidates.len() >= ABSENT as usize
            || routes.len() >= ABSENT as usize
            || n >= ABSENT as usize
        {
            return Err(Error::Capacity);
        }
        let words = candidates.len().div_ceil(64);
        let bytes = [
            mul(routes.len(), size_of::<Zone>())?,
            mul(n, size_of::<Variant>())?,
            mul(regions, size_of::<u32>())?,
            mul(words, size_of::<u64>())?,
            mul(routes.len(), size_of::<Home>())?,
            mul(words, size_of::<u64>())?,
            Jobs::storage_bytes(limits.jobs)?,
            mul(routes.len(), size_of::<Home>())?,
            mul(routes.len(), 2 * size_of::<bool>())?,
            mul(routes.len().min(limits.jobs), size_of::<usize>())?,
            size_of::<Self>(),
            size_of::<State>(),
        ]
        .into_iter()
        .try_fold(0, add)?;
        if bytes > limits.bytes {
            return Err(Error::Capacity);
        }
        routes.sort_by_key(|r| r.source_zone);
        if routes
            .iter()
            .any(|r| r.low_key > r.high_key || r.high_key > 127)
            || routes
                .windows(2)
                .any(|r| r[0].source_zone == r[1].source_zone)
        {
            return Err(Error::InvalidInput);
        }
        let mut zones = Vec::with_capacity(routes.len());
        let mut variants = Vec::with_capacity(n);
        let mut region_zones = vec![ABSENT; regions];
        let mut region_variants = vec![ABSENT; regions]; // worker-only reverse index
        for (zone, mut route) in routes.into_iter().enumerate() {
            route.variants.sort_by_key(|v| v.group);
            if route.variants.is_empty()
                || route.variants.iter().any(|v| v.group > i32::MAX as u32)
                || route.variants.windows(2).any(|v| v[0].group == v[1].group)
            {
                return Err(Error::InvalidInput);
            }
            let start = variants.len();
            let initial = route
                .variants
                .binary_search_by_key(&route.group, |v| v.group)
                .map_err(|_| Error::InvalidInput)?
                + start;
            let source = catalog.find(route.source_zone).ok_or(Error::InvalidInput)?;
            if source.home
                != (SourceZoneHome {
                    group: route.group,
                    low_key: route.low_key,
                    high_key: route.high_key,
                })
                || route.variants[initial - start].region != source.region
            {
                return Err(Error::InvalidInput);
            }
            for variant in route.variants {
                if let Some(region) = variant.region {
                    let rz = region_zones.get_mut(region).ok_or(Error::InvalidInput)?;
                    if *rz != ABSENT {
                        return Err(Error::InvalidInput);
                    }
                    *rz = zone as u32;
                    region_variants[region] = variants.len() as u32;
                }
                variants.push(Variant {
                    group: variant.group,
                    region: variant.region,
                    candidates: [ABSENT; 128],
                });
            }
            zones.push(Zone {
                source: route.source_zone,
                variants: start..variants.len(),
                initial: Home {
                    variant: initial,
                    low: route.low_key,
                    high: route.high_key,
                },
                initial_keys: [0; 2],
            });
        }
        let mut initial_words = vec![u64::MAX; words];
        for (index, c) in candidates.iter().enumerate() {
            if c.key > 127 {
                return Err(Error::InvalidInput);
            }
            let variant = *region_variants.get(c.region).ok_or(Error::InvalidInput)?;
            if variant != ABSENT {
                let cell = &mut variants[variant as usize].candidates[usize::from(c.key)];
                if *cell != ABSENT {
                    return Err(Error::InvalidInput);
                }
                *cell = index as u32;
                let zone = region_zones[c.region] as usize;
                if c.initial && zones[zone].initial.variant == variant as usize {
                    zones[zone].initial_keys[usize::from(c.key) / 64] |= 1u64 << (c.key % 64);
                }
                initial_words[index / 64] &= !(1u64 << (index % 64));
            }
        }
        if variants
            .iter()
            .any(|v| v.region.is_some() && v.candidates.iter().all(|&c| c == ABSENT))
        {
            return Err(Error::InvalidInput);
        }
        let mut partial_initial_homes = 0;
        let mut partial_initial_keys = 0;
        let catalog_only_initial_homes = catalog
            .rows
            .iter()
            .filter(|row| row.region.is_none())
            .count();
        for z in &zones {
            if variants[z.initial.variant].region.is_none() {
                // Only the source-bound roster can authorize this catalog-only
                // original. No playable default or fake candidate is inserted.
                continue;
            }
            let mut missing = 0;
            for key in z.initial.low..=z.initial.high {
                let index = variants[z.initial.variant].candidates[usize::from(key)];
                if index == ABSENT
                    || z.initial_keys[usize::from(key) / 64] & (1u64 << (key % 64)) == 0
                {
                    missing += 1;
                    continue;
                }
                let index = index as usize;
                initial_words[index / 64] |= 1u64 << (index % 64);
            }
            partial_initial_homes += usize::from(missing != 0);
            partial_initial_keys += missing;
        }
        let unavailable_destinations = variants.iter().filter(|v| v.region.is_none()).count();
        Ok(Self {
            zones: zones.into_boxed_slice(),
            variants: variants.into_boxed_slice(),
            region_zones: region_zones.into_boxed_slice(),
            initial_words: initial_words.into_boxed_slice(),
            jobs: limits.jobs,
            bytes,
            mode,
            partial_initial_homes,
            partial_initial_keys,
            unavailable_destinations,
            catalog_only_initial_homes,
        })
    }
    fn zone(&self, source: u32) -> Result<usize, Error> {
        self.zones
            .binary_search_by_key(&source, |z| z.source)
            .map_err(|_| Error::InvalidInput)
    }
    fn changed(
        &self,
        zone: usize,
        mut home: Home,
        parameter: ZoneParameter,
        value: i32,
    ) -> Result<Home, Error> {
        match parameter {
            ZoneParameter::Group => {
                let group = u32::try_from(value).map_err(|_| Error::InvalidInput)?;
                let range = self.zones[zone].variants.clone();
                home.variant = self.variants[range.clone()]
                    .binary_search_by_key(&group, |v| v.group)
                    .map_err(|_| Error::InvalidInput)?
                    + range.start;
                if self.variants[home.variant].region.is_none() {
                    return Err(Error::InvalidInput);
                }
            }
            ZoneParameter::LowKey => {
                home.low = u8::try_from(value).map_err(|_| Error::InvalidInput)?
            }
            ZoneParameter::HighKey => {
                home.high = u8::try_from(value).map_err(|_| Error::InvalidInput)?
            }
        }
        // Key fields have independent domains. Transient wide/reversed ranges
        // may be repaired later in this callback's native batch.
        if home.low > 127 || home.high > 127 {
            return Err(Error::InvalidInput);
        }
        Ok(home)
    }
    fn supports(&self, zone: usize, home: Home) -> bool {
        // Exact restoration/no-op retains the pre-existing admitted domain and
        // its separately reported pitch-limit narrowing; no fake key cells.
        self.variants[home.variant].region.is_some()
            && (home == self.zones[zone].initial
                || home.low <= home.high
                    && (home.low..=home.high).all(|key| {
                        self.variants[home.variant].candidates[usize::from(key)] != ABSENT
                    }))
    }
    pub fn state(&self) -> Result<State, Error> {
        let jobs = Jobs::new(self.jobs)?;
        let homes: Box<[Home]> = self.zones.iter().map(|z| z.initial).collect();
        let mut touched = Vec::new();
        let capacity = self.jobs.min(homes.len());
        touched
            .try_reserve_exact(capacity)
            .map_err(|_| Error::Capacity)?;
        if touched.capacity() != capacity {
            return Err(Error::Capacity);
        }
        Ok(State {
            scratch: homes.clone(),
            dirty: vec![false; homes.len()].into_boxed_slice(),
            valid: vec![false; homes.len()].into_boxed_slice(),
            homes,
            words: self.initial_words.clone(),
            jobs,
            touched,
            refusals: [0; 8],
            last_refusal: None,
        })
    }
}
impl State {
    pub fn get(
        &self,
        prepared: &PreparedRouting,
        source: u32,
        parameter: ZoneParameter,
    ) -> Result<i32, Error> {
        let h = self.homes[prepared.zone(source)?];
        Ok(match parameter {
            ZoneParameter::Group => i32::try_from(prepared.variants[h.variant].group)
                .map_err(|_| Error::InvalidInput)?,
            ZoneParameter::LowKey => i32::from(h.low),
            ZoneParameter::HighKey => i32::from(h.high),
        })
    }
    /// Skip dormant templates without a linear physical-zone or variant scan.
    pub fn next_eligible(&self, range: Range<usize>) -> Option<usize> {
        let mut first = range.start;
        while first < range.end {
            let index = first / 64;
            let bits = self.words[index] & (u64::MAX << (first % 64));
            if bits != 0 {
                let found = index * 64 + bits.trailing_zeros() as usize;
                return (found < range.end).then_some(found);
            }
            first = (index + 1) * 64;
        }
        None
    }
    pub fn bounds_for_region(&self, prepared: &PreparedRouting, region: usize) -> Option<(u8, u8)> {
        let zone = *prepared.region_zones.get(region)?;
        if zone == ABSENT {
            return None;
        }
        let h = self.homes[zone as usize];
        (prepared.variants[h.variant].region == Some(region)).then_some((h.low, h.high))
    }
    fn publish_home(&mut self, p: &PreparedRouting, zone: usize, next: Home) {
        let old = self.homes[zone];
        for key in old.low..=old.high {
            let bit = p.variants[old.variant].candidates[usize::from(key)];
            if bit == ABSENT {
                continue;
            }
            let bit = bit as usize;
            self.words[bit / 64] &= !(1u64 << (bit % 64));
        }
        for key in next.low..=next.high {
            let bit = p.variants[next.variant].candidates[usize::from(key)];
            if bit == ABSENT
                || next == p.zones[zone].initial
                    && p.zones[zone].initial_keys[usize::from(key) / 64] & (1u64 << (key % 64)) == 0
            {
                continue;
            }
            let bit = bit as usize;
            self.words[bit / 64] |= 1u64 << (bit % 64);
        }
        self.homes[zone] = next;
    }
    /// No async ID is consumed until this preflight has succeeded. The caller
    /// then reserves generation callback capacity and the shared monotonic ID.
    fn refuse(&mut self, reason: ZoneRoutingRefusal) -> Error {
        self.refusals[reason as usize] = self.refusals[reason as usize].saturating_add(1);
        self.last_refusal = Some(reason);
        if reason == ZoneRoutingRefusal::Capacity {
            Error::Capacity
        } else {
            Error::InvalidInput
        }
    }
    pub fn reserve(
        &mut self,
        p: &PreparedRouting,
        owner: BehaviorId,
        instance: ScriptInstanceId,
        source: u32,
        parameter: ZoneParameter,
        value: i32,
    ) -> Result<Reservation, Error> {
        if self.jobs.len() == self.jobs.capacity() {
            return Err(self.refuse(ZoneRoutingRefusal::Capacity));
        }
        let zone = match p.zone(source) {
            Ok(z) => z,
            Err(_) => return Err(self.refuse(ZoneRoutingRefusal::UnknownZone)),
        };
        if !matches!(p.mode, 2 | 3) {
            return Err(self.refuse(ZoneRoutingRefusal::Policy));
        }
        if p.variants[p.zones[zone].initial.variant].region.is_none() {
            return Err(self.refuse(ZoneRoutingRefusal::UnavailableTemplate));
        }
        if parameter == ZoneParameter::Group {
            let group = match u32::try_from(value) {
                Ok(g) => g,
                Err(_) => return Err(self.refuse(ZoneRoutingRefusal::UnknownGroup)),
            };
            let range = p.zones[zone].variants.clone();
            let selected = match p.variants[range.clone()].binary_search_by_key(&group, |v| v.group)
            {
                Ok(i) => i + range.start,
                Err(_) => return Err(self.refuse(ZoneRoutingRefusal::UnknownGroup)),
            };
            if p.variants[selected].region.is_none() {
                return Err(self.refuse(ZoneRoutingRefusal::UnavailableTemplate));
            }
        }
        if p.changed(zone, self.homes[zone], parameter, value).is_err() {
            return Err(self.refuse(ZoneRoutingRefusal::InvalidKey));
        }
        Ok(Reservation {
            owner,
            instance,
            change: Change {
                zone,
                parameter,
                value,
            },
        })
    }
    pub fn report(&self, p: &PreparedRouting) -> ZoneRoutingReport {
        ZoneRoutingReport {
            zones: p.zones.len(),
            templates: p.variants.len(),
            metadata_bytes: p.bytes,
            partial_initial_homes: p.partial_initial_homes,
            partial_initial_keys: p.partial_initial_keys,
            unavailable_destinations: p.unavailable_destinations,
            catalog_only_initial_homes: p.catalog_only_initial_homes,
            job_capacity: p.jobs,
            pending_jobs: self.jobs.len(),
            refusals: self.refusals,
            last_refusal: self.last_refusal,
        }
    }
    /// Infallible after same-audio-writer reservation and ID/callback preflight.
    /// IDs come from this generation's strictly monotonic shared async allocator.
    pub fn enqueue_reserved(
        &mut self,
        r: Reservation,
        id: i32,
        completion: Option<ReservedCallback>,
    ) {
        assert!(id > 0 && self.jobs.len() < self.jobs.capacity());
        self.jobs.push(Job {
            id,
            owner: r.owner,
            instance: r.instance,
            change: r.change,
            completed: None,
            refusal: None,
            completion,
        });
    }
    /// Semantic first wait/end, not block end or an explicit note forwarding.
    /// O(owner_jobs * log(capacity) + touched_zones * 128), fixed storage.
    /// Full-handle owner lookup is bounded by 192 radix bits; no unrelated jobs
    /// are visited. Coalesces a zone's final home without discarding any async ID. Unsupported final geometry leaves
    /// that entire zone unchanged and completes its affected jobs as false;
    /// this is an explicit representation failure policy, not a native oracle.
    pub fn flush(&mut self, p: &PreparedRouting, owner: BehaviorId) {
        debug_assert!(self.touched.is_empty());
        let first = self.jobs.pending(owner);
        let mut slot = first;
        while slot != ABSENT {
            self.jobs.visit_pending();
            let j = self.jobs.job(slot);
            let zone = j.change.zone;
            if !self.dirty[zone] {
                self.dirty[zone] = true;
                self.valid[zone] = true;
                self.scratch[zone] = self.homes[zone];
                self.touched.push(zone); // <= unique pending jobs, reserved off audio
            }
            match p.changed(zone, self.scratch[zone], j.change.parameter, j.change.value) {
                Ok(next) => self.scratch[zone] = next,
                Err(_) => self.valid[zone] = false,
            }
            slot = self.jobs.slots[slot as usize].next_pending;
        }
        for i in 0..self.touched.len() {
            let zone = self.touched[i];
            self.valid[zone] &= p.supports(zone, self.scratch[zone]);
            if self.valid[zone] {
                self.publish_home(p, zone, self.scratch[zone]);
            } else {
                self.refuse(ZoneRoutingRefusal::UnsupportedKeyRange);
            }
        }
        let mut slot = self.jobs.take_pending(owner);
        debug_assert_eq!(slot, first);
        while slot != ABSENT {
            self.jobs.visit_pending();
            let next = self.jobs.slots[slot as usize].next_pending;
            let success = self.valid[self.jobs.job(slot).change.zone];
            self.jobs.mark_ready(
                slot,
                success,
                (!success).then_some(ZoneRoutingRefusal::UnsupportedKeyRange),
            );
            slot = next;
        }
        for &zone in &self.touched {
            self.dirty[zone] = false;
        }
        self.touched.clear(); // usize elements, no destructor or capacity change
    }
    pub fn cancel_owner(&mut self, owner: BehaviorId) {
        let mut slot = self.jobs.take_pending(owner);
        while slot != ABSENT {
            self.jobs.visit_pending();
            let next = self.jobs.slots[slot as usize].next_pending;
            self.jobs
                .mark_ready(slot, false, Some(ZoneRoutingRefusal::Cancelled));
            slot = next;
        }
    }
    /// Panic is consistent with the existing MIDI/array cancellation policy:
    /// abandon outstanding async work, do not spawn new authored callbacks.
    /// The caller releases these exact arena/headroom reservations and pins.
    pub fn cancel_all(&mut self) -> CancelledReservations {
        debug_assert!(self.touched.is_empty());
        self.jobs.cancel_all()
    }
    pub fn has_job(&self, id: i32) -> bool {
        self.jobs.find(id) != ABSENT
    }
    pub fn waiting(&self, id: i32) -> bool {
        let slot = self.jobs.find(id);
        slot != ABSENT && self.jobs.job(slot).completed.is_none()
    }
    pub fn completion(&self) -> Option<Completion> {
        // O(1) polling, including a full queue with no ready jobs. Heap order
        // is accepted async-ID order, not flush order or physical slot order.
        let j = self.jobs.job(*self.jobs.ready.first()?);
        Some(Completion {
            id: j.id,
            instance: j.instance,
            success: j.completed?,
            refusal: j.refusal,
            completion: j.completion,
        })
    }
    /// Invoke ONLY after async callback/continuation admission succeeds. Until
    /// then the entry and its generation pin remain, with no completion loss.
    /// At most two 31-bit radix walks plus a bounded ready-heap repair; no job
    /// scans/shifts. Arbitrary already-ready IDs retain the prior API behavior.
    pub fn delivered(&mut self, id: i32) -> Result<(), Error> {
        self.jobs.delivered(id)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRoutingFeature {
    DynamicSampleUpdates,
    NonAttackTemplate,
    AbsolutePitch,
    SourceLayout,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRoutingBuildError {
    InvalidInput,
    Capacity,
    Unsupported(ZoneRoutingFeature),
}
impl From<Error> for ZoneRoutingBuildError {
    fn from(error: Error) -> Self {
        if error == Error::Capacity {
            Self::Capacity
        } else {
            Self::InvalidInput
        }
    }
}

// Child of prepare.rs to share the ordinary immutable Candidate/region law.
impl super::Prepared {
    /// Install native zone-command identities and authored homes in physical order.
    /// Metadata-only rows remain readable without an admitted playable region.
    /// This catalog is independent of the event IDs installed by `with_source_zones`.
    pub fn with_source_zone_identities(
        mut self,
        rows: Vec<SourceZoneIdentity>,
        max_zones: usize,
        max_bytes: usize,
    ) -> Result<Self, Error> {
        if self.source_zone_catalog.is_some() || self.zone_routing.is_some() {
            return Err(Error::InvalidInput);
        }
        let catalog = SourceZoneCatalog::new(rows, self.regions.len(), max_zones, max_bytes)?;
        for row in &catalog.rows {
            let Some(region) = row.region else {
                continue;
            };
            let dense = self
                .region_groups
                .get(region)
                .copied()
                .flatten()
                .ok_or(Error::InvalidInput)?;
            let physical = self
                .group_source_ids
                .get(dense as usize)
                .copied()
                .unwrap_or(dense);
            if physical != row.home.group {
                return Err(Error::InvalidInput);
            }
        }
        self.source_zone_catalog = Some(catalog);
        Ok(self)
    }
    /// Retained source jobs, independent of runtime callback arena availability.
    pub fn zone_routing_job_capacity(&self) -> usize {
        self.zone_routing.as_ref().map_or(0, |routing| routing.jobs)
    }
    /// Worst-case exact-instance completion reservations for this routing bank.
    /// Instances without an async callback need no arena or numeric headroom.
    pub(crate) fn zone_completion_capacity(&self) -> usize {
        if self.signal_programs.iter().any(|entry| {
            entry.signal == crate::MIDI_ASYNC_SIGNAL
                && self.programs[entry.program].script_instance.is_some()
        }) {
            self.zone_routing_job_capacity()
        } else {
            0
        }
    }
    pub fn source_zone_identity_count(&self) -> usize {
        self.source_zone_catalog
            .as_ref()
            .map_or(0, SourceZoneCatalog::len)
    }
    pub fn source_zone_identity(&self, index: usize) -> Option<u32> {
        self.source_zone_catalog.as_ref()?.identity(index)
    }
    pub fn source_zone_identity_home(&self, id: u32) -> Option<SourceZoneHome> {
        Some(self.source_zone_catalog.as_ref()?.find(id)?.home)
    }
    /// Native command ID -> immutable original source; unlike legacy EventInfo
    /// lookup, ID0 is valid. A catalog-only source is explicitly unavailable.
    pub fn source_zone_identity_region(&self, id: u32) -> Option<usize> {
        self.source_zone_catalog.as_ref()?.find(id)?.region
    }

    pub fn with_zone_routing(
        mut self,
        mode: u8,
        routes: Vec<ZoneRouting>,
        limits: ZoneRoutingLimits,
        tuning: &super::Tuning,
    ) -> Result<Self, ZoneRoutingBuildError> {
        if self.zone_routing.is_some() || routes.is_empty() || !matches!(mode, 2 | 3) {
            return Err(ZoneRoutingBuildError::InvalidInput);
        }
        if self.sample_limits.is_some() {
            return Err(ZoneRoutingBuildError::Unsupported(
                ZoneRoutingFeature::DynamicSampleUpdates,
            ));
        }
        if routes
            .iter()
            .flat_map(|route| &route.variants)
            .filter_map(|variant| variant.region)
            .any(|region| {
                self.region_source_storage(region)
                    .is_some_and(|s| s.stereo().is_none())
            })
        {
            return Err(ZoneRoutingBuildError::Unsupported(
                ZoneRoutingFeature::SourceLayout,
            ));
        }
        let variant_count = routes
            .iter()
            .try_fold(0usize, |n, r| add(n, r.variants.len()))?;
        if routes.len() > limits.zones
            || variant_count > limits.variants
            || limits.jobs > MAX_ZONE_BATCH_JOBS
        {
            return Err(ZoneRoutingBuildError::Capacity);
        }
        let mut routed = vec![false; self.regions.len()];
        for route in &routes {
            for variant in &route.variants {
                let Some(region_index) = variant.region else {
                    continue;
                };
                let Some(region) = self.regions.get(region_index) else {
                    return Err(ZoneRoutingBuildError::InvalidInput);
                };
                if region.trigger != crate::Trigger::Attack {
                    return Err(ZoneRoutingBuildError::Unsupported(
                        ZoneRoutingFeature::NonAttackTemplate,
                    ));
                }
                let dense = self
                    .region_groups
                    .get(region_index)
                    .copied()
                    .flatten()
                    .ok_or(ZoneRoutingBuildError::InvalidInput)?;
                let physical = self
                    .group_source_ids
                    .get(dense as usize)
                    .copied()
                    .unwrap_or(dense);
                if physical != variant.group || std::mem::replace(&mut routed[region_index], true) {
                    return Err(ZoneRoutingBuildError::InvalidInput);
                }
                // Routing IDs are explicit source-defined KSP identities,
                // including zero. Do not join through legacy EventInfo IDs or +1.
            }
        }
        let upper = add(
            self.candidates.iter().filter(|c| !routed[c.region]).count(),
            mul(routed.iter().filter(|&&r| r).count(), 128)?,
        )?;
        if upper > limits.candidates {
            return Err(ZoneRoutingBuildError::Capacity);
        }
        // Charge the full rebuilt candidate bank + conditional end index too,
        // in addition to routing metadata/state checked by PreparedRouting::new.
        let extra = add(
            mul(upper, size_of::<super::Candidate>())?,
            if self.conditions.is_empty() {
                0
            } else {
                mul(upper, size_of::<usize>())?
            },
        )?;
        let catalog_bytes = self
            .source_zone_catalog
            .as_ref()
            .ok_or(ZoneRoutingBuildError::InvalidInput)?
            .bytes;
        let extra = add(extra, catalog_bytes)?;
        let remaining = limits
            .bytes
            .checked_sub(extra)
            .ok_or(ZoneRoutingBuildError::Capacity)?;
        let mut candidates = Vec::with_capacity(upper);
        let mut offsets = [0; 129];
        for key in 0..128 {
            offsets[key] = candidates.len();
            candidates.extend(
                self.candidates[self.offsets[key]..self.offsets[key + 1]]
                    .iter()
                    .copied()
                    .filter(|c| !routed[c.region]),
            );
            for (index, region) in self.regions.iter().enumerate().filter(|(i, _)| routed[*i]) {
                let cursor = region.cursor.map(|c| c.cursor(&self.cursor_loops));
                let wave = cursor.and_then(|c| c.wavetable());
                let source_rate = self
                    .source_storage(region.sample)
                    .map_or(self.rate, |source| source.sample_rate());
                let step = if let Some(root) = region.root_key {
                    // SAME root+tuning arithmetic and Playback::step as new_tuned.
                    let mut playback = crate::Playback {
                        wavetable: wave,
                        transpose_semitones: region.transpose_semitones,
                        ..Default::default()
                    };
                    playback.transpose_semitones += key as f64 - f64::from(root) + tuning.0[key];
                    playback.step(source_rate, self.rate)
                } else {
                    cursor.map_or_else(
                        || {
                            crate::Playback {
                                transpose_semitones: region.transpose_semitones,
                                ..Default::default()
                            }
                            .step(source_rate, self.rate)
                        },
                        |c| c.step(),
                    )
                };
                if step.is_finite()
                    && step > 0.
                    && (wave.is_some()
                        || (crate::resample::MIN_STEP..=crate::resample::MAX_STEP).contains(&step))
                {
                    candidates.push(super::Candidate {
                        region: index,
                        step,
                    });
                }
            }
        }
        offsets[128] = candidates.len();
        self.offsets = Box::new(offsets);
        self.candidates = candidates.into_boxed_slice();
        self.compile_selection(); // same trigger/take/articulation/condition order
        let mut identities = Vec::with_capacity(self.candidates.len());
        for key in 0..128 {
            identities.extend(
                self.candidates[self.offsets[key]..self.offsets[key + 1]]
                    .iter()
                    .map(|c| {
                        let r = self.regions[c.region];
                        CandidateIdentity {
                            region: c.region,
                            key: key as u8,
                            initial: usize::from(r.bounds[0]) <= key
                                && key <= usize::from(r.bounds[1]),
                        }
                    }),
            );
        }
        let catalog = self
            .source_zone_catalog
            .as_ref()
            .ok_or(ZoneRoutingBuildError::InvalidInput)?;
        let mut bank = PreparedRouting::new(
            mode,
            routes,
            &identities,
            self.regions.len(),
            catalog,
            ZoneRoutingLimits {
                bytes: remaining,
                ..limits
            },
        )?;
        bank.bytes = add(bank.bytes, extra)?;
        self.zone_routing = Some(bank);
        Ok(self)
    }
}

impl crate::Runtime {
    pub(crate) fn validate_zone_routing_pitch(
        &self,
        plan: crate::PlanId,
        pitch: crate::NotePitch,
    ) -> Result<(), Error> {
        let g = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        if matches!(pitch, crate::NotePitch::Absolute(_)) && g.prepared.zone_routing.is_some() {
            return Err(Error::UnsupportedZoneRoutingPitch);
        }
        Ok(())
    }
    pub fn zone_routing_report(
        &self,
        plan: crate::PlanId,
    ) -> Result<Option<ZoneRoutingReport>, Error> {
        let g = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        Ok(g.prepared
            .zone_routing
            .as_ref()
            .zip(g.zone_routing.as_ref())
            .map(|(p, s)| s.report(p)))
    }
    pub fn zone_parameter_in(
        &self,
        plan: crate::PlanId,
        zone: u32,
        parameter: ZoneParameter,
    ) -> Result<i32, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        match (&generation.prepared.zone_routing, &generation.zone_routing) {
            (Some(prepared), Some(state)) if prepared.zone(zone).is_ok() => {
                return state.get(prepared, zone, parameter);
            }
            (Some(_), None) | (None, Some(_)) => return Err(Error::InvalidInput),
            _ => {}
        }
        let home = generation
            .prepared
            .source_zone_identity_home(zone)
            .ok_or(Error::InvalidInput)?;
        Ok(match parameter {
            ZoneParameter::Group => home.group as i32,
            ZoneParameter::LowKey => i32::from(home.low_key),
            ZoneParameter::HighKey => i32::from(home.high_key),
        })
    }
    pub(crate) fn request_zone_parameter(
        &mut self,
        owner: BehaviorId,
        zone: u32,
        parameter: ZoneParameter,
        value: i32,
    ) -> Result<i32, Error> {
        let callback = *self.behaviors.get(owner.0).ok_or(Error::StaleHandle)?;
        if callback.outcome.is_some() {
            return Err(Error::InvalidInput);
        }
        let plan = self.behavior_plan(callback.owner)?;
        let (instance, reservation) = {
            let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
            let instance = generation.prepared.programs[callback.program]
                .script_instance
                .ok_or(Error::InvalidInput)?;
            let prepared = generation
                .prepared
                .zone_routing
                .as_ref()
                .ok_or(Error::InvalidInput)?;
            let state = generation
                .zone_routing
                .as_mut()
                .ok_or(Error::InvalidInput)?;
            if prepared.zone(zone).is_err()
                && generation
                    .prepared
                    .source_zone_identity_home(zone)
                    .is_some()
            {
                return Err(state.refuse(ZoneRoutingRefusal::UnavailableTemplate));
            }
            let reservation = state.reserve(prepared, owner, instance, zone, parameter, value)?;
            if generation.callbacks.checked_add(1).is_none()
                || generation
                    .midi_object
                    .async_id_high_water()
                    .checked_add(1)
                    .is_none()
            {
                return Err(state.refuse(ZoneRoutingRefusal::Capacity));
            }
            (instance, reservation)
        };
        // All state/ID/property checks above are pure except truthful refusal
        // counters. Callback preflight may reclaim finished owners, but only
        // after an otherwise representable zone job is known to fit.
        let completion = (|| {
            let completion = self
                .preflight_async_callback(plan, instance)?
                .map(|(p, context)| ReservedCallback {
                    program: p.program,
                    context,
                });
            if completion.is_some() {
                callback_id_preflight(self.last_callback_id, self.callback_id_reservations, 1)?;
            }
            Ok(completion)
        })();
        let completion = match completion {
            Ok(completion) => completion,
            Err(Error::Capacity) => {
                let state = self
                    .plans
                    .get_mut(plan.0)
                    .unwrap()
                    .zone_routing
                    .as_mut()
                    .unwrap();
                return Err(state.refuse(ZoneRoutingRefusal::Capacity));
            }
            Err(error) => return Err(error),
        };
        let generation = self.plans.get_mut(plan.0).ok_or(Error::StaleHandle)?;
        let callbacks = generation.callbacks.checked_add(1).ok_or(Error::Capacity)?;
        let id = generation.midi_object.allocate_async_id()?;
        // Single audio writer. No user code/fallible operations after this
        // point: both the exact arena slot and numeric-ID headroom are owned.
        if completion.is_some() {
            self.behaviors.reserve(1);
            self.callback_id_reservations += 1;
        }
        generation
            .zone_routing
            .as_mut()
            .unwrap()
            .enqueue_reserved(reservation, id, completion);
        generation.callbacks = callbacks;
        Ok(id)
    }
    pub(crate) fn flush_zone_parameters(&mut self, owner: BehaviorId) {
        let Some(callback) = self.behaviors.get(owner.0) else {
            return;
        };
        let Ok(plan) = self.behavior_plan(callback.owner) else {
            return;
        };
        let Some(g) = self.plans.get_mut(plan.0) else {
            return;
        };
        if let (Some(p), Some(s)) = (&g.prepared.zone_routing, &mut g.zone_routing) {
            s.flush(p, owner);
        }
    }
    pub(crate) fn cancel_zone_parameters(&mut self, owner: BehaviorId) {
        let Some(callback) = self.behaviors.get(owner.0) else {
            return;
        };
        let Ok(plan) = self.behavior_plan(callback.owner) else {
            return;
        };
        if let Some(state) = self
            .plans
            .get_mut(plan.0)
            .and_then(|g| g.zone_routing.as_mut())
        {
            state.cancel_owner(owner);
        }
    }
    pub(crate) fn has_ready_zone_completions(&self) -> bool {
        self.plans.slots.iter().any(|slot| {
            slot.value
                .as_ref()
                .and_then(|g| g.zone_routing.as_ref())
                .is_some_and(|s| s.completion().is_some())
        })
    }
    /// AFTER settled ready-entry handling. Each accepted job owns a fixed
    /// callback slot and ID headroom, so other admissions cannot steal them.
    /// The job pin is transferred to the callback, not briefly doubled.
    pub(crate) fn service_zone_completions(&mut self) -> Result<bool, Error> {
        for index in 0..self.plans.slots.len() {
            if self.plans.slots[index].value.is_none() {
                continue;
            }
            let plan = crate::PlanId(self.plans.id(index));
            let next = self
                .plans
                .get(plan.0)
                .and_then(|g| g.zone_routing.as_ref())
                .and_then(State::completion);
            let Some(c) = next else {
                continue;
            };
            let admitted = if let Some(callback) = c.completion {
                self.behaviors.unreserve(1);
                self.callback_id_reservations -= 1;
                self.plans.get_mut(plan.0).unwrap().callbacks -= 1;
                match self.admit_plan_context(plan, callback.program, callback.context) {
                    Ok(id) => Some(id),
                    Err(error) => {
                        self.plans.get_mut(plan.0).unwrap().callbacks += 1;
                        self.behaviors.reserve(1);
                        self.callback_id_reservations += 1;
                        // Context/program are immutable and ID headroom is
                        // reserved. Any refusal here is an invariant failure;
                        // preserve the job rather than lose its completion.
                        if error == Error::Capacity {
                            continue;
                        }
                        return Err(error);
                    }
                }
            } else {
                self.plans.get_mut(plan.0).unwrap().callbacks -= 1;
                None
            };
            self.plans
                .get_mut(plan.0)
                .unwrap()
                .zone_routing
                .as_mut()
                .unwrap()
                .delivered(c.id)?;
            // Queue waiters first: the ready stack is LIFO, so the exact
            // completion callback runs before its resumed waiter. No code
            // executes while the caller holds the dispatch guard.
            self.finish_async_callback(plan, c.id, c.success, None)?;
            if let Some(id) = admitted {
                self.behaviors.get_mut(id.0).unwrap().async_result =
                    Some((c.id, i32::from(c.success)));
                self.resume_behavior(id);
            }
            return Ok(true);
        }
        Ok(false)
    }
    /// Invoke after panic's release callbacks have run and their continuations
    /// are marked cancelled, before final generation cancellation/retirement.
    /// Calling before release callbacks could miss new jobs they authored.
    pub(crate) fn abandon_zone_jobs(&mut self) {
        for slot in &mut self.plans.slots {
            let Some(g) = slot.value.as_mut() else {
                continue;
            };
            let Some(state) = g.zone_routing.as_mut() else {
                continue;
            };
            let cancelled = state.cancel_all();
            self.behaviors.unreserve(cancelled.callbacks);
            self.callback_id_reservations -= cancelled.callbacks;
            g.callbacks -= cancelled.jobs;
        }
    }
}

#[cfg(test)]
mod authored {
    use super::*;
    fn owner(index: usize) -> BehaviorId {
        BehaviorId(crate::Handle {
            runtime: 77,
            index,
            generation: 1,
        })
    }
    fn catalog(
        source: u32,
        group: u32,
        low_key: u8,
        high_key: u8,
        region: Option<usize>,
        regions: usize,
    ) -> SourceZoneCatalog {
        SourceZoneCatalog::new(
            vec![SourceZoneIdentity {
                id: Some(source),
                home: SourceZoneHome {
                    group,
                    low_key,
                    high_key,
                },
                region,
            }],
            regions,
            1,
            4096,
        )
        .unwrap()
    }
    fn bank(low: u8, high: u8, jobs: usize) -> PreparedRouting {
        try_bank(low, high, jobs, 1 << 20).unwrap()
    }
    fn try_bank(low: u8, high: u8, jobs: usize, bytes: usize) -> Result<PreparedRouting, Error> {
        let mut cells = Vec::new();
        for key in 0..128 {
            if (40..=80).contains(&key) {
                cells.push(CandidateIdentity {
                    region: 0,
                    key,
                    initial: true,
                });
            }
            cells.push(CandidateIdentity {
                region: 1,
                key,
                initial: true,
            });
        }
        PreparedRouting::new(
            3,
            vec![ZoneRouting {
                source_zone: 9001,
                group: 42,
                low_key: low,
                high_key: high,
                variants: vec![
                    ZoneRouteVariant {
                        group: 42,
                        region: Some(0),
                    },
                    ZoneRouteVariant {
                        group: 11,
                        region: Some(1),
                    },
                    ZoneRouteVariant {
                        group: 99,
                        region: None,
                    },
                ],
            }],
            &cells,
            2,
            &catalog(9001, 42, low, high, Some(0), 2),
            ZoneRoutingLimits {
                zones: 1,
                variants: 3,
                candidates: 256,
                jobs,
                bytes,
            },
        )
    }
    fn queue(
        s: &mut State,
        p: &PreparedRouting,
        owner: BehaviorId,
        id: i32,
        parameter: ZoneParameter,
        value: i32,
    ) {
        let r = s
            .reserve(p, owner, ScriptInstanceId(0), 9001, parameter, value)
            .unwrap();
        s.enqueue_reserved(r, id, None);
    }
    fn deliver(s: &mut State, expected: &[(i32, bool)]) {
        for &(id, success) in expected {
            let c = s.completion().unwrap();
            assert_eq!((c.id, c.success), (id, success));
            s.delivered(id).unwrap();
        }
        assert!(s.completion().is_none());
    }
    fn callback() -> ReservedCallback {
        ReservedCallback {
            program: 0,
            context: crate::behavior::PlanContext::Bare,
        }
    }
    #[test]
    fn physical_roster_retains_native_zero_sparse_ids_order_and_filtered_homes() {
        let rows = vec![
            SourceZoneIdentity {
                id: Some(0),
                home: SourceZoneHome {
                    group: 42,
                    low_key: 60,
                    high_key: 60,
                },
                region: Some(0),
            },
            SourceZoneIdentity {
                id: Some(7),
                home: SourceZoneHome {
                    group: 11,
                    low_key: 0,
                    high_key: 127,
                },
                region: None,
            },
            SourceZoneIdentity {
                id: Some(2018),
                home: SourceZoneHome {
                    group: 92,
                    low_key: 12,
                    high_key: 84,
                },
                region: Some(1),
            },
        ];
        let p = SourceZoneCatalog::new(rows, 2, 3, 4096).unwrap();
        assert_eq!(p.len(), 3);
        assert_eq!(
            (0..4).map(|i| p.identity(i)).collect::<Vec<_>>(),
            vec![Some(0), Some(7), Some(2018), None]
        );
        assert_eq!(
            p.find(0).unwrap().region,
            Some(0),
            "waveform0 uses explicit native ID0"
        );
        assert_eq!(
            p.find(7).unwrap().home,
            SourceZoneHome {
                group: 11,
                low_key: 0,
                high_key: 127
            }
        );
        assert_eq!(
            p.find(7).unwrap().region,
            None,
            "source filtering preserves metadata without fake PCM"
        );
        assert!(p.find(1).is_none());
        // A shared identity stays on both zones and finds the first.
        let mut duplicate = p.rows.to_vec();
        duplicate[2].id = Some(0);
        let shared = SourceZoneCatalog::new(duplicate, 2, 3, 4096).unwrap();
        assert_eq!((shared.identity(0), shared.identity(2)), (Some(0), Some(0)));
        assert_eq!(shared.find(0).unwrap().region, Some(0));
        let mut unknown = p.rows.to_vec();
        unknown[1].id = None;
        let p = SourceZoneCatalog::new(unknown, 2, 3, 4096).unwrap();
        assert_eq!(p.identity(1), None);
        assert!(p.find(7).is_none());
    }
    #[test]
    fn reserved_callback_ids_survive_ordinary_admissions_at_exact_exhaustion() {
        let mut last = i32::MAX - 3;
        let mut reserved = 2;
        assert_eq!(allocate_callback_id(&mut last, reserved), Ok(i32::MAX - 2));
        assert_eq!(
            allocate_callback_id(&mut last, reserved),
            Err(Error::Capacity)
        );
        assert_eq!(
            last,
            i32::MAX - 2,
            "refusal must not publish/consume a callback ID"
        );
        reserved -= 1;
        assert_eq!(allocate_callback_id(&mut last, reserved), Ok(i32::MAX - 1));
        reserved -= 1;
        assert_eq!(allocate_callback_id(&mut last, reserved), Ok(i32::MAX));
        assert_eq!(
            allocate_callback_id(&mut last, reserved),
            Err(Error::Capacity)
        );
        assert_eq!(callback_id_preflight(i32::MAX - 2, 2, 0), Ok(()));
        assert_eq!(
            callback_id_preflight(i32::MAX - 2, 2, 1),
            Err(Error::Capacity)
        );
        assert_eq!(
            callback_id_preflight(0, usize::MAX, 1),
            Err(Error::Capacity)
        );
    }
    #[test]
    fn cancelled_job_retains_exact_completion_slot_until_delivery_and_panic_releases_all() {
        let p = bank(60, 60, 4);
        let mut s = p.state().unwrap();
        let a = owner(0);
        let b = owner(1);
        let mut arena = crate::Arena::<u8>::new(77, 3);
        let owner_slot = arena.insert(9).unwrap();
        let mut headroom = 0usize;
        let mut pins = 0usize;
        let mut last = i32::MAX - 2;
        for (id, owner) in [(1, a), (2, b)] {
            let r = s
                .reserve(
                    &p,
                    owner,
                    ScriptInstanceId(0),
                    9001,
                    ZoneParameter::Group,
                    11,
                )
                .unwrap();
            callback_id_preflight(last, headroom, 1).unwrap();
            arena.reserve(1);
            headroom += 1;
            pins += 1;
            s.enqueue_reserved(r, id, Some(callback()));
        }
        assert_eq!(
            (arena.available(), arena.reserved, headroom, pins),
            (0, 2, 2, 2)
        );
        assert_eq!(arena.insert(8), Err(Error::Capacity));
        s.cancel_owner(a);
        s.cancel_owner(a);
        assert_eq!(s.get(&p, 9001, ZoneParameter::Group), Ok(42));
        assert_eq!(
            (arena.reserved, headroom, pins),
            (2, 2, 2),
            "cancel keeps exactly-once failure reserved"
        );
        let c = s.completion().unwrap();
        assert_eq!(
            (c.id, c.success, c.refusal),
            (1, false, Some(ZoneRoutingRefusal::Cancelled))
        );
        // Same transfer order as Runtime: release own reservation, transfer job
        // pin to admitted callback, then remove job before running any callback.
        arena.unreserve(1);
        headroom -= 1;
        pins -= 1;
        allocate_callback_id(&mut last, headroom).unwrap();
        let completed = arena.insert(1).unwrap();
        pins += 1;
        s.delivered(c.id).unwrap();
        assert_eq!((arena.reserved, headroom, pins, s.jobs.len()), (1, 1, 2, 1));
        assert!(
            s.completion().is_none(),
            "second owner's pending job is not spuriously completed"
        );
        let cancelled = s.cancel_all();
        arena.unreserve(cancelled.callbacks);
        headroom -= cancelled.callbacks;
        pins -= cancelled.jobs;
        assert_eq!((cancelled.jobs, cancelled.callbacks), (1, 1));
        assert_eq!((arena.reserved, headroom, pins, s.jobs.len()), (0, 0, 1, 0));
        let again = s.cancel_all();
        assert_eq!((again.jobs, again.callbacks), (0, 0));
        arena.remove(completed);
        pins -= 1;
        arena.remove(owner_slot);
        assert_eq!(
            (arena.available(), arena.reserved, headroom, pins),
            (3, 0, 0, 0)
        );
        assert_eq!(
            allocate_callback_id(&mut last, headroom),
            Ok(i32::MAX),
            "cancelled reservation returns numeric headroom"
        );
    }
    #[test]
    fn rejected_job_and_no_callback_instance_consume_no_completion_reservation() {
        let p = bank(60, 60, 1);
        let mut s = p.state().unwrap();
        let owner = owner(0);
        let arena = crate::Arena::<u8>::new(77, 1);
        let high = 17;
        let headroom = 0;
        assert!(
            s.reserve(
                &p,
                owner,
                ScriptInstanceId(0),
                9001,
                ZoneParameter::Group,
                99
            )
            .is_err()
        );
        assert_eq!(
            (
                arena.available(),
                arena.reserved,
                headroom,
                high,
                s.jobs.len()
            ),
            (1, 0, 0, 17, 0)
        );
        queue(&mut s, &p, owner, 18, ZoneParameter::Group, 11);
        s.flush(&p, owner);
        assert!(s.completion().unwrap().completion.is_none());
        deliver(&mut s, &[(18, true)]);
        assert_eq!((arena.available(), arena.reserved, headroom), (1, 0, 0));
    }
    #[test]
    fn zero_zone_identity_is_valid_and_unknown_is_an_option_not_zero() {
        let cells = [CandidateIdentity {
            region: 0,
            key: 60,
            initial: true,
        }];
        let p = PreparedRouting::new(
            3,
            vec![ZoneRouting {
                source_zone: 0,
                group: 42,
                low_key: 60,
                high_key: 60,
                variants: vec![ZoneRouteVariant {
                    group: 42,
                    region: Some(0),
                }],
            }],
            &cells,
            1,
            &catalog(0, 42, 60, 60, Some(0), 1),
            ZoneRoutingLimits {
                zones: 1,
                variants: 1,
                candidates: 1,
                jobs: 1,
                bytes: 4096,
            },
        )
        .unwrap();
        let s = p.state().unwrap();
        assert_eq!(s.get(&p, 0, ZoneParameter::Group), Ok(42));
        assert_eq!(s.get(&p, 1, ZoneParameter::Group), Err(Error::InvalidInput));
    }
    #[test]
    fn final_home_batch_has_no_transient_pitch_refusal_and_keeps_all_ids() {
        let p = bank(60, 60, 16);
        let mut s = p.state().unwrap();
        let owner = owner(0);
        queue(&mut s, &p, owner, 1, ZoneParameter::Group, 11);
        queue(&mut s, &p, owner, 2, ZoneParameter::LowKey, 0);
        queue(&mut s, &p, owner, 3, ZoneParameter::HighKey, 127);
        assert_eq!(s.get(&p, 9001, ZoneParameter::Group), Ok(42));
        s.flush(&p, owner);
        assert_eq!(s.get(&p, 9001, ZoneParameter::Group), Ok(11));
        deliver(&mut s, &[(1, true), (2, true), (3, true)]);
        // Demotion first: sampler cannot play transient0..127. Only final60 matters.
        queue(&mut s, &p, owner, 4, ZoneParameter::Group, 42);
        queue(&mut s, &p, owner, 5, ZoneParameter::LowKey, 60);
        queue(&mut s, &p, owner, 6, ZoneParameter::HighKey, 60);
        s.flush(&p, owner);
        assert_eq!(s.get(&p, 9001, ZoneParameter::Group), Ok(42));
        assert_eq!(s.get(&p, 9001, ZoneParameter::LowKey), Ok(60));
        deliver(&mut s, &[(4, true), (5, true), (6, true)]);
        s.flush(&p, owner);
        assert!(s.completion().is_none());
    }
    #[test]
    fn unsupported_final_home_is_atomic_and_unavailable_target_never_queues() {
        let p = bank(60, 60, 8);
        let mut s = p.state().unwrap();
        let owner = owner(0);
        let before = s.words.clone();
        queue(&mut s, &p, owner, 1, ZoneParameter::HighKey, 127);
        s.flush(&p, owner);
        assert_eq!(s.get(&p, 9001, ZoneParameter::HighKey), Ok(60));
        assert_eq!(s.words, before);
        deliver(&mut s, &[(1, false)]);
        assert!(matches!(
            s.reserve(
                &p,
                owner,
                ScriptInstanceId(0),
                9001,
                ZoneParameter::Group,
                99
            ),
            Err(Error::InvalidInput)
        ));
        assert!(s.jobs.is_empty());
        assert_eq!(s.words, before);
        assert_eq!(p.unavailable_destinations, 1);
    }
    #[test]
    fn catalog_only_original_is_explicit_and_never_invents_playable_candidates() {
        let roster = catalog(7, 42, 60, 60, None, 0);
        let p = PreparedRouting::new(
            3,
            vec![ZoneRouting {
                source_zone: 7,
                group: 42,
                low_key: 60,
                high_key: 60,
                variants: vec![
                    ZoneRouteVariant {
                        group: 42,
                        region: None,
                    },
                    ZoneRouteVariant {
                        group: 11,
                        region: None,
                    },
                ],
            }],
            &[],
            0,
            &roster,
            ZoneRoutingLimits {
                zones: 1,
                variants: 2,
                candidates: 0,
                jobs: 2,
                bytes: 4096,
            },
        )
        .unwrap();
        let mut s = p.state().unwrap();
        assert_eq!(p.catalog_only_initial_homes, 1);
        assert_eq!(s.get(&p, 7, ZoneParameter::Group), Ok(42));
        assert!(
            s.reserve(
                &p,
                owner(0),
                ScriptInstanceId(0),
                7,
                ZoneParameter::Group,
                11
            )
            .is_err()
        );
        assert!(
            s.reserve(
                &p,
                owner(0),
                ScriptInstanceId(0),
                7,
                ZoneParameter::LowKey,
                60
            )
            .is_err()
        );
        assert!(s.jobs.is_empty());
        assert!(s.words.is_empty());
        // Even a prepared destination cannot promote a metadata-only primary.
        let cells = [CandidateIdentity {
            region: 0,
            key: 60,
            initial: true,
        }];
        let unavailable = PreparedRouting::new(
            3,
            vec![ZoneRouting {
                source_zone: 7,
                group: 42,
                low_key: 60,
                high_key: 60,
                variants: vec![
                    ZoneRouteVariant {
                        group: 42,
                        region: None,
                    },
                    ZoneRouteVariant {
                        group: 11,
                        region: Some(0),
                    },
                ],
            }],
            &cells,
            1,
            &catalog(7, 42, 60, 60, None, 1),
            ZoneRoutingLimits {
                zones: 1,
                variants: 2,
                candidates: 1,
                jobs: 2,
                bytes: 8192,
            },
        )
        .unwrap();
        let mut state = unavailable.state().unwrap();
        assert!(
            state
                .reserve(
                    &unavailable,
                    owner(0),
                    ScriptInstanceId(0),
                    7,
                    ZoneParameter::Group,
                    11
                )
                .is_err()
        );
        assert_eq!(
            state.last_refusal,
            Some(ZoneRoutingRefusal::UnavailableTemplate)
        );
        assert!(state.jobs.is_empty());
        assert_eq!(state.next_eligible(0..1), None);
        // Same unavailable bank cannot be substituted for an admitted original.
        let cells = [CandidateIdentity {
            region: 0,
            key: 60,
            initial: true,
        }];
        assert!(matches!(
            PreparedRouting::new(
                3,
                vec![ZoneRouting {
                    source_zone: 7,
                    group: 42,
                    low_key: 60,
                    high_key: 60,
                    variants: vec![ZoneRouteVariant {
                        group: 42,
                        region: None
                    }]
                }],
                &cells,
                1,
                &catalog(7, 42, 60, 60, Some(0), 1),
                ZoneRoutingLimits {
                    zones: 1,
                    variants: 1,
                    candidates: 1,
                    jobs: 2,
                    bytes: 4096
                }
            ),
            Err(Error::InvalidInput)
        ));
    }
    #[test]
    fn authored_initial_partial_domain_restores_exactly_without_fake_candidates() {
        let p = bank(0, 127, 8);
        let mut s = p.state().unwrap();
        let owner = owner(0);
        assert_eq!((p.partial_initial_homes, p.partial_initial_keys), (1, 87));
        let original = s.words.clone();
        assert_eq!(s.get(&p, 9001, ZoneParameter::LowKey), Ok(0));
        assert_eq!(s.get(&p, 9001, ZoneParameter::HighKey), Ok(127));
        queue(&mut s, &p, owner, 1, ZoneParameter::Group, 11);
        s.flush(&p, owner);
        deliver(&mut s, &[(1, true)]);
        queue(&mut s, &p, owner, 2, ZoneParameter::Group, 42);
        s.flush(&p, owner);
        deliver(&mut s, &[(2, true)]);
        assert_eq!(s.words, original);
        assert_eq!(
            s.next_eligible(0..1),
            None,
            "unsupported original low key has no source cell"
        );
    }
    #[test]
    fn small_capacity_refuses_before_id_publication_and_owner_flush_is_exact() {
        let p = bank(60, 60, 1);
        let mut s = p.state().unwrap();
        let a = owner(0);
        let b = owner(1);
        queue(&mut s, &p, a, 1, ZoneParameter::LowKey, 59);
        assert!(matches!(
            s.reserve(&p, b, ScriptInstanceId(0), 9001, ZoneParameter::HighKey, 62),
            Err(Error::Capacity)
        ));
        s.flush(&p, b);
        assert!(s.completion().is_none());
        assert_eq!(s.get(&p, 9001, ZoneParameter::LowKey), Ok(60));
        s.flush(&p, a);
        assert_eq!(s.get(&p, 9001, ZoneParameter::LowKey), Ok(59));
        deliver(&mut s, &[(1, true)]);
        assert!(s.delivered(1).is_err());
        assert!(
            s.reserve(&p, a, ScriptInstanceId(0), 8888, ZoneParameter::Group, 11)
                .is_err()
        );
    }

    fn runtime_plan() -> crate::Prepared {
        runtime_plan_with_jobs(4)
    }
    fn runtime_plan_with_jobs(jobs: usize) -> crate::Prepared {
        let region = |gain| crate::Region {
            sample: 0,
            key_low: 60,
            key_high: 60,
            root_key: None,
            velocity_low: 0.,
            velocity_high: 1.,
            gain,
            envelope: crate::Envelope::new(0, 0, 0, 1., 0).unwrap(),
            playback: crate::Playback::default(),
        };
        let program = |instance| {
            crate::Program::new(vec![crate::Instruction::End])
                .unwrap()
                .with_script_instance(ScriptInstanceId(instance))
                .with_wait_lifetime(crate::WaitLifetime::Callback)
        };
        crate::Prepared::new(
            48_000,
            vec![crate::Pcm::new(48_000, vec![[0.25; 2]; 64].into_boxed_slice()).unwrap()],
            vec![region(1.), region(0.5)],
            2,
        )
        .unwrap()
        .with_groups(2, vec![Some(0), Some(1)])
        .unwrap()
        .with_source_groups(vec![42, 11])
        .unwrap()
        .with_source_zone_identities(
            vec![SourceZoneIdentity {
                id: Some(0),
                home: SourceZoneHome {
                    group: 42,
                    low_key: 60,
                    high_key: 60,
                },
                region: Some(0),
            }],
            1,
            4096,
        )
        .unwrap()
        .with_zone_routing(
            3,
            vec![ZoneRouting {
                source_zone: 0,
                group: 42,
                low_key: 60,
                high_key: 60,
                variants: vec![
                    ZoneRouteVariant {
                        group: 42,
                        region: Some(0),
                    },
                    ZoneRouteVariant {
                        group: 11,
                        region: Some(1),
                    },
                ],
            }],
            ZoneRoutingLimits {
                zones: 1,
                variants: 2,
                candidates: 256,
                jobs,
                bytes: 32 << 20,
            },
            &super::super::Tuning::default(),
        )
        .unwrap()
        .with_script_instances(vec![vec![], vec![]])
        .unwrap()
        .with_programs(vec![program(0), program(0), program(1)], None)
        .unwrap()
        .with_signal_programs(vec![crate::SignalProgram {
            signal: crate::MIDI_ASYNC_SIGNAL,
            program: 1,
            stage: 0,
        }])
        .unwrap()
    }
    fn runtime_for(plan: crate::Prepared) -> crate::Runtime {
        let mut limits = crate::Limits::for_plan(&plan, 4, 4);
        limits.behaviors = 5;
        limits.behavior_cells = plan.behavior_local_count() * 5;
        crate::Runtime::new(plan, limits).unwrap()
    }
    #[test]
    fn actual_runtime_numeric_headroom_is_reserved_before_positive_async_ids() {
        let mut rt = runtime_for(runtime_plan());
        let plan = rt.active_plan();
        rt.last_callback_id = i32::MAX - 4;
        let owner = rt
            .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        let mut ids = [0; 3];
        for id in &mut ids {
            *id = rt
                .request_zone_parameter(owner, 0, ZoneParameter::Group, 11)
                .unwrap();
        }
        assert_eq!(ids, [1, 2, 3]);
        assert_eq!(
            (
                rt.last_callback_id,
                rt.callback_id_reservations,
                rt.behaviors.reserved
            ),
            (i32::MAX - 3, 3, 3)
        );
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, 4);
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 42),
            Err(Error::Capacity)
        );
        assert_eq!(
            rt.plans
                .get(plan.0)
                .unwrap()
                .midi_object
                .async_id_high_water(),
            3
        );
        assert_eq!(
            rt.admit_plan_context(plan, 2, crate::behavior::PlanContext::Bare),
            Err(Error::Capacity)
        );
        rt.flush_zone_parameters(owner);
        rt.drain_behavior();
        assert_eq!(
            (
                rt.last_callback_id,
                rt.callback_id_reservations,
                rt.behaviors.reserved
            ),
            (i32::MAX, 0, 0)
        );
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            0
        );
        assert_eq!(rt.zone_parameter_in(plan, 0, ZoneParameter::Group), Ok(11));
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(
            rt.plans.get(plan.0).unwrap().callbacks,
            1,
            "only the unrun owner remains pinned"
        );
    }
    #[test]
    fn actual_cancel_and_panic_conserve_job_pins_and_callback_reservations() {
        let mut rt = runtime_for(runtime_plan());
        let plan = rt.active_plan();
        let owner = rt
            .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11)
            .unwrap();
        rt.cancel_behavior(owner).unwrap();
        assert_eq!(rt.zone_parameter_in(plan, 0, ZoneParameter::Group), Ok(42));
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (1, 1));
        let c = rt
            .plans
            .get(plan.0)
            .unwrap()
            .zone_routing
            .as_ref()
            .unwrap()
            .completion()
            .unwrap();
        assert_eq!(
            (c.id, c.success, c.refusal),
            (1, false, Some(ZoneRoutingRefusal::Cancelled))
        );
        rt.drain_behavior();
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (0, 0));
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, 0);
        for _ in 0..2 {
            let owner = rt
                .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
                .unwrap();
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11)
                .unwrap();
        }
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (2, 2));
        rt.panic();
        rt.panic();
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (0, 0));
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, 0);
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            0
        );
    }
    #[test]
    fn exact_instance_without_completion_does_not_reserve_another_instances_callback() {
        let mut rt = runtime_for(runtime_plan());
        let plan = rt.active_plan();
        rt.last_callback_id = i32::MAX - 1;
        let owner = rt
            .admit_plan_context(plan, 2, crate::behavior::PlanContext::Bare)
            .unwrap();
        assert_eq!(rt.last_callback_id, i32::MAX);
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
            Ok(1)
        );
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (0, 0));
        rt.flush_zone_parameters(owner);
        rt.drain_behavior();
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, 1);
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            0
        );
    }
    #[test]
    fn accepted_zone_job_pins_its_original_generation_across_plan_replacement() {
        let first = runtime_plan();
        let next = runtime_plan();
        let mut limits = crate::Limits::for_plan(&first, 4, 4);
        limits.behaviors = 5;
        limits.behavior_cells = first.behavior_local_count() * 5;
        let (mut rt, mut control) = crate::Runtime::with_plan_updates(first, limits, 2, 1).unwrap();
        let old = rt.active_plan();
        let owner = rt
            .admit_plan_context(old, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11)
            .unwrap();
        control.submit(Box::new(next)).unwrap();
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_ne!(old, new);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.flush_zone_parameters(owner);
        rt.drain_behavior();
        assert_eq!(rt.zone_parameter_in(old, 0, ZoneParameter::Group), Ok(11));
        assert_eq!(rt.zone_parameter_in(new, 0, ZoneParameter::Group), Ok(42));
        rt.cancel_behavior(owner).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert!(control.retired().is_some());
        assert_eq!(
            rt.zone_parameter_in(old, 0, ZoneParameter::Group),
            Err(Error::StaleHandle)
        );
    }
    #[test]
    fn unexpected_completion_backpressure_restores_reservations_and_audio_clock_advances() {
        let mut rt = runtime_for(runtime_plan());
        let plan = rt.active_plan();
        let owner = rt
            .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11)
            .unwrap();
        rt.flush_zone_parameters(owner);
        // Fault injection: normal admissions cannot consume this held headroom.
        // Exercise the conservation/backpressure path if that invariant breaks.
        rt.last_callback_id = i32::MAX;
        let pins = rt.plans.get(plan.0).unwrap().callbacks;
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        assert_eq!(rt.now(), 8);
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (1, 1));
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, pins);
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            1
        );
        rt.last_callback_id = i32::MAX - 1;
        rt.render(&mut [[0.; 2]; 8]).unwrap();
        assert_eq!(rt.now(), 16);
        assert_eq!((rt.callback_id_reservations, rt.behaviors.reserved), (0, 0));
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            0
        );
    }
    fn queue_work(jobs: &Jobs) -> [usize; 4] {
        [
            jobs.by_id.steps.load(std::sync::atomic::Ordering::Relaxed),
            jobs.by_owner
                .steps
                .load(std::sync::atomic::Ordering::Relaxed),
            jobs.heap_steps.load(std::sync::atomic::Ordering::Relaxed),
            jobs.pending_visits
                .load(std::sync::atomic::Ordering::Relaxed),
        ]
    }
    fn reset_queue_work(jobs: &Jobs) {
        jobs.by_id
            .steps
            .store(0, std::sync::atomic::Ordering::Relaxed);
        jobs.by_owner
            .steps
            .store(0, std::sync::atomic::Ordering::Relaxed);
        jobs.heap_steps
            .store(0, std::sync::atomic::Ordering::Relaxed);
        jobs.pending_visits
            .store(0, std::sync::atomic::Ordering::Relaxed);
    }
    fn assert_index_invariants(index: &SlotIndex, key_of: impl Fn(u32) -> IndexKey) -> Vec<u32> {
        let mut branches = vec![false; index.branches.len()];
        let mut leaves = Vec::new();
        let mut stack = vec![(index.root, None)];
        while let Some((node, parent_bit)) = stack.pop() {
            if node == ABSENT {
                continue;
            }
            if node & LEAF != 0 {
                let slot = node & !LEAF;
                assert_eq!(index.find(key_of(slot), &key_of), slot);
                leaves.push(slot);
            } else {
                assert!(!std::mem::replace(&mut branches[node as usize], true));
                let branch = index.branches[node as usize];
                assert!(parent_bit.is_none_or(|parent| parent < branch.bit));
                assert!(branch.bit < 192);
                for (side, child) in branch.children.into_iter().enumerate() {
                    assert_ne!(
                        child, ABSENT,
                        "compressed branches have two nonempty children"
                    );
                    let mut representative = child;
                    while representative & LEAF == 0 {
                        representative = index.branches[representative as usize].children[0];
                    }
                    assert_eq!(key_bit(key_of(representative & !LEAF), branch.bit), side);
                    stack.push((child, Some(branch.bit)));
                }
            }
        }
        let used = branches.iter().filter(|&&used| used).count();
        assert_eq!(used, leaves.len().saturating_sub(1));
        let mut free = index.free;
        while free != ABSENT {
            assert!(!std::mem::replace(&mut branches[free as usize], true));
            free = index.branches[free as usize].children[0];
        }
        assert!(
            branches.into_iter().all(|seen| seen),
            "every branch is owned or free"
        );
        leaves.sort_unstable();
        assert!(leaves.windows(2).all(|pair| pair[0] != pair[1]));
        leaves
    }
    fn assert_queue_invariants(s: &State) {
        let jobs = &s.jobs;
        let indexed = assert_index_invariants(&jobs.by_id, |slot| id_key(jobs.job(slot).id));
        assert_eq!(indexed.len(), jobs.len());
        let mut owned = vec![false; jobs.capacity()];
        for slot in indexed {
            assert!(!std::mem::replace(&mut owned[slot as usize], true));
        }
        let mut free = jobs.free;
        while free != ABSENT {
            assert!(jobs.slots[free as usize].job.is_none());
            assert!(!std::mem::replace(&mut owned[free as usize], true));
            free = jobs.slots[free as usize].next_pending;
        }
        assert!(
            owned.into_iter().all(|seen| seen),
            "every slot is owned or free"
        );
        let mut scheduled = vec![false; jobs.capacity()];
        for (position, &slot) in jobs.ready.iter().enumerate() {
            assert!(!std::mem::replace(&mut scheduled[slot as usize], true));
            assert!(jobs.job(slot).completed.is_some());
            assert_eq!(jobs.slots[slot as usize].ready_position, position as u32);
            if position > 0 {
                assert!(jobs.job(jobs.ready[(position - 1) / 2]).id < jobs.job(slot).id);
            }
        }
        let heads = assert_index_invariants(&jobs.by_owner, |slot| owner_key(jobs.job(slot).owner));
        for head in heads {
            let owner = jobs.job(head).owner;
            let mut slot = head;
            let mut previous = 0;
            loop {
                let job = jobs.job(slot);
                assert_eq!(job.owner, owner, "full runtime/index/generation identity");
                assert!(job.completed.is_none());
                assert_eq!(jobs.slots[slot as usize].ready_position, ABSENT);
                assert!(
                    job.id > previous,
                    "owner chain preserves acceptance order after reuse"
                );
                previous = job.id;
                assert!(!std::mem::replace(&mut scheduled[slot as usize], true));
                let next = jobs.slots[slot as usize].next_pending;
                if next == ABSENT {
                    assert_eq!(jobs.slots[head as usize].pending_tail, slot);
                    break;
                }
                slot = next;
            }
        }
        for (slot, cell) in jobs.slots.iter().enumerate() {
            assert_eq!(
                cell.job.is_some(),
                scheduled[slot],
                "one pending chain or ready entry per live job"
            );
        }
        assert!(s.touched.is_empty());
        assert!(s.dirty.iter().all(|&dirty| !dirty));
    }
    #[test]
    fn source_sized_indexed_queue_full_drain_and_wait_lookup_have_bounded_near_linear_work() {
        for count in [300, 5_955, MAX_ZONE_BATCH_JOBS] {
            let p = try_bank(60, 60, count, usize::MAX).unwrap();
            let mut s = p.state().unwrap();
            let writer = owner(0);
            let pointers = (
                s.jobs.slots.as_ptr(),
                s.jobs.by_id.branches.as_ptr(),
                s.jobs.by_owner.branches.as_ptr(),
                s.jobs.ready.as_ptr(),
            );
            for i in 0..count {
                queue(
                    &mut s,
                    &p,
                    writer,
                    (7 * i + 11) as i32,
                    ZoneParameter::LowKey,
                    60,
                );
            }
            assert_queue_invariants(&s);
            reset_queue_work(&s.jobs);
            // Polling a full, entirely pending queue performs no indexed work.
            for _ in 0..count {
                assert!(s.completion().is_none());
            }
            assert_eq!(queue_work(&s.jobs), [0; 4]);
            for i in 0..count {
                let id = (7 * i + 11) as i32;
                assert!(s.has_job(id));
                assert!(s.waiting(id));
                assert!(
                    !s.has_job(id + 1),
                    "non-routing async-ID gaps are not aliases"
                );
            }
            let lookup = queue_work(&s.jobs);
            assert!(
                lookup[0] <= 3 * 32 * count,
                "31-bit lookup budget: {lookup:?}"
            );
            assert_eq!(&lookup[1..], &[0; 3]);
            reset_queue_work(&s.jobs);
            s.flush(&p, writer);
            let flush = queue_work(&s.jobs);
            assert_eq!(
                flush[0], 0,
                "flush never scans or searches the all-job ID index"
            );
            assert_eq!(
                flush[3],
                2 * count,
                "exactly two visits to this owner's jobs"
            );
            assert!(flush[2] <= 16 * count, "bounded heap insertions: {flush:?}");
            assert_queue_invariants(&s);
            reset_queue_work(&s.jobs);
            for i in 0..count {
                let id = (7 * i + 11) as i32;
                let before = queue_work(&s.jobs);
                let c = s.completion().unwrap();
                assert_eq!((c.id, c.success), (id, true));
                assert_eq!(queue_work(&s.jobs), before, "ready polling is O(1)");
                assert!(!s.waiting(id));
                s.delivered(id).unwrap();
                assert!(!s.has_job(id));
            }
            let drain = queue_work(&s.jobs);
            assert!(
                drain[0] <= 4 * 32 * count,
                "four bounded radix walks/job: {drain:?}"
            );
            assert_eq!(drain[1], 0, "delivery never looks up/scans owners");
            assert!(
                drain[2] <= 32 * count,
                "at most16 heap levels/job: {drain:?}"
            );
            assert_eq!(drain[3], 0, "delivery never revisits pending jobs");
            assert_eq!(
                pointers,
                (
                    s.jobs.slots.as_ptr(),
                    s.jobs.by_id.branches.as_ptr(),
                    s.jobs.by_owner.branches.as_ptr(),
                    s.jobs.ready.as_ptr()
                )
            );
            assert_queue_invariants(&s);
            eprintln!(
                "queue count={count} lookup={lookup:?} flush={flush:?} drain={drain:?} slot_bytes={} branch_bytes={} payload_bytes={}",
                size_of::<JobSlot>(),
                size_of::<IndexBranch>(),
                Jobs::storage_bytes(count).unwrap()
            );
        }
    }
    #[test]
    fn source_sized_indexed_queue_mixed_owners_out_of_order_reuse_and_cancel_are_exact() {
        let count = source_zone_job_capacity(1_985).unwrap();
        let p = try_bank(60, 60, count, usize::MAX).unwrap();
        let mut s = p.state().unwrap();
        let a = owner(0);
        let b = BehaviorId(crate::Handle {
            generation: 2,
            ..a.0
        });
        let c = BehaviorId(crate::Handle { runtime: 78, ..a.0 });
        let d = BehaviorId(crate::Handle {
            index: usize::MAX,
            ..a.0
        });
        let owners = [a, b, c, d];
        // Independent acceptance-order oracle. It includes immutable captures;
        // it does not depend on the candidate's indexes, lists or heap order.
        let mut model: Vec<(
            i32,
            BehaviorId,
            ScriptInstanceId,
            Option<usize>,
            Option<bool>,
            bool,
        )> = Vec::new();
        let mut next_id = 11i32;
        for i in 0..count {
            let writer = owners[i % 4];
            let instance = ScriptInstanceId((i % 2) as u16);
            let program = (i % 3 != 0).then_some(i);
            let r = s
                .reserve(&p, writer, instance, 9001, ZoneParameter::Group, 11)
                .unwrap();
            s.enqueue_reserved(
                r,
                next_id,
                program.map(|program| ReservedCallback {
                    program,
                    ..callback()
                }),
            );
            model.push((next_id, writer, instance, program, None, false));
            next_id += 7;
        }
        assert_queue_invariants(&s);
        reset_queue_work(&s.jobs);
        s.flush(&p, b);
        for job in &mut model {
            if job.1 == b {
                job.4 = Some(true);
            }
        }
        assert_eq!(
            queue_work(&s.jobs)[3],
            2 * model.iter().filter(|job| job.1 == b).count()
        );
        // Deliver half of B while A/C/D remain pending; hold the next ready
        // result for repeated admission-retry polling, then reuse every hole.
        let freed = count / 8;
        for expected in model.iter_mut().filter(|job| job.1 == b).take(freed) {
            let actual = s.completion().unwrap();
            assert_eq!(
                (
                    actual.id,
                    actual.instance,
                    actual.completion.map(|c| c.program),
                    actual.success
                ),
                (expected.0, expected.2, expected.3, true)
            );
            s.delivered(actual.id).unwrap();
            expected.5 = true;
        }
        let held = s.completion().unwrap();
        for _ in 0..300 {
            assert_eq!(s.completion().unwrap().id, held.id);
        }
        assert_eq!(
            s.delivered(model[0].0),
            Err(Error::InvalidInput),
            "pending job cannot be released"
        );
        assert_queue_invariants(&s);
        for i in 0..freed {
            let writer = if i % 2 == 0 { a } else { b };
            let r = s
                .reserve(
                    &p,
                    writer,
                    ScriptInstanceId(1),
                    9001,
                    ZoneParameter::LowKey,
                    58 + (i % 3) as i32,
                )
                .unwrap();
            s.enqueue_reserved(
                r,
                next_id,
                Some(ReservedCallback {
                    program: i + count,
                    ..callback()
                }),
            );
            model.push((
                next_id,
                writer,
                ScriptInstanceId(1),
                Some(i + count),
                None,
                false,
            ));
            next_id += 7;
        }
        assert_eq!(s.jobs.len(), count);
        assert!(matches!(
            s.reserve(&p, d, ScriptInstanceId(0), 9001, ZoneParameter::HighKey, 61),
            Err(Error::Capacity)
        ));
        assert_eq!(
            s.completion().unwrap().id,
            held.id,
            "slot reuse cannot reorder ready acceptance"
        );
        assert_queue_invariants(&s);
        s.flush(&p, d);
        s.cancel_owner(a);
        s.cancel_owner(c);
        s.flush(&p, b); // only B's newly enqueued batch, previous B results stay true
        let last_b = (0..freed).rev().find(|i| i % 2 == 1).unwrap();
        assert_eq!(
            s.get(&p, 9001, ZoneParameter::LowKey),
            Ok(58 + (last_b % 3) as i32),
            "pending owner links preserve final-write order after slot reuse"
        );
        for job in &mut model {
            if job.4.is_none() {
                job.4 = Some(job.1 == b || job.1 == d);
            }
        }
        assert_queue_invariants(&s);
        for expected in model.iter().filter(|job| !job.5) {
            let actual = s.completion().unwrap();
            assert_eq!(
                (
                    actual.id,
                    actual.instance,
                    actual.completion.map(|c| c.program),
                    actual.success,
                    actual.refusal
                ),
                (
                    expected.0,
                    expected.2,
                    expected.3,
                    expected.4.unwrap(),
                    (!expected.4.unwrap()).then_some(ZoneRoutingRefusal::Cancelled)
                )
            );
            s.delivered(actual.id).unwrap();
            assert_eq!(s.delivered(actual.id), Err(Error::InvalidInput));
        }
        assert!(s.completion().is_none());
        assert_queue_invariants(&s);
        // Full refill after many owner-head and slot lifetimes, then panic with
        // both ready and pending jobs: exact captures/reservations, no callbacks.
        let mut callbacks = 0;
        for i in 0..count {
            let r = s
                .reserve(
                    &p,
                    owners[i % 4],
                    ScriptInstanceId(0),
                    9001,
                    ZoneParameter::Group,
                    42,
                )
                .unwrap();
            let capture = (i % 2 == 0).then(callback);
            callbacks += usize::from(capture.is_some());
            s.enqueue_reserved(r, next_id, capture);
            next_id += 7;
        }
        s.flush(&p, a);
        s.cancel_owner(d);
        assert_queue_invariants(&s);
        let abandoned = s.cancel_all();
        assert_eq!((abandoned.jobs, abandoned.callbacks), (count, callbacks));
        assert_queue_invariants(&s);
        assert_eq!((s.cancel_all().jobs, s.cancel_all().callbacks), (0, 0));
        // Every branch in both indexes must be reusable, not just each job slot.
        for i in 0..count {
            queue(&mut s, &p, owner(i), next_id, ZoneParameter::Group, 11);
            next_id += 7;
        }
        assert_queue_invariants(&s);
        reset_queue_work(&s.jobs);
        for i in (0..count).rev() {
            s.cancel_owner(owner(i));
        }
        let cancellation = queue_work(&s.jobs);
        assert_eq!(cancellation[0], 0);
        assert_eq!(
            cancellation[3], count,
            "one visit/job even for thousands of single-job owners"
        );
        assert!(
            cancellation[1] <= 193 * count,
            "bounded full-handle radix path: {cancellation:?}"
        );
        let mut previous = 0;
        while let Some(result) = s.completion() {
            assert!(result.id > previous);
            assert!(!result.success);
            previous = result.id;
            s.delivered(result.id).unwrap();
        }
        assert_queue_invariants(&s);
    }
    #[test]
    fn indexed_queue_arbitrary_ready_removal_and_empty_single_slot_resources_are_exact() {
        for count in [0, 1, 300] {
            let p = try_bank(60, 60, count, usize::MAX).unwrap();
            let mut s = p.state().unwrap();
            let actual_bytes = s.jobs.slots.len() * size_of::<JobSlot>()
                + (s.jobs.by_id.branches.len() + s.jobs.by_owner.branches.len())
                    * size_of::<IndexBranch>()
                + s.jobs.ready.capacity() * size_of::<u32>();
            assert_eq!(Jobs::storage_bytes(count), Ok(actual_bytes));
            assert_eq!(size_of::<IndexBranch>(), 12);
            assert_eq!(s.jobs.by_id.branches.len(), count.saturating_sub(1));
            assert_eq!(s.jobs.by_owner.branches.len(), count.saturating_sub(1));
            for i in 0..count {
                queue(
                    &mut s,
                    &p,
                    owner(0),
                    (i + 1) as i32,
                    ZoneParameter::Group,
                    11,
                );
            }
            s.flush(&p, owner(0));
            // Retain delivered(id)'s ability to acknowledge any ready entry;
            // exercise both heap repair directions and root/interior/leaf erase.
            let mut remaining: Vec<i32> = (1..=count as i32).collect();
            while !remaining.is_empty() {
                let at = (remaining.len() * 7 / 11).min(remaining.len() - 1);
                let id = remaining.remove(at);
                s.delivered(id).unwrap();
                assert_eq!(s.completion().map(|c| c.id), remaining.first().copied());
                assert_queue_invariants(&s);
            }
            assert_queue_invariants(&s);
            assert!(matches!(
                try_bank(60, 60, count, p.bytes - 1),
                Err(Error::Capacity)
            ));
            assert!(try_bank(60, 60, count, p.bytes).is_ok());
        }
    }
    #[test]
    fn indexed_queue_sparse_async_id_ceiling_and_all_owner_key_bits_remain_distinct() {
        let count = 300;
        let p = try_bank(60, 60, count, usize::MAX).unwrap();
        let mut s = p.state().unwrap();
        let owners: Vec<_> = (0..count)
            .map(|i| {
                BehaviorId(crate::Handle {
                    runtime: if i % 2 == 0 { 0 } else { u64::MAX },
                    index: (i as usize).rotate_left((i % usize::BITS as usize) as u32),
                    generation: u64::MAX - i as u64,
                })
            })
            .collect();
        for (i, &writer) in owners.iter().enumerate() {
            queue(
                &mut s,
                &p,
                writer,
                i32::MAX - 7 * (count - 1 - i) as i32,
                ZoneParameter::Group,
                11,
            );
        }
        assert_queue_invariants(&s);
        for &writer in owners.iter().rev() {
            s.flush(&p, writer);
        }
        assert_queue_invariants(&s);
        for i in 0..count {
            let expected = i32::MAX - 7 * (count - 1 - i) as i32;
            assert_eq!(s.completion().unwrap().id, expected);
            s.delivered(expected).unwrap();
        }
        assert!(!s.has_job(0));
        assert!(!s.has_job(-1));
        assert!(!s.waiting(i32::MAX));
        assert_queue_invariants(&s);
    }
    #[test]
    fn source_budget_and_preparation_bytes_are_checked_before_state_allocation() {
        assert_eq!(source_zone_job_capacity(0), Ok(0));
        assert_eq!(source_zone_job_capacity(1), Ok(256));
        assert_eq!(source_zone_job_capacity(85), Ok(256));
        assert_eq!(source_zone_job_capacity(86), Ok(258));
        assert_eq!(source_zone_job_capacity(100), Ok(300));
        assert_eq!(source_zone_job_capacity(1_985), Ok(5_955));
        let largest = MAX_ZONE_BATCH_JOBS / 3;
        assert_eq!(source_zone_job_capacity(largest), Ok(largest * 3));
        assert_eq!(source_zone_job_capacity(largest + 1), Err(Error::Capacity));
        assert_eq!(source_zone_job_capacity(usize::MAX), Err(Error::Capacity));
        assert_eq!(Jobs::storage_bytes(usize::MAX), Err(Error::Capacity));
        assert!(matches!(
            try_bank(60, 60, MAX_ZONE_BATCH_JOBS + 1, usize::MAX),
            Err(Error::Capacity)
        ));
        let jobs = source_zone_job_capacity(100).unwrap();
        let p = bank(60, 60, jobs);
        assert!(p.bytes >= Jobs::storage_bytes(jobs).unwrap());
        assert!(matches!(
            try_bank(60, 60, jobs, p.bytes - 1),
            Err(Error::Capacity)
        ));
        let exact = try_bank(60, 60, jobs, p.bytes).unwrap();
        let state = exact.state().unwrap();
        assert_eq!(state.jobs.capacity(), jobs);
        assert_eq!(state.touched.capacity(), 1);
        assert_eq!(state.report(&exact).job_capacity, jobs);
    }
    fn source_sized_runtime_plan() -> crate::Prepared {
        // This explicit builder fixture keeps one real zone while exercising
        // every retained async job. Production roster derivation is covered by
        // the ordinary-lower100-zone integration test.
        runtime_plan_with_jobs(source_zone_job_capacity(100).unwrap())
    }
    #[test]
    fn source_sized_limit_plus_one_preserves_all_ids_exact_completions_and_headroom() {
        let p = source_sized_runtime_plan();
        let jobs = p.zone_routing_job_capacity();
        let limits = crate::Limits::for_plan(&p, 4, 4);
        // with_programs installs one stage, even with no note binding.
        assert_eq!(p.stage_count(), 1);
        assert_eq!(limits.behaviors, jobs + 4 * crate::Limits::SCRIPT_KEYS + 1);
        let mut rt = crate::Runtime::new(p, limits).unwrap();
        let plan = rt.active_plan();
        rt.last_callback_id = i32::MAX - jobs as i32 - 1;
        let owner = rt
            .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        for id in 1..=jobs as i32 {
            assert_eq!(
                rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
                Ok(id)
            );
        }
        assert_eq!(
            (rt.behaviors.reserved, rt.callback_id_reservations),
            (jobs, jobs)
        );
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, jobs + 1);
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 42),
            Err(Error::Capacity)
        );
        assert_eq!(
            rt.plans
                .get(plan.0)
                .unwrap()
                .midi_object
                .async_id_high_water(),
            jobs as i32
        );
        assert_eq!(rt.zone_parameter_in(plan, 0, ZoneParameter::Group), Ok(42));
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().refusals[0],
            1
        );
        // Ordinary callbacks cannot steal already reserved numeric identities.
        assert_eq!(
            rt.admit_plan_context(plan, 2, crate::behavior::PlanContext::Bare),
            Err(Error::Capacity)
        );
        rt.flush_zone_parameters(owner);
        rt.dispatching_behavior = true;
        for id in 1..=jobs as i32 {
            let completed = rt
                .plans
                .get(plan.0)
                .unwrap()
                .zone_routing
                .as_ref()
                .unwrap()
                .completion()
                .unwrap();
            assert_eq!(
                (
                    completed.id,
                    completed.success,
                    completed.completion.unwrap().program
                ),
                (id, true, 1)
            );
            assert!(rt.service_zone_completions().unwrap());
            let crate::behavior::Ready::Resume { id: callback, .. } =
                *rt.behavior_ready.last().unwrap()
            else {
                panic!("completion callback missing");
            };
            assert_eq!(
                rt.behaviors.get(callback.0).unwrap().async_result,
                Some((id, 1))
            );
        }
        rt.dispatching_behavior = false;
        rt.drain_behavior();
        assert_eq!(rt.last_callback_id, i32::MAX);
        assert_eq!((rt.behaviors.reserved, rt.callback_id_reservations), (0, 0));
        assert_eq!(
            rt.zone_routing_report(plan).unwrap().unwrap().pending_jobs,
            0
        );
        assert_eq!(rt.zone_parameter_in(plan, 0, ZoneParameter::Group), Ok(11));
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.plans.get(plan.0).unwrap().callbacks, 1);
    }
    #[test]
    fn source_sized_cancelled_generation_jobs_keep_failed_completions_then_retire() {
        let first = source_sized_runtime_plan();
        let next = source_sized_runtime_plan();
        let jobs = first.zone_routing_job_capacity();
        let limits = crate::Limits::for_plan(&first, 4, 4);
        let (mut rt, mut control) = crate::Runtime::with_plan_updates(first, limits, 2, 1).unwrap();
        let old = rt.active_plan();
        let owner = rt
            .admit_plan_context(old, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        for id in 1..=jobs as i32 {
            assert_eq!(
                rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
                Ok(id)
            );
        }
        control.submit(Box::new(next)).unwrap();
        rt.poll_plan_update().unwrap();
        let new = rt.active_plan();
        assert_ne!(old, new);
        assert_eq!(rt.collect_retired_plans(), 0);
        rt.cancel_zone_parameters(owner);
        rt.dispatching_behavior = true;
        for id in 1..=jobs as i32 {
            let completed = rt
                .plans
                .get(old.0)
                .unwrap()
                .zone_routing
                .as_ref()
                .unwrap()
                .completion()
                .unwrap();
            assert_eq!(
                (completed.id, completed.success, completed.refusal),
                (id, false, Some(ZoneRoutingRefusal::Cancelled))
            );
            assert!(rt.service_zone_completions().unwrap());
            let crate::behavior::Ready::Resume { id: callback, .. } =
                *rt.behavior_ready.last().unwrap()
            else {
                panic!("completion callback missing");
            };
            assert_eq!(
                rt.behaviors.get(callback.0).unwrap().async_result,
                Some((id, 0))
            );
        }
        assert_eq!((rt.behaviors.reserved, rt.callback_id_reservations), (0, 0));
        rt.dispatching_behavior = false;
        rt.drain_behavior();
        assert_eq!(rt.zone_parameter_in(old, 0, ZoneParameter::Group), Ok(42));
        assert_eq!(rt.zone_parameter_in(new, 0, ZoneParameter::Group), Ok(42));
        rt.cancel_behavior(owner).unwrap();
        rt.flush_behaviors(|_, _, _| true);
        assert_eq!(rt.collect_retired_plans(), 1);
        assert!(control.retired().is_some());
        assert_eq!(
            rt.zone_parameter_in(old, 0, ZoneParameter::Group),
            Err(Error::StaleHandle)
        );
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
            Err(Error::StaleHandle)
        );
    }
    #[test]
    fn callback_cell_ceiling_is_bounded_and_its_refusal_is_reported_before_async_id() {
        let p = source_sized_runtime_plan();
        let mut limits = crate::Limits::for_plan(&p, 4, 4);
        limits.behaviors = 2;
        limits.behavior_cells = p.behavior_local_count() * limits.behaviors;
        let mut rt = crate::Runtime::new(p, limits).unwrap();
        let plan = rt.active_plan();
        let owner = rt
            .admit_plan_context(plan, 0, crate::behavior::PlanContext::Bare)
            .unwrap();
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
            Ok(1)
        );
        assert_eq!(
            rt.request_zone_parameter(owner, 0, ZoneParameter::Group, 11),
            Err(Error::Capacity)
        );
        assert_eq!(
            rt.plans
                .get(plan.0)
                .unwrap()
                .midi_object
                .async_id_high_water(),
            1
        );
        let report = rt.zone_routing_report(plan).unwrap().unwrap();
        assert_eq!(
            (report.job_capacity, report.pending_jobs, report.refusals[0]),
            (300, 1, 1)
        );
        assert_eq!((rt.behaviors.reserved, rt.callback_id_reservations), (1, 1));
        rt.panic();
        assert_eq!((rt.behaviors.reserved, rt.callback_id_reservations), (0, 0));
    }

    #[test]
    fn default_source_completion_slots_respect_local_cell_and_callback_byte_limits() {
        // This fixture exercises callback/cell admission, not the independent
        // absolute job-count ceiling. Retained callback context is real storage:
        // derive enough jobs for the actual callback ceiling inside the existing
        // 32MiB routing budget, rather than demanding 65536 large JobSlots.
        let bytes_per_slot = size_of::<crate::Slot<crate::behavior::Continuation>>()
            + size_of::<crate::behavior::Ready>()
            + size_of::<crate::BehaviorId>()
            + size_of::<u64>()
            + 4096 * size_of::<i64>();
        let callback_ceiling = (crate::Limits::SCRIPT_CELLS / 4096)
            .min(crate::Limits::SCRIPT_CALLBACK_BYTES / bytes_per_slot);
        assert!(callback_ceiling > 0 && callback_ceiling < MAX_ZONE_BATCH_JOBS);
        let p = runtime_plan_with_jobs(callback_ceiling);
        // The absolute maximum request still refuses before state allocation
        // when even its independently priced job storage cannot fit.
        let insufficient = Jobs::storage_bytes(MAX_ZONE_BATCH_JOBS).unwrap() - 1;
        assert!(matches!(
            try_bank(60, 60, MAX_ZONE_BATCH_JOBS, insufficient),
            Err(Error::Capacity)
        ));
        let program = |instance| {
            crate::Program::new(vec![
                crate::Instruction::SetLocal {
                    local: 4095,
                    value: 0,
                },
                crate::Instruction::End,
            ])
            .unwrap()
            .with_script_instance(ScriptInstanceId(instance))
            .with_wait_lifetime(crate::WaitLifetime::Callback)
        };
        let p = p
            .with_programs(vec![program(0), program(0), program(1)], None)
            .unwrap()
            // Replacing programs clears the async binding from the base fixture.
            .with_signal_programs(vec![crate::SignalProgram {
                signal: crate::MIDI_ASYNC_SIGNAL,
                program: 1,
                stage: 0,
            }])
            .unwrap();
        assert_eq!(p.zone_completion_capacity(), callback_ceiling);
        let limits = crate::Limits::for_plan(&p, 4, 4);
        assert_eq!(limits.behaviors, callback_ceiling);
        assert_eq!(limits.behavior_cells, callback_ceiling * 4096);
        assert!(limits.behavior_cells <= crate::Limits::SCRIPT_CELLS);
        assert!(limits.behaviors * bytes_per_slot <= crate::Limits::SCRIPT_CALLBACK_BYTES);
        assert!(limits.behaviors < MAX_ZONE_BATCH_JOBS);
    }
}
