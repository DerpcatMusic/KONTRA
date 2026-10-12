//! Managed background-task pool.
//!
//! A module-local pool of worker threads runs plugin
//! `BackgroundTask::run` handlers off the audio thread. Each plugin
//! instance owns a
//! preallocated, wait-free inbound queue via a [`TaskSpawner`]: the
//! audio thread (or the editor, or `init`) pushes tasks without
//! allocating or blocking, and a pool worker drains them. Feedback to
//! the audio thread stays the plugin's job through shared `#[skip]`
//! channels - the pool owns only the worker threads and the inbound
//! queue. One lightweight non-realtime notifier owns wake syscalls and retries
//! accepted lane-local work that could not enter the bounded global injector;
//! scheduling from `process` remains atomics and lock-free queues only.
//! A handler that returns a continuation yields after one pass; workers
//! alternate that lane-owned tail with newly submitted work and with other
//! ready instances instead of looping one long catch-up job in place.
//!
//! ## Concurrency
//!
//! By default drains are **not** mutually exclusive: the stranding-
//! avoidance handshake clears a sink's `scheduled` flag before draining,
//! so a burst that re-arms an instance mid-drain can hand a second idle
//! worker the same sink - `run` can run concurrently with itself for
//! one instance. Handlers must therefore be reentrancy-safe: talk to the
//! audio thread only through lock-free / atomic channels (the reverb
//! example's MPMC handoff), or guard shared mutable state (the
//! `AudioTap::drain_with` `try_lock` idiom). A plugin that can't meet that
//! contract sets `BackgroundTask::SERIALIZED = true`, and the pool then
//! runs that instance's handler one at a time.
//!
//! A static plug-in module shares one pool across its instances. Each
//! hot-reload logic generation has its own pool because its monomorphized
//! handlers and queue operations live in that dylib. The loader warms the
//! candidate pool off-thread, then closes the old lanes and joins the old
//! workers before swapping generations. A plugin that never declares a
//! `BackgroundTask` spawns no threads.
//!
//! Because the pool is shared and small (`available_parallelism() - 1`,
//! as few as one thread), task handlers must stay short and
//! non-blocking: one plugin that blocks on I/O or a lock stalls every
//! other instance's background work. Long or blocking work belongs on a
//! plugin's own thread (`AudioTap::spawn_worker`), not the pool.

// Gated on `not(miri)` to match `pin_current_module`, which is a no-op
// under Miri (no dynamic loader to pin), so these FFI types aren't used.
use std::collections::VecDeque;
#[cfg(all(unix, not(miri)))]
use std::ffi::c_void;
#[cfg(all(unix, not(miri)))]
use std::ffi::{c_char, c_int};
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU8, AtomicUsize, Ordering, fence};
use std::sync::{Arc, Mutex, RwLock, Weak};
use std::thread::{self, JoinHandle, Thread};
use std::time::{Duration, Instant};

use crossbeam_queue::ArrayQueue;

use crate::snapshot::SnapshotPublisher;

/// Preallocated inbound-queue capacity per instance. Mirrors
/// `EVENT_LIST_PREALLOC`: a block that schedules more tasks than this
/// drops the overflow (`try_spawn` returns `Err`) rather than
/// allocating on the audio thread.
pub const TASK_QUEUE_PREALLOC: usize = 256;

/// How many instances can have pending work queued in the pool at once.
/// Sized well past any realistic simultaneous-instance count.
const INJECTOR_CAP: usize = 4096;

/// Ordinary worker defensive timeout. If notifier startup fails, exactly one
/// designated worker uses [`NOTIFIER_INTERVAL`] instead; the others keep this
/// long park so fallback does not become a thundering herd.
const PARK_TIMEOUT: Duration = Duration::from_secs(1);
/// One non-realtime notifier polls the atomic scheduling epoch at this rate,
/// retries lanes that missed the bounded injector, and owns all wake syscalls.
const NOTIFIER_INTERVAL: Duration = Duration::from_millis(1);
/// Serialized contention owns a real injector entry, but must not spin on it
/// while the current bounded handler finishes.
const BUSY_RETRY_TIMEOUT: Duration = Duration::from_millis(1);

#[derive(Clone, Copy, PartialEq, Eq)]
enum DrainOutcome {
    Done,
    Deferred,
    Busy,
}

/// A drainable instance queue, type-erased so the one pool holds many
/// task types at once.
trait Drain: Send + Sync {
    /// Run one fair drain turn and report whether the worker owns a later
    /// continuation turn or collided with this serialized lane's handler.
    fn drain(&self) -> DrainOutcome;
    /// Claim one off-thread retry request and arm this lane for injection.
    fn claim_retry(&self) -> bool;
    /// Restore a claimed retry after the bounded injector was still full.
    fn retry_failed(&self);
}

/// Lanes register off-thread when their spawner is built. The notifier scans
/// weak handles only, so registry membership never extends an instance's life.
static REGISTERED_LANES: Mutex<Vec<Weak<dyn Drain>>> = Mutex::new(Vec::new());
/// Audio/editor scheduling publishes only this atomic epoch; the notifier owns
/// `Thread::unpark` and every retry of a full/unavailable injector.
static NOTIFY_EPOCH: AtomicUsize = AtomicUsize::new(0);
/// Sticky module-local hint: the notifier clears it only for one retry scan
/// and republishes it when the injector is still saturated.
static RETRY_PENDING: AtomicBool = AtomicBool::new(false);

/// Per-instance inbound queue plus the monomorphized handler. Shared
/// (`Arc`) between the schedulers (audio thread / editor / init, via
/// [`TaskSpawner`]) and the pool worker that drains it.
struct Sink<T: Send + 'static> {
    /// `try_spawn`: FIFO, every queued task runs.
    queue: ArrayQueue<T>,
    /// `spawn_coalescing`: a single slot. `force_push` keeps only the
    /// newest target, and `drain` runs it at most once, so a burst of
    /// requests between two drains collapses to one execution instead of
    /// running one build per intermediate target.
    coalesced: ArrayQueue<T>,
    /// Preallocated lane-owned tails returned by managed handlers. Overflow
    /// drops the newest returned tail instead of allocating or blocking.
    continuation: ArrayQueue<T>,
    /// Alternates lane-owned continuation work with newly submitted work so
    /// neither source can starve the other.
    prefer_continuation: AtomicBool,
    /// Coalesces wake-ups: set when this sink is already queued in the
    /// injector, so a burst of pushes injects it once.
    scheduled: AtomicBool,
    /// Set when a local task was accepted but its lane could not enter the
    /// bounded global injector. Cleared only by the non-realtime retry owner.
    retry_requested: AtomicBool,
    /// Serialized ("one-slot") mode: when set, at most one worker runs
    /// `run` for this sink at a time. `false` (default) lets a second
    /// worker drain concurrently for throughput.
    serialized: bool,
    /// Exclusive-drain guard for [`Self::serialized`]. A worker that finds
    /// it already held defers its injector entry instead of spinning or
    /// draining again in the current turn. Unused in concurrent mode.
    draining: AtomicBool,
    /// One linearizable scheduling gate: the high bit closes the lane and
    /// the remaining bits count producers between admission and the final
    /// pool injection. Retirement sets the closed bit and waits for the
    /// count to reach zero before clearing queues, so a producer can never
    /// enqueue after that clear.
    schedule_state: AtomicUsize,
    /// `run(task)` is `move |task| task.run(&params)`, built
    /// once when the instance registers - never per task.
    run: Box<dyn Fn(T) -> Option<T> + Send + Sync>,
}

