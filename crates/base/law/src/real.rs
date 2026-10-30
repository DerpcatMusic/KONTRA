//! The float types a law evaluates in: f32 for translators and the audio thread, f64 for the
//! engine's parameter round trips. One generic evaluator serves both.

use std::ops::{Add, Div, Mul, Neg, Sub};

/// A float a [`crate::Law`] can evaluate in.
pub(crate) trait Real:
    Copy
    + PartialOrd
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
{
    /// Widens (or keeps) a law's stored `f32` parameter.
    fn of(v: f32) -> Self;
    /// Converts a count, as `n as Self`.
    fn of_count(n: usize) -> Self;
    /// Widens to f64.
    fn to_f64(self) -> f64;
    /// Narrows to f32.
    fn to_f32(self) -> f32;
    /// Truncates toward zero to an index, saturating at 0.
    fn to_index(self) -> usize;
    fn powf(self, e: Self) -> Self;
    fn powi(self, n: i32) -> Self;
    fn exp(self) -> Self;
    fn exp2(self) -> Self;
    fn sin(self) -> Self;
    fn abs(self) -> Self;
    fn clamp(self, lo: Self, hi: Self) -> Self;
    /// π / 2.
    const FRAC_PI_2: Self;
}

macro_rules! real {
    ($t:ident) => {
        impl Real for $t {
            fn of(v: f32) -> Self {
                Self::from(v)
            }
            fn of_count(n: usize) -> Self {
                n as Self
            }
            fn to_f64(self) -> f64 {
                f64::from(self)
            }
            fn to_f32(self) -> f32 {
                self as f32
            }
            fn to_index(self) -> usize {
                self as usize
            }
            fn powf(self, e: Self) -> Self {
                $t::powf(self, e)
            }
            fn powi(self, n: i32) -> Self {
                $t::powi(self, n)
            }
            fn exp(self) -> Self {
                $t::exp(self)
            }
            fn exp2(self) -> Self {
                $t::exp2(self)
            }
            fn sin(self) -> Self {
                $t::sin(self)
            }
            fn abs(self) -> Self {
                $t::abs(self)
            }
            fn clamp(self, lo: Self, hi: Self) -> Self {
                $t::clamp(self, lo, hi)
            }
            const FRAC_PI_2: Self = std::$t::consts::FRAC_PI_2;
        }
    };
}

real!(f32);
real!(f64);
