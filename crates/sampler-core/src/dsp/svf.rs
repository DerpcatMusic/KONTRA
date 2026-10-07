use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::{BLOCK, Planar, flush};
use crate::Error;

#[derive(Clone, Copy, Debug)]
pub enum SvfMode {
    LowPass,
    HighPass,
    /// Unity peak at the cutoff frequency.
    BandPass,
    Notch,
    AllPass,
    /// 6 dB/octave: `y += (x - y) b` with `b = 1 - exp(-2 pi f / rate)`; the
    /// high pass is `x - y`. Shares the SVF coefficient cache (`a3` holds `b`);
    /// Q is unused. Spec: DSP_FORMAT_SPECIFICATION.md, Workstation OnePole
    /// kernels; the exponent's multiplier is unresolved there, 2 pi is assumed.
    OnePoleLowPass,
    OnePoleHighPass,
}

/// Stereo trapezoidal state-variable filter. Smoothing occurs in hertz and Q,
/// before coefficient calculation; histories belong to the voice or summed bus.
#[derive(Clone, Copy, Debug)]
pub struct StateVariableFilter {
    pub mode: SvfMode,
    pub cutoff_hz: Parameter,
    pub q: Parameter,
}

impl StateVariableFilter {
    pub(super) fn valid(self) -> bool {
        self.cutoff_hz.valid() && self.q.valid()
    }