impl<T: Send + 'static> Sink<T> {
    /// Run one task, catching panics so a bad handler can't kill the
    /// shared worker (which would strand every other instance's tasks).
    /// `run`/`task` are effectively unwind-safe: `run` is `&`-borrowed and
    /// a poisoned task is simply dropped.
    fn run_one(&self, task: T) -> bool {
        if self.schedule_state.load(Ordering::Acquire) & LANE_CLOSED != 0 {
            return false;
        }
        let run = &self.run;
        let next = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(task)))
            .ok()
            .flatten();
        if let Some(next) = next {
            // Closing an instance never waits for handler duration. Only the
            // bounded tail publication enters the lane gate: close either
            // precedes it and rejects the continuation, or follows it, waits
            // for this short section, and clears the published tail.
            let Some(_publishing) = self.begin_schedule() else {
                return false;
            };
            self.continuation.push(next).is_ok()
        } else {
            false
        }
    }
}

impl<T: Send + 'static> Sink<T> {
    /// Clear `scheduled`, then run one fair task turn. The
    /// clear-before-drain + `SeqCst` fence is the stranding-avoidance
    /// handshake: a task pushed mid-drain re-arms the sink (its `arm` swap
    /// sees `scheduled == false`) instead of being stranded. Release/AcqRel
    /// don't order a store followed by a load of a *different* location, so
    /// without the `SeqCst` store + fence a worker could clear the flag, read
    /// the queue empty, and a concurrent producer could push a task and read
    /// the flag still `true` - stranding it. Only `SeqCst` forbids that.
    fn drain_queues(&self) -> bool {
        self.scheduled.store(false, Ordering::SeqCst);
        fence(Ordering::SeqCst);
        let continuation = || self.continuation.pop();
        let submitted = || self.coalesced.pop().or_else(|| self.queue.pop());
        let task = if self.prefer_continuation.fetch_xor(true, Ordering::Relaxed) {
            continuation().or_else(submitted)
        } else {
            submitted().or_else(continuation)
        };
        if let Some(task) = task {
            let _ = self.run_one(task);
        }
        fence(Ordering::SeqCst);
        let pending =
            !self.queue.is_empty() || !self.coalesced.is_empty() || !self.continuation.is_empty();
        pending && !self.scheduled.swap(true, Ordering::SeqCst)
    }
}

impl<T: Send + 'static> Drain for Sink<T> {
    fn drain(&self) -> DrainOutcome {
        // Concurrent (default) mode: a second worker may drain this sink at
        // the same time. Handlers must be reentrancy-safe (see
        // `BackgroundTask::SERIALIZED`).
        if !self.serialized {
            return if self.drain_queues() {
                DrainOutcome::Deferred
            } else {
                DrainOutcome::Done
            };
        }
        // Serialized ("one-slot") mode: run the handler for this instance on
        // at most one worker at a time. A worker that finds the guard held
        // is inert and returns - it touches nothing, so the `scheduled`
        // handshake below stays the sole no-stranding signal, exactly as in
        // the concurrent path.
        if self.draining.swap(true, Ordering::Acquire) {
            return DrainOutcome::Busy;
        }
        let reinject = self.drain_queues();
        self.draining.store(false, Ordering::Release);
        if reinject {
            DrainOutcome::Deferred
        } else {
            DrainOutcome::Done
        }
    }

    fn claim_retry(&self) -> bool {
        if !self.retry_requested.swap(false, Ordering::AcqRel)
            || self.schedule_state.load(Ordering::Acquire) & LANE_CLOSED != 0
        {
            return false;
        }
        fence(Ordering::SeqCst);
        !self.scheduled.swap(true, Ordering::SeqCst)
    }

    fn retry_failed(&self) {
        self.scheduled.store(false, Ordering::SeqCst);
        if self.schedule_state.load(Ordering::Acquire) & LANE_CLOSED == 0 {
            self.retry_requested.store(true, Ordering::Release);
        }
    }
}

fn register_lane(sink: &Arc<dyn Drain>) {
    REGISTERED_LANES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(Arc::downgrade(sink));
}

/// Retry every accepted lane that previously missed the global injector.
/// Called only by non-realtime pool threads.
fn retry_registered(shared: &Shared) -> (usize, bool) {
    let mut injected = 0_usize;
    let mut remaining = false;
    REGISTERED_LANES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .retain(|weak| {
            let Some(sink) = weak.upgrade() else {
                return false;
            };
            if sink.claim_retry() {
                match shared.injector.push(sink) {
                    Ok(()) => injected = injected.saturating_add(1),
                    Err(sink) => {
                        sink.retry_failed();
                        remaining = true;
                    }
                }
            }
            true
        });
    (injected, remaining)
}

/// Worker-visible pool state: the injector of ready sinks. Held in an
/// `Arc` so every worker closure can reach it.
struct Shared {
    injector: ArrayQueue<Arc<dyn Drain>>,
    /// Non-realtime notifier's round-robin wake cursor.
    next_worker: AtomicUsize,
    /// One linearizable execution gate: the high bit pauses workers before
    /// they enter a sink and the remaining bits count drains already running.
    /// Reload can therefore pause reversibly, wait for zero active handlers,
    /// and either resume unchanged or stop/join without a new drain racing in.
    execution_state: AtomicUsize,
    /// Published off-thread after notifier startup. The first worker is the
    /// sole short-poll fallback while this remains false.
    notifier_running: AtomicBool,
    stopping: AtomicBool,
}

struct Pool {
    shared: Arc<Shared>,
    workers: Box<[Thread]>,
    joins: Mutex<Option<Vec<JoinHandle<()>>>>,
    notifier: Option<Thread>,
    notifier_join: Mutex<Option<JoinHandle<()>>>,
}

const POOL_COLD: u8 = 0;
const POOL_STARTING: u8 = 1;
const POOL_RUNNING: u8 = 2;
const POOL_STOPPING: u8 = 3;
const POOL_STOPPED: u8 = 4;

/// Module-local pool slot. Hot logic dylibs are unique generations: each is
/// started at most once, stopped at most once, and never restarted. A raw
/// pointer keeps the process scheduling path to one acquire load with no
/// lock or lazy allocation; initialization and destruction are explicitly
/// off-thread exports owned by the hot loader.
static POOL_STATE: AtomicU8 = AtomicU8::new(POOL_COLD);
static POOL_PTR: AtomicPtr<Pool> = AtomicPtr::new(core::ptr::null_mut());

fn running_pool() -> Option<&'static Pool> {
    if POOL_STATE.load(Ordering::Acquire) != POOL_RUNNING {
        return None;
    }
    let ptr = POOL_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        return None;
    }
    // SAFETY: `start_pool` publishes a fully initialized `Box<Pool>` before
    // the RUNNING store. Hot shutdown is called only after every managed lane
    // is closed and its producer count is zero, so no scheduling caller can
    // retain this reference when `shutdown_hot_reload_pool` reclaims it.
    Some(unsafe { &*ptr })
}

