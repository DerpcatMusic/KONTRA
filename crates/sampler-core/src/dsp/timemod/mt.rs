//! MT19937 and MSVC's `generate_canonical`, as UVI's delays draw them.

#[derive(Clone)]
pub(crate) struct Mt {
    pub(crate) s: [u32; 624],
    pub(crate) i: usize,
}

impl Mt {
    pub(crate) fn new(seed: u32) -> Self {
        let mut s = [0u32; 624];
        s[0] = seed;
        for i in 1..624 {
            s[i] = 1_812_433_253u32.wrapping_mul(s[i - 1] ^ (s[i - 1] >> 30)).wrapping_add(i as u32);
        }
        Self { s, i: 624 }
    }

    pub(crate) fn next(&mut self) -> u32 {
        if self.i >= 624 {
            for k in 0..624 {
                let y = (self.s[k] & 0x8000_0000) | (self.s[(k + 1) % 624] & 0x7fff_ffff);
                self.s[k] = self.s[(k + 397) % 624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.i = 0;
        }
        let mut y = self.s[self.i];
        self.i += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^ (y >> 18)
    }

    /// `generate_canonical<float, 24>`: one draw, `(float)u / 2^32`.
    pub(crate) fn unit_f32(&mut self) -> f32 {
        self.next() as f32 / 4_294_967_296.0
    }

    /// `generate_canonical<double, 53>`: two draws, low word first.
    pub(crate) fn unit_f64(&mut self) -> f64 {
        let a = f64::from(self.next());
        let b = f64::from(self.next());
        (a + b * 4_294_967_296.0) / 18_446_744_073_709_551_616.0
    }
}

#[cfg(test)]
mod tests {
    /// The C++ standard's check (10000th draw of seed 5489) and UVI's
    /// TapeEcho stream (seed 42: 3ebfc3b9, 3f4bea3e, 3f736203).
    #[test]
    fn matches_the_standard_and_native_draws() {
        let mut m = super::Mt::new(5489);
        let v = (0..10_000).map(|_| m.next()).last().unwrap();
        assert_eq!(v, 4_123_659_995);
        let mut m = super::Mt::new(42);
        assert_eq!([m.unit_f32(), m.unit_f32(), m.unit_f32()].map(f32::to_bits), [0x3ebfc3b9, 0x3f4bea3e, 0x3f736203]);
    }
}
