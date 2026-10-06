#![allow(unsafe_code)]
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::*},
    },
    thread::{self, JoinHandle},
};

struct Shared {
    /// Run generation in the high half, next task index in the low half.
    ticket: AtomicU64,
    total: AtomicUsize,
    done: AtomicUsize,
    call: AtomicUsize,
    data: AtomicUsize,
    quit: AtomicBool,
    panicked: AtomicBool,
    sleeping: Box<[AtomicBool]>,
}

impl Shared {
    /// Claim and run tasks of run `generation` until none are left.
    fn work(&self, generation: u32) {
        loop {
            let t = self.ticket.load(Acquire);
            if (t >> 32) as u32 != generation {
                return;
            }
            if t as u32 as usize >= self.total.load(Relaxed) {
                return;
            }
            // Winning the claim means this run is still live (it cannot end
            // before this task is done), so `call` and `data` are its own.
            if self.ticket.compare_exchange_weak(t, t + 1, AcqRel, Acquire).is_err() {
                continue;
            }
            // SAFETY: `call` holds a `fn(usize, usize)` stored by `run` for this run.
            let call: fn(usize, usize) = unsafe { std::mem::transmute(self.call.load(Relaxed)) };
            let data = self.data.load(Relaxed);
            if catch_unwind(AssertUnwindSafe(|| call(data, t as u32 as usize))).is_err() {
                self.panicked.store(true, Relaxed);
            }
            self.done.fetch_add(1, Release);
        }
    }
}

pub struct Pool {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
    generation: u32,
}

impl Pool {
    /// A pool of `workers` parked threads; with 0, [`Pool::run`] runs inline.
    pub fn new(workers: usize) -> Self {
        let shared = Arc::new(Shared {
            ticket: AtomicU64::new(0),
            total: AtomicUsize::new(0),
            done: AtomicUsize::new(0),
            call: AtomicUsize::new(0),
            data: AtomicUsize::new(0),
            quit: AtomicBool::new(false),
            panicked: AtomicBool::new(false),
            sleeping: (0..workers).map(|_| AtomicBool::new(false)).collect(),
        });
        let workers = (0..workers)
            .map(|id| {
                let shared = shared.clone();
                thread::Builder::new()
                    .name(format!("sampler-render-{id}"))
                    .spawn(move || worker(&shared, id))
                    .expect("spawn render worker")
            })
            .collect();
        Self { shared, workers, generation: 0 }
    }

    /// Worker threads, not counting the caller.
    pub fn workers(&self) -> usize {
        self.workers.len()
    }

    /// Run `f(0..tasks)`, each index once, in any order and on any thread,
    /// and return when all are done. A panicking task is re-raised here.
    pub fn run<F: Fn(usize) + Sync>(&mut self, tasks: usize, f: &F) {
        if self.workers.is_empty() || tasks < 2 {
            (0..tasks).for_each(f);
            return;
        }
        fn call<F: Fn(usize)>(data: usize, i: usize) {
            // SAFETY: `data` is the `&F` that `run` published and keeps alive
            // until every task of the run is done.
            unsafe { (*(data as *const F))(i) }
        }
        let s = &*self.shared;
        s.total.store(tasks, Relaxed);
        s.done.store(0, Relaxed);
        s.call.store(call::<F> as fn(usize, usize) as usize, Relaxed);
        s.data.store(f as *const F as usize, Relaxed);
        self.generation = self.generation.wrapping_add(1);
        s.ticket.store(u64::from(self.generation) << 32, SeqCst);
        for (worker, handle) in self.workers.iter().take(tasks - 1).enumerate() {
            if s.sleeping[worker].load(SeqCst) {
                handle.thread().unpark();
            }
        }
        s.work(self.generation);
        let mut spins = 0u32;
        while s.done.load(Acquire) < tasks {
            if spins < 200 {
                spins += 1;
                std::hint::spin_loop();
            } else {
                thread::yield_now();
            }
        }
        if s.panicked.swap(false, Relaxed) {
            panic!("a render task panicked");
        }
    }
}

fn worker(s: &Shared, id: usize) {
    let mut seen = 0u32;
    loop {
        let generation = (s.ticket.load(Acquire) >> 32) as u32;
        if generation != seen {
            seen = generation;
            s.work(generation);
            continue;
        }
        if s.quit.load(Acquire) {
            return;
        }
        // Announce, then re-check, so a run published meanwhile is not slept
        // through (and an unpark that beats the park leaves a token).
        s.sleeping[id].store(true, SeqCst);
        if (s.ticket.load(SeqCst) >> 32) as u32 == seen && !s.quit.load(SeqCst) {
            thread::park();
        }
        s.sleeping[id].store(false, Relaxed);
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        self.shared.quit.store(true, SeqCst);
        for handle in &self.workers {
            handle.thread().unpark();
        }
        for handle in self.workers.drain(..) {
            let _ = handle.join();
        }
    }
}
