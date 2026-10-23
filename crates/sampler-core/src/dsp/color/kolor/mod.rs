//! Kontakt Kolor (`BEffectKolor`, module 0x79): the shaper stage and its interface.
//!
//! The per-block process `14197a2c0` calls the shaper dispatch `1419749f0` on the
//! object at `param + 0x800 + (mode*2 + k) * 0x700`. The dispatch selects a case by
//! the object's type byte (`+0x6f0`, cases 0..9). Each case is one lane here:
//!
//! - [`chain`]: the dry path (per-channel gain ramp). Matches the native vectors for the
//!   shaper-free modes 0, 1, 3, 4, 6..9. Modes 2 and 5 still need the post-shaper sections.
//! - [`shaper_2_5`]: cases 2 and 5, stateless, decoded and tested.
//! - [`shaper_4`]: case 4, output identically zero natively (state unobservable), tested against native vectors.
//! - cases 0, 1, 3, 6..9: not ported yet, owned by other lanes.
//!
//! `control` (`mode_from_knob`, `shaper_selector` = mode, the control tick) is native-verified; the
//! notify lambdas A and B (P/P2 blocks and phase) are ported in [`notify`] and native-verified. The
//! writer of the source block S is not traced yet, see `docs/re/engine/kolor-shaper-cases.md`.
//!
//! Status: partial. The control tick, prepare and biquad sections are open (see
//! `docs/re/engine/k-effects-batch-a.md`), so modes 2 and 5 of
//! `tools/re/fixtures/kolor_native_vectors.json` do not match yet.

pub mod chain;
pub mod control;
pub mod notify;
pub mod shaper_2_5;
pub mod shaper_4;
// unwired until the cfb0 state update (c830) is recovered; see docs/re/engine/kolor-shaper-cases.md
#[allow(dead_code)]
pub mod shaper_6_9;
#[allow(dead_code)]
pub mod shaper_0_1_3;

/// One Kolor shaper case: a sample in, a sample out. Stateless cases ignore `&mut self`.
pub trait KolorShaper {
    fn process(&mut self, x: f32) -> f32;
}