fn start_pool(pin_module: bool) -> bool {
    match POOL_STATE.compare_exchange(
        POOL_COLD,
        POOL_STARTING,
        Ordering::AcqRel,
        Ordering::Acquire,
    ) {
        Ok(_) => {}
        Err(POOL_RUNNING) => {
            return running_pool().is_some_and(|pool| !pool.workers.is_empty());
        }
        Err(POOL_STARTING) => {
            while POOL_STATE.load(Ordering::Acquire) == POOL_STARTING {
                thread::yield_now();
            }
            return running_pool().is_some_and(|pool| !pool.workers.is_empty());
        }
        Err(_) => return false,
    }

    let shared = Arc::new(Shared {
        injector: ArrayQueue::new(INJECTOR_CAP),
        next_worker: AtomicUsize::new(0),
        execution_state: AtomicUsize::new(0),
        notifier_running: AtomicBool::new(false),
        stopping: AtomicBool::new(false),
    });
    // One fewer than the core count, floored at one, so the pool never
    // starves the audio and main threads on a small machine.
    let n = thread::available_parallelism().map_or(1, |p| p.get().saturating_sub(1).max(1));
    let mut workers = Vec::with_capacity(n);
    let mut joins = Vec::with_capacity(n);
    for index in 0..n {
        let shared = Arc::clone(&shared);
        match thread::Builder::new()
            .name("moose-task-pool".into())
            .spawn(move || worker_loop(&shared, index == 0))
        {
            Ok(handle) => {
                workers.push(handle.thread().clone());
                joins.push(handle);
            }
            // Pool startup is always off-thread, but still must not unwind
            // across the hot loader's Rust ABI export. Keep any workers that
            // did start; a zero-worker pool is reported as unavailable.
            Err(e) => {
                eprintln!("[moose] task-pool worker spawn failed: {e}");
                break;
            }
        }
    }
    if pin_module && !workers.is_empty() {
        pin_current_module();
    }
    let notifier_join = if workers.is_empty() {
        None
    } else {
        let notifier_workers = workers.clone();
        let notifier_shared = Arc::clone(&shared);
        thread::Builder::new()
            .name("moose-task-notifier".into())
            .spawn(move || notifier_loop(&notifier_shared, &notifier_workers))
            .ok()
    };
    shared
        .notifier_running
        .store(notifier_join.is_some(), Ordering::Release);
    let notifier = notifier_join.as_ref().map(|handle| handle.thread().clone());
    let available = !workers.is_empty();
    let pool = Box::new(Pool {
        shared,
        workers: workers.into_boxed_slice(),
        joins: Mutex::new(Some(joins)),
        notifier,
        notifier_join: Mutex::new(notifier_join),
    });
    POOL_PTR.store(Box::into_raw(pool), Ordering::Release);
    POOL_STATE.store(POOL_RUNNING, Ordering::Release);
    available
}

/// Pin a static plugin module so its process-lifetime shared workers can
/// never wake into unmapped code after host teardown. Hot logic generations
/// do not use this: their loader closes every lane, stops and joins their
/// workers, and separately retains the mapping for editor/vtable safety.
#[cfg(all(unix, not(miri)))]
fn pin_current_module() {
    // Field names mirror the platform's `Dl_info`; only `dli_fname` is
    // read, the rest are here for correct C layout (`dladdr` writes all).
    #[repr(C)]
    #[allow(clippy::struct_field_names)]
    struct DlInfo {
        dli_fname: *const c_char,
        dli_fbase: *mut c_void,
        dli_sname: *const c_char,
        dli_saddr: *mut c_void,
    }
    unsafe extern "C" {
        fn dladdr(addr: *const c_void, info: *mut DlInfo) -> c_int;
        fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    }
    // RTLD_NOLOAD | RTLD_NODELETE: re-reference the already-loaded object
    // and mark it non-deletable. The flag values differ between glibc and
    // Darwin.
    #[cfg(target_os = "linux")]
    const FLAGS: c_int = 0x0004 | 0x1000;
    #[cfg(not(target_os = "linux"))]
    const FLAGS: c_int = 0x0010 | 0x0080;

    // SAFETY: `dladdr` reads the address of a live function in this module
    // and fills `info` with loader-owned strings valid for the immediate
    // `dlopen`. NOLOAD only re-references the already-mapped object; the
    // returned handle is intentionally leaked so NODELETE persists.
    unsafe {
        let mut info: DlInfo = core::mem::zeroed();
        if dladdr(pin_current_module as *const c_void, &raw mut info) != 0
            && !info.dli_fname.is_null()
        {
            let _ = dlopen(info.dli_fname, FLAGS);
        }
    }
}

#[cfg(all(windows, not(miri)))]
fn pin_current_module() {
    use windows_sys::Win32::System::LibraryLoader::{
        GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_PIN, GetModuleHandleExW,
    };
    // FROM_ADDRESS: interpret `name` as an address inside this module;
    // PIN: bump its load count so it never unloads.
    // SAFETY: `name` is the address of a live function in this module;
    // `module` receives the pinned handle, which we intentionally leak.
    unsafe {
        let mut module = core::ptr::null_mut();
        let _ = GetModuleHandleExW(
            GET_MODULE_HANDLE_EX_FLAG_PIN | GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
            pin_current_module as *const u16,
            &raw mut module,
        );
    }
}

// No-op where there's no dynamic loader to pin against, and under Miri -
// which has no dlopen/dlclose (so nothing can unload the module) and
// doesn't support `dladdr` / `GetModuleHandleExW`.
#[cfg(any(miri, not(any(unix, windows))))]
fn pin_current_module() {}

/// Eagerly start a static plugin module's process-lifetime pool and pin that
/// module. The static shell calls this off-thread before audio can schedule.
pub fn warm_pool() {
    let _ = start_pool(true);
}

/// Start one hot logic generation's restart-forbidden pool off-thread.
/// Returns false only when no worker could be created.
#[doc(hidden)]
#[must_use]
pub fn warm_hot_reload_pool() -> bool {
    start_pool(false)
}

const POOL_PAUSED: usize = 1usize << (usize::BITS - 1);
const POOL_EXECUTOR_MASK: usize = POOL_PAUSED - 1;

struct ExecutionGuard<'a>(&'a AtomicUsize);

impl Drop for ExecutionGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

fn begin_execution(shared: &Shared) -> Option<ExecutionGuard<'_>> {
    // One RMW is the worker-entry linearization point. An entry before the
    // PAUSED fetch_or contributes to the count quiesce waits on; an entry
    // after it observes the bit and retains its popped sink without calling
    // plugin code. The guard's decrement lets an Acquire zero observation
    // prove every admitted handler has returned.
    let previous = shared.execution_state.fetch_add(1, Ordering::AcqRel);
    let guard = ExecutionGuard(&shared.execution_state);
    (previous & POOL_PAUSED == 0).then_some(guard)
}

fn wake_workers(pool: &Pool) {
    for worker in &pool.workers {
        worker.unpark();
    }
}

/// Reversibly pause one hot generation before any new handler entry and wait
/// for handlers already running to leave. A timeout restores the running pool
/// exactly as it was; workers that already popped an injector entry retain it
/// until resume instead of losing queued work.
#[doc(hidden)]
#[must_use]
pub fn quiesce_hot_reload_pool(timeout: Duration) -> bool {
    let Some(pool) = running_pool() else {
        return true;
    };
    pool.shared
        .execution_state
        .fetch_or(POOL_PAUSED, Ordering::AcqRel);
    wake_workers(pool);

    let deadline = Instant::now() + timeout;
    while pool.shared.execution_state.load(Ordering::Acquire) & POOL_EXECUTOR_MASK != 0
        && Instant::now() < deadline
    {
        thread::yield_now();
    }
    if pool.shared.execution_state.load(Ordering::Acquire) & POOL_EXECUTOR_MASK == 0 {
        return true;
    }

    pool.shared
        .execution_state
        .fetch_and(POOL_EXECUTOR_MASK, Ordering::AcqRel);
    wake_workers(pool);
    false
}

