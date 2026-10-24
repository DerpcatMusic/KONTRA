//! Rust side of the Signalsmith shim (`candidates/signalsmith-dsp/shim.cpp`). Bench only.
#![allow(unsafe_code)]

use crate::block::Block;

unsafe extern "C" {
    fn sig_bq_new() -> *mut std::ffi::c_void;
    fn sig_bq_free(p: *mut std::ffi::c_void);
    fn sig_bq_set(p: *mut std::ffi::c_void, shape: i32, design: i32, scaled_freq: f64, db: f64, q: f64);
    fn sig_bq_tick(p: *mut std::ffi::c_void, x: f64) -> f64;
    fn sig_bq_reset(p: *mut std::ffi::c_void);
    fn sig_os_new(max_block: i32) -> *mut std::ffi::c_void;
    fn sig_os_free(p: *mut std::ffi::c_void);
    fn sig_os_latency(p: *mut std::ffi::c_void) -> i32;
    fn sig_os_reset(p: *mut std::ffi::c_void);
    fn sig_os_up(p: *mut std::ffi::c_void, input: *const f64, n: i32);
    fn sig_os_buffer(p: *mut std::ffi::c_void) -> *mut f64;
    fn sig_os_down(p: *mut std::ffi::c_void, out: *mut f64, n: i32);
}

/// Signalsmith `BiquadStatic`: a bell (peak) or shelf section. `design`: 0 Vicanek, 1 cookbook.
pub struct SigBiquad {
    raw: *mut std::ffi::c_void,
    rate: f64,
    shape: i32,
    design: i32,
    hz: f64,
    db: f64,
    q: f64,
}

impl SigBiquad {
    pub fn new(rate: f64, shape: i32, design: i32, hz: f64, db: f64, q: f64) -> Self {
        // SAFETY: `sig_bq_new` returns an owned heap object, freed in `Drop`.
        let raw = unsafe { sig_bq_new() };
        let mut s = Self { raw, rate, shape, design, hz, db, q };
        s.configure();
        s
    }

    fn configure(&mut self) {
        // SAFETY: `raw` is live for the life of `self`.
        unsafe { sig_bq_set(self.raw, self.shape, self.design, self.hz / self.rate, self.db, self.q) };
    }
}

impl Drop for SigBiquad {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `sig_bq_new` and is freed exactly once.
        unsafe { sig_bq_free(self.raw) };
    }
}

impl Block for SigBiquad {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        for (y, &x) in out.iter_mut().zip(input) {
            // SAFETY: as above.
            *y = unsafe { sig_bq_tick(self.raw, f64::from(x)) } as f32;
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "hz" => self.hz = value,
            "db" => self.db = value,
            "q" => self.q = value,
            _ => return false,
        }
        self.configure();
        true
    }
    fn reset(&mut self) {
        // SAFETY: as above.
        unsafe { sig_bq_reset(self.raw) };
    }
}

/// Signalsmith `Oversampler2xFIR` around a fixed nonlinearity: up, `tanh(drive x)`, down.
pub struct SigOversampler {
    raw: *mut std::ffi::c_void,
    drive: f64,
    up: Vec<f64>,
}

impl SigOversampler {
    pub fn new(max_block: usize, drive: f64) -> Self {
        // SAFETY: owned object, freed in `Drop`.
        let raw = unsafe { sig_os_new(max_block as i32) };
        Self { raw, drive, up: Vec::new() }
    }
    pub fn latency(&self) -> usize {
        // SAFETY: as above.
        unsafe { sig_os_latency(self.raw) as usize }
    }
}

impl Drop for SigOversampler {
    fn drop(&mut self) {
        // SAFETY: freed exactly once.
        unsafe { sig_os_free(self.raw) };
    }
}

impl Block for SigOversampler {
    fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let n = input.len();
        let x: Vec<f64> = input.iter().map(|&v| f64::from(v)).collect();
        // SAFETY: `up` writes 2n samples into the shim's buffer; we read/write exactly 2n of them.
        unsafe {
            sig_os_up(self.raw, x.as_ptr(), n as i32);
            let buf = std::slice::from_raw_parts_mut(sig_os_buffer(self.raw), 2 * n);
            for v in buf.iter_mut() {
                *v = (self.drive * *v).tanh() / self.drive;
            }
        }
        self.up.clear();
        self.up.resize(n, 0.0);
        // SAFETY: `down` writes n samples into `up`.
        unsafe { sig_os_down(self.raw, self.up.as_mut_ptr(), n as i32) };
        for (y, v) in out.iter_mut().zip(&self.up) {
            *y = *v as f32;
        }
    }
    fn set(&mut self, key: &str, value: f64) -> bool {
        match key {
            "drive" => self.drive = value,
            _ => return false,
        }
        true
    }
    fn reset(&mut self) {
        // SAFETY: as above.
        unsafe { sig_os_reset(self.raw) };
    }
}
