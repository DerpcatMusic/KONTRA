//! Test-only allocator witness for the experimental HQ wrapper.
//! Uses original source modules through #[path], never copies/rebuilds another repo.
//! Counting allocator lives in the existing integration support, outside core forbid(unsafe).
#[path = "../src/dsp/kaiser.rs"]
pub(crate) mod original_kaiser;
mod dsp {
    pub(crate) use crate::original_kaiser as kaiser;
}
#[path = "../src/dsp/filter/ladder_kernel.rs"]
mod original_ladder;
mod support;

#[test]
fn ladder_research_wrapper_callback_counts_zero_heap_work() {
    original_ladder::wrapper_research::callback_storage_case(|callback| {
        support::without_heap(|| callback());
    });
}