/// Stop and join one hot logic generation's pool. The loader calls this only
/// after all of that generation's lanes are closed and admitted producers
/// have left their scheduling critical sections. Never called on audio.
#[doc(hidden)]
pub fn shutdown_hot_reload_pool() {
    loop {
        match POOL_STATE.load(Ordering::Acquire) {
            POOL_COLD | POOL_STOPPED => return,
            POOL_STARTING => thread::yield_now(),
            POOL_RUNNING => {
                if POOL_STATE
                    .compare_exchange(
                        POOL_RUNNING,
                        POOL_STOPPING,
                        Ordering::AcqRel,
                        Ordering::Acquire,
                    )
                    .is_ok()
                {
                    break;
                }
            }
            POOL_STOPPING => {
                while POOL_STATE.load(Ordering::Acquire) == POOL_STOPPING {
                    thread::yield_now();
                }
                return;
            }
            _ => unreachable!(),
        }
    }

    let ptr = POOL_PTR.load(Ordering::Acquire);
    if ptr.is_null() {
        POOL_STATE.store(POOL_STOPPED, Ordering::Release);
        return;
    }
    // SAFETY: this function won the sole RUNNING -> STOPPING transition;
    // the pointer remains owned by POOL_PTR until after all joins below.
    let pool = unsafe { &*ptr };
    // Reload calls `quiesce_hot_reload_pool` first. Teardown may come here
    // directly, so close the execution gate and wait without a deadline:
    // unloading with a live handler is unsound, while handlers are required
    // to be short and nonblocking.
    pool.shared
        .execution_state
        .fetch_or(POOL_PAUSED, Ordering::AcqRel);
    while pool.shared.execution_state.load(Ordering::Acquire) & POOL_EXECUTOR_MASK != 0 {
        thread::yield_now();
    }
    pool.shared.stopping.store(true, Ordering::Release);
    wake_workers(pool);
    if let Some(notifier) = &pool.notifier {
        notifier.unpark();
    }
    if let Some(join) = pool
        .notifier_join
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    {
        let _ = join.join();
    }
    let joins = pool
        .joins
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
        .unwrap_or_default();
    for join in joins {
        let _ = join.join();
    }
    while pool.shared.injector.pop().is_some() {}

    POOL_PTR.store(core::ptr::null_mut(), Ordering::Release);
    // SAFETY: all producers were excluded by lane closure before shutdown,
    // every worker is joined, and the injector no longer owns a sink.
    drop(unsafe { Box::from_raw(ptr) });
    POOL_STATE.store(POOL_STOPPED, Ordering::Release);
}

fn notifier_loop(shared: &Shared, workers: &[Thread]) {
    let mut observed = NOTIFY_EPOCH.load(Ordering::Acquire);
    let mut first_pass = true;
    loop {
        thread::park_timeout(NOTIFIER_INTERVAL);
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        let current = NOTIFY_EPOCH.load(Ordering::Acquire);
        let retry_hint = RETRY_PENDING.swap(false, Ordering::AcqRel);
        let (retried, retry_remaining) = if retry_hint {
            retry_registered(shared)
        } else {
            (0, false)
        };
        if retry_remaining {
            RETRY_PENDING.store(true, Ordering::Release);
        }
        if first_pass || retried != 0 || current != observed {
            first_pass = false;
            observed = current;
            wake_ready_workers(shared, workers);
        }
    }
}

fn wake_ready_workers(shared: &Shared, workers: &[Thread]) {
    let count = shared.injector.len().min(workers.len());
    if count == 0 {
        return;
    }
    let start = shared.next_worker.fetch_add(count, Ordering::Relaxed);
    for offset in 0..count {
        workers[(start.wrapping_add(offset)) % workers.len()].unpark();
    }
}

fn worker_loop(shared: &Shared, notifier_fallback: bool) {
    let mut deferred = VecDeque::new();
    let mut prefer_injector = true;
    loop {
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        let next_sink = if prefer_injector {
            shared.injector.pop().or_else(|| deferred.pop_front())
        } else {
            deferred.pop_front().or_else(|| shared.injector.pop())
        };
        if let Some(sink) = next_sink {
            prefer_injector = !prefer_injector;
            loop {
                if shared.stopping.load(Ordering::Acquire) {
                    return;
                }
                if let Some(executing) = begin_execution(shared) {
                    let outcome = sink.drain();
                    drop(executing);
                    match outcome {
                        DrainOutcome::Done => {}
                        DrainOutcome::Deferred => deferred.push_back(sink),
                        DrainOutcome::Busy => {
                            deferred.push_back(sink);
                            prefer_injector = true;
                            if shared.injector.is_empty() {
                                thread::park_timeout(BUSY_RETRY_TIMEOUT);
                            }
                        }
                    }
                    break;
                }
                // Keep ownership of this injector entry while a reversible
                // pause is active. On abort it runs normally; on shutdown it
                // drops only after the stopping flag is published.
                thread::park_timeout(PARK_TIMEOUT);
            }
            continue;
        }
        if shared.stopping.load(Ordering::Acquire) {
            return;
        }
        // A notifier-start failure still has a bounded, non-spinning fallback:
        // one designated worker retries registered lanes every millisecond.
        let retry_hint = RETRY_PENDING.swap(false, Ordering::AcqRel);
        let (retried, retry_remaining) = if retry_hint {
            retry_registered(shared)
        } else {
            (0, false)
        };
        if retry_remaining {
            RETRY_PENDING.store(true, Ordering::Release);
        }
        if retried != 0 || !shared.injector.is_empty() {
            continue;
        }
        let timeout = if notifier_fallback && !shared.notifier_running.load(Ordering::Acquire) {
            NOTIFIER_INTERVAL
        } else {
            PARK_TIMEOUT
        };
        thread::park_timeout(timeout);
    }
}

/// Enqueue a ready sink without waking the OS. The caller performs only the
/// bounded lock-free push and an atomic epoch publication; a non-realtime
/// notifier owns worker wake syscalls and missed-injector retries.
fn schedule(sink: Arc<dyn Drain>) -> bool {
    // Pool startup is never lazy here: static and hot shells warm off-thread
    // before processing. A missing/stopping pool rejects instead of creating
    // threads, allocating, locking, or logging on the caller.
    let Some(pool) = running_pool() else {
        return false;
    };
    if pool.workers.is_empty() || pool.shared.stopping.load(Ordering::Acquire) {
        return false;
    }
    if pool.shared.injector.push(sink).is_err() {
        return false;
    }
    true
}

fn notify_scheduler() {
    NOTIFY_EPOCH.fetch_add(1, Ordering::Release);
}

/// A cheap-to-clone handle for scheduling background tasks onto the
/// shared pool. Held by the shell and handed to the plugin through
/// [`InitContext`], `ProcessContext`, and the editor's `PluginContext`.
///
/// A spawner built with [`Self::new`] may run its handler concurrently
/// with itself for one instance (see the module's Concurrency section);
/// [`Self::new_serialized`] runs it one at a time.
pub struct TaskSpawner<T: Send + 'static> {
    sink: Arc<Sink<T>>,
}