    pub(super) fn compile(
        self,
        rate: u32,
        bindings: &mut Vec<ControlRange>,
    ) -> Result<PreparedFilter, Error> {
        for hz in self.cutoff_hz.bounds() {
            for q in self.q.bounds() {
                if rate == 0 || hz <= 0. || hz >= f64::from(rate) * 0.5 || q <= 0. {
                    return Err(Error::InvalidInput);
                }
                let coefficients = Coefficients::new(self.mode, f64::from(rate), hz, q);
                let one = self.mode.one_pole();
                if ![
                    coefficients.a1,
                    coefficients.a2,
                    coefficients.a3,
                    coefficients.k,
                ]
                .iter()
                .all(|v| v.is_finite() && (*v > 0. || (one && *v == 0.)))
                {
                    return Err(Error::InvalidInput);
                }
            }
        }
        Ok(PreparedFilter {
            mode: self.mode,
            rate: f64::from(rate),
            cutoff: self.cutoff_hz.compile(bindings),
            q: self.q.compile(bindings),
        })
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedFilter {
    mode: SvfMode,
    rate: f64,
    cutoff: PreparedParameter,
    q: PreparedParameter,
}

impl PreparedFilter {
    pub fn requires_expression(self) -> bool {
        self.cutoff.requires_expression() || self.q.requires_expression()
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(super) struct Coefficients {
    pub a1: f64,
    pub a2: f64,
    pub a3: f64,
    pub k: f64,
}
impl Coefficients {
    fn new(mode: SvfMode, rate: f64, hz: f64, q: f64) -> Self {
        if mode.one_pole() {
            return Self {
                a1: 0.,
                a2: 0.,
                a3: -(-std::f64::consts::TAU * hz / rate).exp_m1(),
                k: 0.,
            };
        }
        let g = (std::f64::consts::PI * (hz / rate)).tan();
        let k = 1. / q;
        let a1 = 1. / (1. + g * (g + k));
        let a2 = g * a1;
        Self {
            a1,
            a2,
            a3: g * a2,
            k,
        }
    }
}

/// One coefficient window per authored filter, shared across its live voices.
/// Filled lazily for the frames a block actually renders, so unused chains
/// calculate nothing. The window is keyed by its absolute start frame; render
/// segmentation never presents more than [`BLOCK`] frames from one start.
pub(crate) struct FilterCache {
    filter: PreparedFilter,
    owner: Option<crate::ExpressionId>,
    start: Option<u64>,
    filled: usize,
    /// Every filled entry equals the first: the block runs on hoisted values.
    pub(super) uniform: bool,
    pub(super) coefficients: [Coefficients; BLOCK],
    last_values: Option<[f64; 2]>,
    last: Coefficients,
}
impl FilterCache {
    pub fn new(filter: PreparedFilter) -> Self {
        Self {
            filter,
            owner: None,
            start: None,
            filled: 0,
            uniform: true,
            coefficients: [Coefficients::default(); BLOCK],
            last_values: None,
            last: Coefficients::default(),
        }
    }

    pub(super) fn one_pole_high(&self) -> Option<bool> {
        self.filter.mode.one_pole_high()
    }

    /// The response's output mix: `[m0, mk, m2]`.
    pub(super) fn mix(&self) -> [f64; 3] {
        self.filter.mode.mix()
    }

    /// Coefficients for `len` frames from `at`, evaluated per frame at the
    /// parameter values of that frame. Unchanged values reuse the previous set.
    fn prepare(
        &mut self,
        at: u64,
        len: usize,
        parameters: &[ControlRamp],
        expression: Option<&crate::Expression>,
    ) {
        if self.start != Some(at) {
            self.start = Some(at);
            self.filled = 0;
            self.uniform = true;
        }
        while self.filled < len {
            let frame = at + self.filled as u64;
            let values = [
                self.filter.cutoff.value(parameters, frame, expression),
                self.filter.q.value(parameters, frame, expression),
            ];
            if self.last_values != Some(values) {
                self.last = Coefficients::new(self.filter.mode, self.filter.rate, values[0], values[1]);
                self.last_values = Some(values);
            }
            self.uniform &= self.filled == 0 || self.last == self.coefficients[0];
            self.coefficients[self.filled] = self.last;
            self.filled += 1;
        }
    }

    /// Filter `len` planar frames in place. The response's output mix is chosen
    /// once per block; per frame it is `m0 x + (mk k) band + m2 low`, which is
    /// exactly the named response for each mode (the factors are 0 and +-1, +-2).
    #[inline]
    pub fn process(
        &mut self,
        state: &mut [[f64; 2]; 2],
        block: &mut Planar,
        len: usize,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<&crate::Expression>,
    ) {
        self.prepare(at, len, parameters, expression);
        let mix = self.filter.mode.mix();
        if let Some(high) = self.filter.mode.one_pole_high() {
            let coefficients = &self.coefficients;
            let uniform = self.uniform;
            one_pole(state, block, len, high, |i| coefficients[if uniform { 0 } else { i }].a3);
        } else if self.uniform {
            let c = self.coefficients[0];
            run(state, block, len, mix, |_| c);
        } else {
            let coefficients = &self.coefficients;
            run(state, block, len, mix, |i| coefficients[i]);
        }
    }
}

/// `y += (x - y) b` per channel in `state[0]`; the band-state row stays zero.
#[inline(always)]
fn one_pole(
    state: &mut [[f64; 2]; 2],
    block: &mut Planar,
    len: usize,
    high: bool,
    b: impl Fn(usize) -> f64,
) {
    let [mut yl, mut yr] = state[0];
    let [left, right] = block;
    for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
        let b = b(i);
        yl += (*l - yl) * b;
        yr += (*r - yr) * b;
        (*l, *r) = if high { (*l - yl, *r - yr) } else { (yl, yr) };
    }
    state[0] = [yl, yr].map(flush);
}

#[inline(always)]
fn run(
    state: &mut [[f64; 2]; 2],
    block: &mut Planar,
    len: usize,
    [m0, mk, m2]: [f64; 3],
    coefficients: impl Fn(usize) -> Coefficients,
) {
    let [mut s0, mut s1] = *state;
    let [left, right] = block;
    for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
        let c = coefficients(i);
        let x = [*l, *r];
        let y: [f64; 2] = std::array::from_fn(|ch| {
            let (ic1, ic2) = (s0[ch], s1[ch]);
            let v3 = x[ch] - ic2;
            let band = c.a1 * ic1 + c.a2 * v3;
            let low = ic2 + c.a2 * ic1 + c.a3 * v3;
            s0[ch] = 2. * band - ic1;
            s1[ch] = 2. * low - ic2;
            m0 * x[ch] + (mk * c.k) * band + m2 * low
        });
        (*l, *r) = (y[0], y[1]);
    }
    *state = [s0, s1].map(|channels| channels.map(flush));
}

impl SvfMode {
    fn mix(self) -> [f64; 3] {
        match self {
            Self::LowPass => [0., 0., 1.],
            Self::HighPass => [1., -1., -1.],
            Self::BandPass => [0., 1., 0.],
            Self::Notch => [1., -1., 0.],
            Self::AllPass => [1., -2., 0.],
            Self::OnePoleLowPass => [0., 0., 1.],
            Self::OnePoleHighPass => [1., 0., -1.],
        }
    }

    pub(super) fn one_pole_high(self) -> Option<bool> {
        match self {
            Self::OnePoleLowPass => Some(false),
            Self::OnePoleHighPass => Some(true),
            _ => None,
        }
    }

    fn one_pole(self) -> bool {
        matches!(self, Self::OnePoleLowPass | Self::OnePoleHighPass)
    }
}

#[derive(Clone, Copy)]
enum Scope {
    Shared(usize),
    Expression(usize),
}

/// Prepared caches, indexed by the actual owner rather than MIDI channel or key.
pub(crate) struct FilterBank {
    scopes: Box<[Scope]>,
    shared: Box<[FilterCache]>,
    expressions: Box<[FilterCache]>,
    stride: usize,
    /// Cutoff and Q factors of the voice being rendered (per-voice modulation).
    /// Set around one voice's render; 1.0 uses the cached shared coefficients.
    pub modulation: [f64; 2],
}
impl FilterBank {
    pub fn new(filters: &[PreparedFilter], expressions: usize) -> Result<Self, Error> {
        let mut scopes = Vec::with_capacity(filters.len());
        let mut shared = Vec::new();
        let mut per_expression = Vec::new();
        for &filter in filters {
            scopes.push(if filter.requires_expression() {
                let index = per_expression.len();
                per_expression.push(filter);
                Scope::Expression(index)
            } else {
                let index = shared.len();
                shared.push(FilterCache::new(filter));
                Scope::Shared(index)
            });
        }
        let stride = per_expression.len();
        let count = stride.checked_mul(expressions).ok_or(Error::Capacity)?;
        let mut caches = Vec::new();
        caches
            .try_reserve_exact(count)
            .map_err(|_| Error::Capacity)?;
        for index in 0..count {
            caches.push(FilterCache::new(per_expression[index % stride]));
        }
        Ok(Self {
            scopes: scopes.into_boxed_slice(),
            shared: shared.into_boxed_slice(),
            expressions: caches.into_boxed_slice(),
            stride,
            modulation: [1.0; 2],
        })
    }
}

pub(crate) struct FilterContext<'a> {
    pub bank: &'a mut FilterBank,
    pub expression: Option<(crate::ExpressionId, crate::Expression)>,
    /// A bus's reverbs, by `PreparedProcessor::Reverb` index; empty elsewhere.
    pub reverbs: &'a mut [super::Reverb],
    /// A bus's convolutions, by `PreparedProcessor::Convolution` index.
    pub convolutions: &'a mut [super::Convolution],
}
/// Which cache a voice's filter resolved to for this block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheRef {
    Shared(usize),
    Expression(usize),
}

