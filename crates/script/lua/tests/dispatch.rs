//! What starting a callback costs the audio thread, through the counting
//! allocator: a control change, a connection's move and a note each take a
//! resident coroutine and pass their arguments on its first resume. The
//! allocator sees Luau's 16 KB pages, not its objects, so the exact check
//! is `starting_a_callback_makes_no_luau_object` in the crate.

use std::time::Instant;

use kontra_lua::{Callbacks, compile, probe};
use kontra_vm::{Foreign, HostApi, HostCall, HostValue, Machine, Step};

struct Host;

impl HostApi for Host {
    fn call(&mut self, _: HostCall<'_>) -> HostValue {
        HostValue::Int(1)
    }
}

/// A knob and a note callback that only store numbers: whatever allocates
/// is the dispatch, not the script.
const SOURCE: &str = "
    last = 0
    local k = Knob('k', 0, 0, 100)
    k.changed = function(self) last = self.value end
    function onNote(e) last = e.note end
";

const EVENTS: u32 = 20_000;
const ROUNDS: u32 = 5;

/// Allocations per event over every round after a warm-up, and the
/// fastest round's microseconds per event.
fn measure(
    machine: &mut Box<dyn Machine>,
    event: &mut dyn FnMut(&mut Box<dyn Machine>, u32),
) -> (f64, f64) {
    for i in 0..2000 {
        event(machine, i);
    }
    let _ = probe::take();
    let mut best = f64::MAX;
    for _ in 0..ROUNDS {
        let t = Instant::now();
        for i in 0..EVENTS {
            event(machine, i);
        }
        best = best.min(t.elapsed().as_secs_f64() * 1e6 / f64::from(EVENTS));
    }
    let seen = probe::take();
    (seen.allocs as f64 / f64::from(EVENTS * ROUNDS), best)
}

fn run(machine: &mut Box<dyn Machine>, co: Option<kontra_vm::CoId>) {
    assert_eq!(co.map(|co| machine.run(co, &mut Host)), Some(Step::Done));
}

/// Runs the script's init, then times each kind of event.
fn costs(mut m: Box<dyn Machine>) -> [(&'static str, f64, f64); 3] {
    let init = m.start(Callbacks::INIT, 0);
    run(&mut m, init);
    let (ca, cu) = measure(&mut m, &mut |m, i| {
        let co = m.control(0, f64::from(i % 100), 0);
        run(m, co);
    });
    let (ma, mu) = measure(&mut m, &mut |m, i| {
        let co = m.control_mod(0, 50.0, 0.5, f64::from(i % 3) - 1.0, 0);
        run(m, co);
    });
    let (na, nu) = measure(&mut m, &mut |m, _| {
        let co = m.start(Callbacks::NOTE, 0);
        run(m, co);
    });
    [
        ("control", ca, cu),
        ("control_mod", ma, mu),
        ("note", na, nu),
    ]
}

#[test]
fn control_changes_take_nothing_from_the_allocator() {
    let m = compile(SOURCE, &[], &[]).unwrap().machine();
    for (event, allocs, _) in costs(m).into_iter().take(2) {
        assert_eq!(allocs, 0.0, "{event} allocates per event");
    }
}

/// `cargo test --release -p kontra-lua --test dispatch -- --ignored --nocapture`
#[test]
#[ignore = "a timing, not a check"]
fn dispatch_cost() {
    let m = compile(SOURCE, &[], &[]).unwrap().machine();
    for (event, allocs, us) in costs(m) {
        println!("{event}: {allocs:.2} allocations, {us:.2} µs per event");
    }
}