impl<T: Send + 'static> Clone for TaskSpawner<T> {
    fn clone(&self) -> Self {
        Self {
            sink: Arc::clone(&self.sink),
        }
    }
}

impl<T: Send + 'static> TaskSpawner<T> {
    /// Register an instance's handler with the shared pool. `run` is the
    /// monomorphized `move |task| task.run(&params)`, built once
    /// by the shell. Static and hot shells warm the owning module's pool
    /// off-thread before this spawner can reach `process`.
    ///
    /// The handler may run concurrently with itself for one instance; use
    /// [`Self::new_serialized`] for a handler that isn't reentrancy-safe.
    pub fn new(run: impl Fn(T) + Send + Sync + 'static) -> Self {
        Self::with_mode(
            move |task| {
                run(task);
                None
            },
            false,
        )
    }

    /// Like [`Self::new`], but the pool runs the handler for a given
    /// instance one at a time ("one-slot" mode). The shell selects this
    /// when the plugin's `BackgroundTask::SERIALIZED` is `true`.
    pub fn new_serialized(run: impl Fn(T) + Send + Sync + 'static) -> Self {
        Self::with_mode(
            move |task| {
                run(task);
                None
            },
            true,
        )
    }

    /// Register a handler that may return its task as one fair continuation.
    /// The pool owns that tail, runs one bounded pass per injector turn, and
    /// cancels it when the instance lane closes. Tail storage is preallocated
    /// to [`TASK_QUEUE_PREALLOC`]; overflow drops the additional tail.
    pub fn new_managed(run: impl Fn(T) -> Option<T> + Send + Sync + 'static) -> Self {
        Self::with_mode(run, false)
    }

    /// Serialized form of [`Self::new_managed`].
    pub fn new_managed_serialized(run: impl Fn(T) -> Option<T> + Send + Sync + 'static) -> Self {
        Self::with_mode(run, true)
    }

    fn with_mode(run: impl Fn(T) -> Option<T> + Send + Sync + 'static, serialized: bool) -> Self {
        let sink = Arc::new(Sink {
            queue: ArrayQueue::new(TASK_QUEUE_PREALLOC),
            coalesced: ArrayQueue::new(1),
            continuation: ArrayQueue::new(TASK_QUEUE_PREALLOC),
            prefer_continuation: AtomicBool::new(true),
            scheduled: AtomicBool::new(false),
            retry_requested: AtomicBool::new(false),
            serialized,
            draining: AtomicBool::new(false),
            schedule_state: AtomicUsize::new(0),
            run: Box::new(run),
        });
        let erased = Arc::clone(&sink) as Arc<dyn Drain>;
        register_lane(&erased);
        Self { sink }
    }

    /// Enqueue a task, running it on the pool as soon as a worker is
    /// free. Wait-free. Returns `Err(task)` if the inbound queue is full
    /// (the audio thread decides what to do - drop, or coalesce via
    /// [`Self::spawn_coalescing`] - rather than block).
    ///
    /// # Errors
    ///
    /// Returns the task back when the preallocated inbound queue is full.
    pub fn try_spawn(&self, task: T) -> Result<(), T> {
        let Some(_scheduling) = self.sink.begin_schedule() else {
            return Err(task);
        };
        self.sink.queue.push(task)?;
        self.arm();
        Ok(())
    }

    /// Post a task into the single coalescing slot, replacing any
    /// still-unrun target. Wait-free, never rejects. Only the newest
    /// survives and the worker runs it at most once per drain, so a knob
    /// sweep that outruns the handler collapses to one execution, not one
    /// build per intermediate target. The displaced target drops on the
    /// caller (the audio thread on the hot path), so a coalescing task
    /// type should be cheap to drop - a small `Copy` request, not an
    /// owned buffer.
    pub fn spawn_coalescing(&self, task: T) {
        let Some(_scheduling) = self.sink.begin_schedule() else {
            return;
        };
        let _ = self.sink.coalesced.force_push(task);
        self.arm();
    }

    /// Inject this sink into the pool if it isn't already queued.
    fn arm(&self) {
        // Pairs with the SeqCst store + fence in `Sink::drain`: the caller
        // pushed the task just before this, and that push must be ordered
        // before the flag swap below, or the StoreLoad race described in
        // `drain` strands the task. The fence + SeqCst swap give the total
        // order that Release/AcqRel can't. Still wait-free (one barrier,
        // no lock/alloc/syscall), and `arm` runs at most once per block.
        fence(Ordering::SeqCst);
        if !self.sink.scheduled.swap(true, Ordering::SeqCst) {
            let sink: Arc<dyn Drain> = Arc::clone(&self.sink) as Arc<dyn Drain>;
            if !schedule(sink) {
                self.sink.scheduled.store(false, Ordering::SeqCst);
                self.sink.retry_requested.store(true, Ordering::Release);
                RETRY_PENDING.store(true, Ordering::Release);
            }
            notify_scheduler();
        }
    }
}

const LANE_CLOSED: usize = 1usize << (usize::BITS - 1);
const LANE_PRODUCER_MASK: usize = LANE_CLOSED - 1;

struct ScheduleGuard<'a>(&'a AtomicUsize);

impl Drop for ScheduleGuard<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

impl<T: Send + 'static> Sink<T> {
    fn begin_schedule(&self) -> Option<ScheduleGuard<'_>> {
        // One RMW is the admission linearization point. If it precedes
        // retirement's CLOSED fetch_or, retirement observes this producer in
        // the same atomic's modification order and waits for its decrement.
        // If it follows CLOSED, `previous` contains the bit and this producer
        // removes its count without touching either queue. Therefore queue
        // clearing after a zero-count observation cannot race a later push.
        // The low-half count cannot overflow in practice: that requires at
        // least 2^(usize::BITS-1) simultaneous scheduling callers.
        let previous = self.schedule_state.fetch_add(1, Ordering::AcqRel);
        let guard = ScheduleGuard(&self.schedule_state);
        (previous & LANE_CLOSED == 0).then_some(guard)
    }
}

trait RetireLane: Send + Sync {
    fn retire(&self, deadline: Instant) -> bool;
    fn resume(&self);
    fn close(&self);
}

impl<T: Send + 'static> RetireLane for Sink<T> {
    fn retire(&self, deadline: Instant) -> bool {
        self.schedule_state.fetch_or(LANE_CLOSED, Ordering::AcqRel);
        while self.schedule_state.load(Ordering::Acquire) & LANE_PRODUCER_MASK != 0
            && Instant::now() < deadline
        {
            thread::yield_now();
        }
        if self.schedule_state.load(Ordering::Acquire) & LANE_PRODUCER_MASK != 0 {
            self.resume();
            return false;
        }
        true
    }

    fn resume(&self) {
        // Preserve any post-close rejected producers still unwinding their
        // guard. Clearing only the bit makes abort atomic: old and new
        // producers may proceed, and no queue clear occurred if admission
        // itself timed out.
        self.schedule_state
            .fetch_and(LANE_PRODUCER_MASK, Ordering::AcqRel);
    }

    fn close(&self) {
        self.schedule_state.fetch_or(LANE_CLOSED, Ordering::AcqRel);
        while self.schedule_state.load(Ordering::Acquire) & LANE_PRODUCER_MASK != 0 {
            thread::yield_now();
        }
        while self.coalesced.pop().is_some() {}
        while self.queue.pop().is_some() {}
        while self.continuation.pop().is_some() {}
        self.retry_requested.store(false, Ordering::Release);
        self.scheduled.store(false, Ordering::SeqCst);
    }
}