impl FilterBank {
    /// Fill `index`'s coefficients for one voice's block; see [`FilterContext::process`].
    pub(super) fn prepare(
        &mut self,
        index: usize,
        len: usize,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<(crate::ExpressionId, crate::Expression)>,
    ) -> CacheRef {
        let cache = match self.scopes[index] {
            Scope::Shared(index) => CacheRef::Shared(index),
            Scope::Expression(index) => {
                let (id, _) = expression.expect("prepared voice-scoped filter");
                let slot = id.0.index * self.stride + index;
                let cache = &mut self.expressions[slot];
                if cache.owner != Some(id) {
                    cache.owner = Some(id);
                    cache.start = None;
                }
                CacheRef::Expression(slot)
            }
        };
        let value = expression.as_ref().map(|(_, value)| value);
        match cache {
            CacheRef::Shared(i) => self.shared[i].prepare(at, len, parameters, value),
            CacheRef::Expression(i) => self.expressions[i].prepare(at, len, parameters, value),
        }
        cache
    }

    pub(super) fn cache(&self, cache: CacheRef) -> &FilterCache {
        match cache {
            CacheRef::Shared(i) => &self.shared[i],
            CacheRef::Expression(i) => &self.expressions[i],
        }
    }
}

impl FilterContext<'_> {
    #[inline]
    pub fn process(
        &mut self,
        index: usize,
        state: &mut [[f64; 2]; 2],
        block: &mut Planar,
        len: usize,
        parameters: &[ControlRamp],
        at: u64,
    ) {
        let cache = match self.bank.scopes[index] {
            Scope::Shared(index) => &mut self.bank.shared[index],
            Scope::Expression(index) => {
                let (id, _) = self.expression.expect("prepared voice-scoped filter");
                let cache = &mut self.bank.expressions[id.0.index * self.bank.stride + index];
                if cache.owner != Some(id) {
                    cache.owner = Some(id);
                    cache.start = None;
                }
                cache
            }
        };
        let expression = self.expression.as_ref().map(|(_, value)| value);
        let [cutoff, q] = self.bank.modulation;
        if cutoff != 1.0 || q != 1.0 {
            // A modulated voice's coefficients are its own: one set per chunk at
            // its midpoint, outside the shared cache.
            let filter = cache.filter;
            let middle = at + len as u64 / 2;
            let hz = (filter.cutoff.value(parameters, middle, expression) * cutoff)
                .clamp(20.0, 20_000.0_f64.min(filter.rate * 0.49));
            let q = (filter.q.value(parameters, middle, expression) * q).max(0.025);
            let c = Coefficients::new(filter.mode, filter.rate, hz, q);
            if let Some(high) = filter.mode.one_pole_high() {
                one_pole(state, block, len, high, |_| c.a3);
            } else {
                run(state, block, len, filter.mode.mix(), |_| c);
            }
            return;
        }
        cache.process(state, block, len, parameters, at, expression);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_responses_match_the_analog_prototype_under_bilinear_frequency_mapping() {
        for rate in [44100, 48000, 96000] {
            for mode in [
                SvfMode::LowPass,
                SvfMode::HighPass,
                SvfMode::BandPass,
                SvfMode::Notch,
                SvfMode::AllPass,
            ] {
                let filter = StateVariableFilter {
                    mode,
                    cutoff_hz: Parameter::Constant(f64::from(rate) / 8.),
                    q: Parameter::Constant(0.7),
                }
                .compile(rate, &mut Vec::new())
                .unwrap();
                let mut cache = FilterCache::new(filter);
                let mut state = [[0.; 2]; 2];
                let mut response = [[0.; 2]; 3];
                let mut block = [[0.; BLOCK]; 2];
                for n in 0..4096 {
                    let at = n as usize % BLOCK;
                    if at == 0 {
                        block = [[0.; BLOCK]; 2];
                        if n == 0 {
                            block[0][0] = 1.;
                        }
                        cache.process(&mut state, &mut block, BLOCK, &[], n, None);
                    }
                    let output = [block[0][at], block[1][at]];
                    assert_eq!(output[1], 0.);
                    for (response, omega) in response.iter_mut().zip([
                        0.,
                        std::f64::consts::TAU / 8.,
                        std::f64::consts::PI,
                    ]) {
                        let (sin, cos) = (omega * n as f64).sin_cos();
                        response[0] += cos * output[0];
                        response[1] -= sin * output[0];
                    }
                }
                // Closed-form magnitudes at DC, cutoff and Nyquist, independent
                // of the runtime state recurrence or cached coefficient formula.
                let expected = match mode {
                    SvfMode::LowPass => [1., 0.7, 0.],
                    SvfMode::HighPass => [0., 0.7, 1.],
                    SvfMode::BandPass => [0., 1., 0.],
                    SvfMode::Notch => [1., 0., 1.],
                    SvfMode::AllPass => [1.; 3],
                    SvfMode::OnePoleLowPass | SvfMode::OnePoleHighPass => unreachable!("not in this list"),
                };
                for (a, b) in response.into_iter().zip(expected) {
                    assert!(
                        (a[0].hypot(a[1]) - b).abs() < 1e-12,
                        "{mode:?}, {rate}, {a:?} != {b}"
                    );
                }
            }
        }
    }
}
