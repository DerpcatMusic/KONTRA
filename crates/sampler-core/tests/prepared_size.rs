//! `Prepared` moves by value through every `with_*` builder; in a debug build
//! each call's result gets its own stack slot, so an inline-heavy `Prepared`
//! made `bind_modules` reserve a 573 KB frame and overflow a 2 MB thread.
//! Keep large tables boxed.

#[test]
fn a_prepared_plan_stays_small_by_value() {
    let size = std::mem::size_of::<sampler_core::Prepared>();
    assert!(size <= 4096, "Prepared is {size} bytes by value");
}