/// One type-erased lane. The typed handle serves `downcast`; `control`
/// lets the reload watcher close a previous generation without knowing T.
struct ErasedLane {
    typed: Arc<dyn std::any::Any + Send + Sync>,
    control: Arc<dyn RetireLane>,
}

struct LaneSet(Box<[ErasedLane]>);

impl Drop for LaneSet {
    fn drop(&mut self) {
        self.close();
    }
}

impl LaneSet {
    fn retire(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        for lane in &self.0 {
            if !lane.control.retire(deadline) {
                // The current generation stays live when any lane misses
                // the one shared deadline. Re-open every lane already
                // closed above so a deferred reload cannot leave the live
                // plugin half-retired.
                for lane in &self.0 {
                    lane.control.resume();
                }
                return false;
            }
        }
        true
    }

    fn close(&self) {
        for lane in &self.0 {
            lane.control.close();
        }
    }
}

enum TaskLanes {
    /// Static builds and the hot logic's init/process path. Downcast is only
    /// an Arc clone and remains suitable for the audio thread.
    Fixed(Arc<LaneSet>),
    /// Stable wrapper/editor handle. Reload replaces this off-thread; hot
    /// process never downcasts through it.
    Routed(Arc<RwLock<Arc<LaneSet>>>),
}

/// A bundle of type-erased [`TaskSpawner`]s - one lane per declared task
/// type - so the concrete `ProcessContext` / `InitContext` (whose
/// signatures are fixed by the leaf trait and can't name the plugin's task
/// types) can carry every lane and hand back the right typed spawner on
/// demand via [`Self::downcast`]. Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct AnyTaskSpawner(Arc<TaskLanes>);

impl AnyTaskSpawner {
    /// Erase a single typed spawner into a one-lane bundle.
    #[must_use]
    pub fn new<T: Send + 'static>(spawner: &TaskSpawner<T>) -> Self {
        Self::from_lanes(vec![erased_lane(spawner.clone())])
    }

    /// Bundle several already-erased lanes (one per task type). The
    /// `plugin!` macro builds the lanes with [`TaskSpawnerBundle`].
    #[must_use]
    fn from_lanes(lanes: Vec<ErasedLane>) -> Self {
        Self(Arc::new(TaskLanes::Fixed(Arc::new(LaneSet(
            lanes.into_boxed_slice(),
        )))))
    }

    /// Create the stable GUI/wrapper route used by a hot-reload shell.
    /// Hot logic receives the current fixed generation directly, keeping
    /// process-thread lookup lock-free.
    #[must_use]
    pub fn routed() -> Self {
        Self(Arc::new(TaskLanes::Routed(Arc::new(RwLock::new(
            Arc::new(LaneSet(Box::new([]))),
        )))))
    }

    /// Route future off-thread lookups to a verified logic generation.
    /// The loader retires the previous fixed generation once before this
    /// swap; this method only changes the route and never waits.
    pub fn replace_with(&self, next: &Self) -> bool {
        let TaskLanes::Routed(route) = self.0.as_ref() else {
            return false;
        };
        let TaskLanes::Fixed(next) = next.0.as_ref() else {
            return false;
        };
        let mut active = route
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = Arc::clone(next);
        true
    }

    /// Disconnect a hot shell's stable off-thread route. The loader owns
    /// retirement; clearing a route is only a non-waiting pointer swap.
    pub fn clear_route(&self) -> bool {
        let TaskLanes::Routed(route) = self.0.as_ref() else {
            return false;
        };
        let mut active = route
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active = Arc::new(LaneSet(Box::new([])));
        true
    }

    /// Retire the active generation without installing a replacement.
    /// Existing typed handles reject new work. Queued and running work stays
    /// intact until the loader separately quiesces the owning worker pool, so
    /// an aborted reload can resume this generation without losing tasks.
    pub fn retire(&self, timeout: Duration) -> bool {
        match self.0.as_ref() {
            TaskLanes::Fixed(lanes) => lanes.retire(timeout),
            TaskLanes::Routed(route) => route
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .retire(timeout),
        }
    }

    /// Re-open lanes after a reload aborts. No-op for a routed handle's empty
    /// generation and never used after permanent close.
    pub fn resume(&self) {
        match self.0.as_ref() {
            TaskLanes::Fixed(lanes) => {
                for lane in &lanes.0 {
                    lane.control.resume();
                }
            }
            TaskLanes::Routed(route) => {
                let lanes = route
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                for lane in &lanes.0 {
                    lane.control.resume();
                }
            }
        }
    }

    /// Permanently close all lanes and wait only for admitted scheduling
    /// calls to leave their bounded queue/injection section. Loader teardown
    /// follows this with off-thread worker shutdown/join.
    pub fn close(&self) {
        match self.0.as_ref() {
            TaskLanes::Fixed(lanes) => lanes.close(),
            TaskLanes::Routed(route) => route
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .close(),
        }
    }

    /// Recover the typed spawner for task type `T`, or `None` if no lane of
    /// that type was declared. Lanes have distinct types, so at most one
    /// matches.
    #[must_use]
    pub fn downcast<T: Send + 'static>(&self) -> Option<TaskSpawner<T>> {
        let find = |lanes: &LaneSet| {
            lanes
                .0
                .iter()
                .find_map(|lane| lane.typed.downcast_ref::<TaskSpawner<T>>().cloned())
        };
        match self.0.as_ref() {
            TaskLanes::Fixed(lanes) => find(lanes),
            TaskLanes::Routed(route) => find(
                &route
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            ),
        }
    }
}

fn erased_lane<T: Send + 'static>(spawner: TaskSpawner<T>) -> ErasedLane {
    let control = Arc::clone(&spawner.sink) as Arc<dyn RetireLane>;
    ErasedLane {
        typed: Arc::new(spawner),
        control,
    }
}

/// Builder the `plugin!` macro uses to collect one lane per declared task
/// type into an [`AnyTaskSpawner`]. Kept separate so the macro never has to
/// name the erased-lane type.
#[derive(Default)]
pub struct TaskSpawnerBundle(Vec<ErasedLane>);

impl TaskSpawnerBundle {
    #[must_use]
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add one task type's spawner to the bundle.
    pub fn push<T: Send + 'static>(&mut self, spawner: TaskSpawner<T>) {
        self.0.push(erased_lane(spawner));
    }

    /// Finish: `Some` bundle, or `None` when no lanes were added (a plugin
    /// that declared no tasks), matching the `Option<AnyTaskSpawner>` the
    /// shell threads through.
    #[must_use]
    pub fn into_any(self) -> Option<AnyTaskSpawner> {
        if self.0.is_empty() {
            None
        } else {
            Some(AnyTaskSpawner::from_lanes(self.0))
        }
    }
}

/// Context handed to `init` so a plugin can schedule startup background
/// work before the first block. Concrete (not generic over the task
/// type) because `init`'s signature lives on the leaf trait; recover the
/// typed spawner with [`Self::tasks`]. Params arrive as the separate
/// `init` argument.
pub struct InitContext {
    tasks: Option<AnyTaskSpawner>,
    /// Handle for the off-thread snapshot lane (large state save), when
    /// the shell wired one. `None` in `--shell` hot-reload builds, where the
    /// off-thread snapshot path is not threaded across the dylib boundary.
    snapshot: Option<SnapshotPublisher>,
}

