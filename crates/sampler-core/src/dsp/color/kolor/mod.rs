//! Kontakt Kolor (`BEffectKolor`, module 0x79): the shaper stage and its interface.
//!
//! The per-block process `14197a2c0` calls the shaper dispatch `1419749f0` on the
//! object at `param + 0x800 + (mode*2 + k) * 0x700`. The dispatch selects a case by
//! the object's type byte (`+0x6f0`, cases 0..9). Each case is one lane here:
//!
//! - [`chain`]: the dry path (per-channel gain ramp). Matches the native vectors for the
//!   shaper-free modes 0, 1, 3, 4, 6..9. Modes 2 and 5 still need the post-shaper sections.
//! - [`shaper_2_5`]: cases 2 and 5, stateless, decoded and tested.
//! - cases 0, 1, 3, 4, 6..9: not ported yet, owned by other lanes.
//!
//! Status: partial. The control tick, prepare and biquad sections are open (see
//! `docs/re/engine/k-effects-batch-a.md`), so modes 2 and 5 of
//! `tools/re/fixtures/kolor_native_vectors.json` do not match yet.

pub mod chain;
pub mod shaper_2_5;

/// One Kolor shaper case: a sample in, a sample out. Stateless cases ignore `&mut self`.
pub trait KolorShaper {
    fn process(&mut self, x: f32) -> f32;
}
