//! Translation of soothe3's gain body `0x8e04c0..0x8e53b1` and its callees.
//!
//! The body runs once per hop for each analyzer (`rcx` = DSP object + `0x3248`, the 512-point
//! analyzer, or + `0x600`, the 4096-point one). It windows the newest frame, takes its spectrum,
//! derives a per-bin reduction and writes `-ln(gain)` per bin into the caller's view (`rdx`).
//! Notes: `research/re-latency-fft-scale.md` section 21.
//!
//! Data model. The binary passes multichannel vectors as "views": a block of channel pointers
//! followed by the channel count and the length. Object views are 0x98 bytes (16 pointers,
//! channels at +0x80, length at +0x88); stack/arena views are 0x410 bytes (128 pointers,
//! channels at +0x400, length at +0x404, bytes at +0x408). Here a view is a slice of channel
//! vectors. Scratch the binary carves from its arena (`arg9`, bump cursor at +0x18) lives in
//! [`body::Scratch`], allocated once.
//!
//! IPP calls are in [`ipp`].

pub mod ipp;