impl InitContext {
    #[must_use]
    pub fn new(tasks: Option<AnyTaskSpawner>) -> Self {
        Self {
            tasks,
            snapshot: None,
        }
    }

    /// Attach the off-thread snapshot publisher (see [`SnapshotPublisher`]).
    #[must_use]
    pub fn with_snapshot(mut self, snapshot: SnapshotPublisher) -> Self {
        self.snapshot = Some(snapshot);
        self
    }

    /// The task spawner for task type `T`, or `None` if the plugin declared
    /// no `tasks:` lane of that type on `plugin!`.
    #[must_use]
    pub fn tasks<T: Send + 'static>(&self) -> Option<TaskSpawner<T>> {
        self.tasks.as_ref().and_then(AnyTaskSpawner::downcast::<T>)
    }

    /// Handle to publish large custom state off the audio thread. Stash it
    /// in your DSP state and call `publish` from a background-task handler
    /// after your state changes. `None` in `--shell` builds; use
    /// `snapshot_into` for small state, which works everywhere.
    #[must_use]
    pub fn snapshot_publisher(&self) -> Option<SnapshotPublisher> {
        self.snapshot.clone()
    }
}

#[cfg(test)]
mod tests {
    // `TASK_QUEUE_PREALLOC` is 256, so casting it to `u32` for the loop
    // bounds is always exact.
    #![allow(clippy::cast_possible_truncation)]

    use super::*;
    use crate::snapshot::{SnapshotPublisher, SnapshotSlot};
    use std::sync::atomic::AtomicU32;
    use std::sync::{Condvar, Mutex};
    use std::time::Instant;

    #[test]
    fn init_context_exposes_snapshot_publisher() {
        let slot = SnapshotSlot::new();
        let cx = InitContext::new(None).with_snapshot(SnapshotPublisher::new(&slot));
        // The plugin captures this in `init` and publishes large state
        // through it off the audio thread.
        cx.snapshot_publisher()
            .expect("publisher present")
            .publish(vec![1, 2, 3]);
        assert_eq!(slot.read(), Some(vec![1, 2, 3]));
        // No snapshot wired (the `--shell` / no-slot case) yields None.
        assert!(InitContext::new(None).snapshot_publisher().is_none());
    }

    fn wait_until(deadline: Duration, mut done: impl FnMut() -> bool) -> bool {
        let start = Instant::now();
        while start.elapsed() < deadline {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        done()
    }

    /// Blocking completion latch: the pool handler bumps it, the test
    /// blocks until a count is reached. Unlike the wall-clock `wait_until`,
    /// it has no deadline, so it stays deterministic under Miri, whose
    /// interpreter can't run background tasks within a real-time budget.
    /// A `Mutex`/`Condvar` (not an `mpsc::Sender`, which is `!Sync`) keeps
    /// the handler `Fn + Send + Sync`.
    #[derive(Default)]
    struct Latch {
        ran: Mutex<u32>,
        woke: Condvar,
    }

    impl Latch {
        fn bump(&self) {
            *self.ran.lock().unwrap() += 1;
            self.woke.notify_all();
        }

        fn wait_for(&self, target: u32) {
            let mut ran = self.ran.lock().unwrap();
            while *ran < target {
                ran = self.woke.wait(ran).unwrap();
            }
        }
    }

    #[test]
    fn warm_pool_starts_workers_and_is_idempotent() {
        // Warming off the audio thread is what keeps the first
        // audio-thread schedule from cold-starting the workers inline.
        warm_pool();
        warm_pool();
        assert!(
            !running_pool().expect("pool running").workers.is_empty(),
            "warming spawns at least one worker"
        );
    }

    #[test]
    fn pin_current_module_is_safe_and_idempotent() {
        // Smoke: pinning must never panic or crash. In the test binary
        // the loader query resolves to the harness executable (never
        // unloaded anyway); we only assert the FFI is benign and can run
        // more than once.
        super::pin_current_module();
        super::pin_current_module();
    }

    #[test]
    fn runs_scheduled_tasks_off_thread() {
        warm_pool(); // never lazy; Miri runs tests one at a time
        let latch = Arc::new(Latch::default());
        let sum = Arc::new(AtomicU32::new(0));
        let (l, s) = (Arc::clone(&latch), Arc::clone(&sum));
        let spawner = TaskSpawner::<u32>::new(move |n| {
            s.fetch_add(n, Ordering::Relaxed);
            l.bump();
        });

        for n in 1..=10 {
            spawner.try_spawn(n).expect("queue has room");
        }

        // Block until all ten ran. The latch mutex orders every handler's
        // `sum` write before the read below, so the Relaxed sum is exact.
        latch.wait_for(10);
        assert_eq!(sum.load(Ordering::Relaxed), 55, "all ten tasks ran");
    }

    #[test]
    fn full_queue_returns_the_task() {
        // Handler blocks on a gate so the queue can actually fill.
        let gate = Arc::new(AtomicBool::new(false));
        let g = Arc::clone(&gate);
        let spawner = TaskSpawner::<u32>::new(move |_| {
            while !g.load(Ordering::Acquire) {
                thread::sleep(Duration::from_millis(1));
            }
        });

        // First task is picked up and blocks a worker; fill the rest.
        let mut rejected = 0u32;
        for n in 0..(TASK_QUEUE_PREALLOC as u32 + 64) {
            if spawner.try_spawn(n).is_err() {
                rejected += 1;
            }
        }
        assert!(rejected > 0, "a full inbound queue rejects further tasks");
        gate.store(true, Ordering::Release);
    }

    #[test]
    fn panicking_task_does_not_kill_the_worker() {
        warm_pool(); // never lazy; Miri runs tests one at a time
        let latch = Arc::new(Latch::default());
        let l = Arc::clone(&latch);
        let spawner = TaskSpawner::<bool>::new(move |should_panic| {
            assert!(!should_panic, "intentional panic, caught by the pool");
            l.bump();
        });
        spawner.try_spawn(true).expect("queue has room"); // panics in the handler
        spawner.try_spawn(false).expect("queue has room"); // must still run
        // If the panic had killed the worker, the survivor never runs and
        // this blocks forever - surfaced as a hung test, not a false pass.
        latch.wait_for(1);
    }

    #[test]
    fn coalescing_never_rejects() {
        warm_pool(); // never lazy; Miri runs tests one at a time
        let last = Arc::new(AtomicU32::new(0));
        let l = Arc::clone(&last);
        let spawner = TaskSpawner::<u32>::new(move |n| {
            l.store(n, Ordering::Relaxed);
        });
        for n in 0..(TASK_QUEUE_PREALLOC as u32 * 4) {
            spawner.spawn_coalescing(n); // never panics, never blocks
        }
        let target = TASK_QUEUE_PREALLOC as u32 * 4 - 1;
        assert!(
            wait_until(Duration::from_secs(2), || last.load(Ordering::Relaxed)
                == target),
            "the newest task always runs"
        );
    }

    #[test]
    fn serialized_runs_one_at_a_time_and_drops_nothing() {
        // One-slot mode: the handler must never run concurrently with
        // itself for this instance, and every FIFO task must still run.
        const N: u32 = 64;
        warm_pool(); // never lazy; Miri runs tests one at a time
        let in_flight = Arc::new(AtomicU32::new(0));
        let peak = Arc::new(AtomicU32::new(0));
        let latch = Arc::new(Latch::default());
        let (inf, pk, l) = (
            Arc::clone(&in_flight),
            Arc::clone(&peak),
            Arc::clone(&latch),
        );
        let spawner = TaskSpawner::<u32>::new_serialized(move |_| {
            let now = inf.fetch_add(1, Ordering::AcqRel) + 1;
            pk.fetch_max(now, Ordering::AcqRel);
            // Widen the window so a second worker would overlap if the guard
            // let it - a bare increment could hide a real race.
            thread::sleep(Duration::from_millis(1));
            inf.fetch_sub(1, Ordering::AcqRel);
            l.bump();
        });

        // Push across a burst so re-arms land mid-drain: each one re-injects
        // the sink and tempts an idle worker to pick it up concurrently.
        for n in 0..N {
            while spawner.try_spawn(n).is_err() {
                thread::sleep(Duration::from_millis(1));
            }
        }

        // Blocks until all N ran; a stranded task would hang here (a hung
        // test, not a false pass).
        latch.wait_for(N);
        assert_eq!(
            peak.load(Ordering::Acquire),
            1,
            "serialized: at most one handler in flight at a time"
        );
    }

    #[test]
    fn coalescing_collapses_to_the_newest() {
        let runs = Arc::new(AtomicU32::new(0));
        let last = Arc::new(AtomicU32::new(0));
        let (r, l) = (Arc::clone(&runs), Arc::clone(&last));
        let spawner = TaskSpawner::<u32>::new(move |n| {
            r.fetch_add(1, Ordering::Relaxed);
            l.store(n, Ordering::Relaxed);
        });

        // Fill the coalescing slot repeatedly without arming the pool, so
        // the burst collapses in the slot rather than racing a worker.
        // Then drain once and confirm the whole burst ran a single time,
        // as the newest target.
        for n in 1..=1000 {
            let _ = spawner.sink.coalesced.force_push(n);
        }
        spawner.sink.drain();

        assert_eq!(runs.load(Ordering::Relaxed), 1, "the burst ran once");
        assert_eq!(last.load(Ordering::Relaxed), 1000, "and it was the newest");
    }
}

// Model-checked proof that the schedule/drain handshake can't strand a
// task under any thread interleaving. Run with:
//   cargo test -p moose-core --features loom loom
//
// loom can't see into crossbeam's `ArrayQueue`, so this models the
// protocol directly: a one-slot queue (`item`) plus the `scheduled` flag,
// driven through the exact SeqCst store / fence / swap sequence that
// `Sink::drain` and `TaskSpawner::arm` use. Weakening either side to
// Release/AcqRel (dropping the SeqCst or the fences) makes loom find the
// stranding interleaving; the version below passes.
#[cfg(all(test, feature = "loom"))]
mod loom_tests {
    use loom::sync::Arc;
    use loom::sync::atomic::{AtomicBool, Ordering, fence};
    use loom::thread;

