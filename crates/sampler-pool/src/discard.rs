//! v1 0cb7a8a0:src/engine/stream.rs Slot::reclaim, adapted to exclusive buffers.
#![allow(unsafe_code, reason = "discard only whole pages inside exclusively borrowed f32 payloads")]

/// Worker/control only: release physical pages inside unused sample storage.
/// Released values read as zero; unaligned edges retain their values. Returns
/// the released byte count, or zero on unsupported platforms/system refusal.
pub fn discard_f32(samples: &mut [f32]) -> usize {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: sysconf has no preconditions.
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
        if page <= 0 { return 0; }
        let page = page as usize;
        let start = samples.as_mut_ptr() as usize;
        let Some(end) = start.checked_add(size_of_val(samples)) else { return 0; };
        let Some(lo) = start.checked_next_multiple_of(page) else { return 0; };
        let hi = end / page * page;
        // SAFETY: exclusive borrow; whole pages lie within the sample payload,
        // excluding allocator metadata/neighbors. All-zero bits are valid f32.
        if hi > lo && unsafe { libc::madvise(lo as *mut _, hi - lo, libc::MADV_DONTNEED) } == 0 {
            hi - lo
        } else { 0 }
    }
    #[cfg(not(target_os = "linux"))]
    { let _ = samples; 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn discard_preserves_neighbors_and_unaligned_edges() {
        let mut storage = vec![0.75f32; 65536];
        let start = storage.as_ptr() as usize + 13 * size_of::<f32>();
        let released = discard_f32(&mut storage[13..65517]);
        assert!(storage[..13].iter().chain(&storage[65517..]).all(|&x| x == 0.75));
        if released > 0 {
            assert!(storage[13..65517].iter().any(|&x| x == 0.));
            let first_zero = storage.iter().position(|&x| x == 0.).unwrap();
            let last_zero = storage.iter().rposition(|&x| x == 0.).unwrap();
            assert!(storage[..first_zero].iter().chain(&storage[last_zero + 1..]).all(|&x| x == 0.75));
            assert_eq!((last_zero - first_zero + 1) * size_of::<f32>(), released);
            assert!(storage[first_zero..=last_zero].iter().all(|&x| x == 0.));
            assert!((storage.as_ptr() as usize + first_zero * size_of::<f32>()) >= start);
        }
        assert_eq!(discard_f32(&mut []), 0);
        assert_eq!(discard_f32(&mut storage[..1]), 0);
        storage.fill(-0.5);
        assert!(storage.iter().all(|&x| x == -0.5));
    }
}
