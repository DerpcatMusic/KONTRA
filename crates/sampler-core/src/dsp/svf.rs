use super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use crate::Error;

#[derive(Clone, Copy, Debug)]
pub enum SvfMode {
    LowPass,
    HighPass,
    /// Unity peak at the cutoff frequency.
    BandPass,
    Notch,
    AllPass,
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
                let coefficients = Coefficients::new(f64::from(rate), hz, q);
                if ![
                    coefficients.a1,
                    coefficients.a2,
                    coefficients.a3,
                    coefficients.k,
                ]
                .iter()
                .all(|v| v.is_finite() && *v > 0.)
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

#[derive(Clone, Copy, Default)]
struct Coefficients {
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}
impl Coefficients {
    fn new(rate: f64, hz: f64, q: f64) -> Self {
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
/// Filled lazily, so unused chains calculate nothing. Common render segmentation
/// bounds the window to 64 samples without demoting modulation to block rate.
pub(crate) struct FilterCache {
    filter: PreparedFilter,
    owner: Option<crate::ExpressionId>,
    start: u64,
    valid: u64,
    coefficients: [Coefficients; 64],
    last_values: Option<[f64; 2]>,
    last: Coefficients,
}
impl FilterCache {
    pub fn new(filter: PreparedFilter) -> Self {
        Self {
            filter,
            owner: None,
            start: 0,
            valid: 0,
            coefficients: [Coefficients::default(); 64],
            last_values: None,
            last: Coefficients::default(),
        }
    }
    pub fn begin(&mut self, at: u64) {
        self.start = at;
        self.valid = 0;
    }

    #[inline]
    pub fn process(
        &mut self,
        state: &mut [[f64; 2]; 2],
        input: [f64; 2],
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<&crate::Expression>,
    ) -> [f64; 2] {
        let index = (at - self.start) as usize;
        let bit = 1_u64 << index;
        if self.valid & bit == 0 {
            let values = [
                self.filter.cutoff.value(parameters, at, expression),
                self.filter.q.value(parameters, at, expression),
            ];
            if self.last_values != Some(values) {
                self.last = Coefficients::new(self.filter.rate, values[0], values[1]);
                self.last_values = Some(values);
            }
            self.coefficients[index] = self.last;
            self.valid |= bit;
        }
        let c = self.coefficients[index];
        std::array::from_fn(|i| {
            let v3 = input[i] - state[i][1];
            let band = c.a1 * state[i][0] + c.a2 * v3;
            let low = state[i][1] + c.a2 * state[i][0] + c.a3 * v3;
            state[i] = [2. * band - state[i][0], 2. * low - state[i][1]]
                .map(|v| if v.is_subnormal() { 0. } else { v });
            match self.filter.mode {
                SvfMode::LowPass => low,
                SvfMode::HighPass => input[i] - c.k * band - low,
                SvfMode::BandPass => c.k * band,
                SvfMode::Notch => input[i] - c.k * band,
                SvfMode::AllPass => input[i] - 2. * c.k * band,
            }
        })
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
    at: u64,
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
        std::alloc::Layout::array::<FilterCache>(count).map_err(|_| Error::Capacity)?;
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
            at: 0,
        })
    }
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
    pub fn begin(&mut self, at: u64) {
        self.at = at;
        for filter in &mut self.shared {
            filter.begin(at);
        }
    }
}

pub(crate) struct FilterContext<'a> {
    pub bank: &'a mut FilterBank,
    pub expression: Option<(crate::ExpressionId, crate::Expression)>,
}
impl FilterContext<'_> {
    #[inline]
    pub fn process(
        &mut self,
        index: usize,
        state: &mut [[f64; 2]; 2],
        input: [f64; 2],
        parameters: &[ControlRamp],
        at: u64,
    ) -> [f64; 2] {
        let cache = match self.bank.scopes[index] {
            Scope::Shared(index) => &mut self.bank.shared[index],
            Scope::Expression(index) => {
                let (id, _) = self.expression.expect("prepared voice-scoped filter");
                let cache = &mut self.bank.expressions[id.0.index * self.bank.stride + index];
                if cache.start != self.bank.at || cache.owner != Some(id) {
                    cache.begin(self.bank.at);
                    cache.owner = Some(id);
                }
                cache
            }
        };
        cache.process(
            state,
            input,
            parameters,
            at,
            self.expression.as_ref().map(|(_, value)| value),
        )
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
                for n in 0..4096 {
                    if n % 64 == 0 {
                        cache.begin(n);
                    }
                    let output =
                        cache.process(&mut state, [if n == 0 { 1. } else { 0. }, 0.], &[], n, None);
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