    #[test]
    fn schedule_drain_never_strands_a_task() {
        loom::model(|| {
            // Start with a drain in flight: the sink was scheduled
            // (`flag == true`) and a worker is about to drain an empty
            // queue, concurrent with a producer pushing one more task.
            let flag = Arc::new(AtomicBool::new(true));
            let item = Arc::new(AtomicBool::new(false));

            let (f, i) = (flag.clone(), item.clone());
            let worker = thread::spawn(move || {
                // `Sink::drain`: clear the flag, then check the queue. The
                // presence check must be a plain load (a `pop` reading
                // empty) - an RMW would always read the latest value in
                // modification order and so hide the StoreLoad staleness
                // this test exists to catch.
                f.store(false, Ordering::SeqCst);
                fence(Ordering::SeqCst);
                if i.load(Ordering::Acquire) {
                    i.store(false, Ordering::Release); // popped it
                }
            });

            // `try_spawn` + `arm`: push the task, then flag the sink.
            item.store(true, Ordering::Release);
            fence(Ordering::SeqCst);
            let was_scheduled = flag.swap(true, Ordering::SeqCst);
            // `was_scheduled == false` => the producer injects a fresh
            // drain (it set `flag = true`). `true` => it relies on the
            // in-flight drain to pick the task up.
            let _ = was_scheduled;

            worker.join().unwrap();

            // Safe end states: the queue is empty (some drain popped it),
            // or a drain is still scheduled (`flag == true`) to pick it up.
            // A pending task with `flag == false` is the stranding bug.
            let pending = item.load(Ordering::SeqCst);
            let scheduled = flag.load(Ordering::SeqCst);
            assert!(
                !pending || scheduled,
                "task stranded: pending with scheduled == false"
            );
        });
    }

    // The serialized ("one-slot") path adds a `draining` guard for mutual
    // exclusion. The no-stranding signal stays the `scheduled` handshake: a
    // worker that loses the guard is inert, and the winner re-checks the
    // *flag* (not the queue) after each drain, looping until it reads
    // `false`. This models that body for two workers plus a producer and
    // asserts the same invariant. Re-checking `item` instead of the flag
    // (an unsynchronized queue read) makes loom find the interleaving where
    // the winner misses the producer's task and it strands.
    #[test]
    fn serialized_drain_never_strands_a_task() {
        loom::model(|| {
            // A sink already scheduled with one queued task; two workers pop
            // it (the producer's re-inject can hand it to a second worker),
            // and the producer pushes one more task concurrently.
            let scheduled = Arc::new(AtomicBool::new(true));
            let item = Arc::new(AtomicBool::new(true));
            let draining = Arc::new(AtomicBool::new(false));

            // One execution of the serialized `Sink::drain` path. The
            // re-check loop is bounded to two passes - enough for the one
            // extra task a single producer can push.
            let worker =
                |scheduled: Arc<AtomicBool>, item: Arc<AtomicBool>, draining: Arc<AtomicBool>| {
                    if draining.swap(true, Ordering::Acquire) {
                        return; // loser: inert
                    }
                    for _ in 0..2 {
                        // drain_queues: clear the flag (SeqCst), fence, pop.
                        scheduled.store(false, Ordering::SeqCst);
                        fence(Ordering::SeqCst);
                        let _ = item.swap(false, Ordering::AcqRel);
                        draining.store(false, Ordering::Release);
                        fence(Ordering::SeqCst);
                        // Re-check the flag, not the queue.
                        if !scheduled.load(Ordering::SeqCst) {
                            return;
                        }
                        if draining.swap(true, Ordering::Acquire) {
                            return;
                        }
                    }
                };

            let (s1, i1, d1) = (scheduled.clone(), item.clone(), draining.clone());
            let w1 = thread::spawn(move || worker(s1, i1, d1));
            let (s2, i2, d2) = (scheduled.clone(), item.clone(), draining.clone());
            let w2 = thread::spawn(move || worker(s2, i2, d2));

            // `try_spawn` + `arm`: push the task, then flag the sink.
            item.store(true, Ordering::Release);
            fence(Ordering::SeqCst);
            let _ = scheduled.swap(true, Ordering::SeqCst);

            w1.join().unwrap();
            w2.join().unwrap();

            // Same invariant: no task left pending unless the flag is still
            // set for a future drain to pick it up.
            let pending = item.load(Ordering::SeqCst);
            let is_scheduled = scheduled.load(Ordering::SeqCst);
            assert!(
                !pending || is_scheduled,
                "serialized task stranded: pending with scheduled == false"
            );
        });
    }
}
