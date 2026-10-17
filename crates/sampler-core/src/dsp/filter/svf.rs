use super::super::control::{ControlRamp, ControlRange, Parameter, PreparedParameter};
use super::super::{BLOCK, Planar, flush};
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
///
/// A generic TPT stand-in, not the Kontakt SV kernel: the validation domain, coefficient law,
/// mode mixes, state recurrence, one-pole conversion, coefficient cache cadence and the
/// modulation midpoint adaptation are KONTRA's own (audit CORE-FILTERS-065..072, 080, 090).
/// What is measured against Kontakt 8 is the response of SV LP2 (cutoff and Q laws live in
/// `sampler_ir::fx::filter`), not this recurrence.
#[derive(Clone, Copy, Debug)]
pub struct StateVariableFilter {
    pub mode: SvfMode,
    pub cutoff_hz: Parameter,
    pub q: Parameter,
}

impl StateVariableFilter {
    pub(crate) fn valid(self) -> bool {
        self.cutoff_hz.valid() && self.q.valid()
    }

    pub(crate) fn compile(
        self,
        rate: u32,
        bindings: &mut Vec<ControlRange>,
    ) -> Result<PreparedSvf, Error> {
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
        Ok(PreparedSvf {
            mode: self.mode,
            rate: f64::from(rate),
            cutoff: self.cutoff_hz.compile(bindings),
            q: self.q.compile(bindings),
        })
    }
}

/// Summed-part Tone adapter for the shared one-pole, with caller-owned histories.
pub struct OutputLowPass {
    cache: FilterCache,
    rate: u32,
}
impl OutputLowPass {
    pub fn new(rate: u32) -> Result<Self, Error> {
        let filter = StateVariableFilter {
            mode: SvfMode::OnePoleLowPass,
            cutoff_hz: Parameter::Constant(20_000f64.min(f64::from(rate) * 0.45)),
            q: Parameter::Constant(1.),
        }
        .compile(rate, &mut Vec::new())?;
        Ok(Self {
            cache: FilterCache::new(filter),
            rate,
        })
    }

    /// v1 Player::output: dry bypass tracks the last sample before filter enable.
    pub fn process(
        &mut self,
        frames: &mut [crate::Frame],
        state: &mut [[f64; 2]; 2],
        cutoff: f64,
        at: u64,
    ) -> Result<(), Error> {
        if !(20.0..=20_000.).contains(&cutoff) {
            return Err(Error::InvalidInput);
        }
        if cutoff >= 20_000. {
            if let Some(last) = frames.last() {
                *state = [last.map(f64::from), [0.; 2]];
            }
            return Ok(());
        }
        self.cache.filter.cutoff =
            PreparedParameter::Constant(cutoff.min(f64::from(self.rate) * 0.45));
        for (chunk, frames) in frames.chunks_mut(BLOCK).enumerate() {
            let mut block = [[0.; BLOCK]; 2];
            for (n, frame) in frames.iter().enumerate() {
                block[0][n] = f64::from(frame[0]);
                block[1][n] = f64::from(frame[1]);
            }
            self.cache.process(
                state,
                &mut block,
                frames.len(),
                &[],
                at + (chunk * BLOCK) as u64,
                None,
            );
            for (n, frame) in frames.iter_mut().enumerate() {
                *frame = [block[0][n] as f32, block[1][n] as f32];
            }
        }
        Ok(())
    }
}

/// The logical target kind is admission metadata. It owns no coefficient or
/// history storage and never dispatches an SVF render operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum FilterKind {
    StateVariable,
    NativeControl,
}

/// Issued only while appending an actual SVF design to its prepared registry.
/// Addressed identity is public modulation order; dense identity owns caches.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SvfHandle {
    addressed: usize,
    dense: usize,
}
impl SvfHandle {
    pub(crate) fn addressed(self) -> usize { self.addressed }
}

/// Control-side builder: append kind and design together in constant work.
#[derive(Default)]
pub(crate) struct FilterRegistryBuilder {
    kinds: Vec<FilterKind>,
    svfs: Vec<PreparedSvf>,
}
impl FilterRegistryBuilder {
    pub(crate) fn push_svf(&mut self, filter: PreparedSvf) -> SvfHandle {
        let handle = SvfHandle { addressed: self.kinds.len(), dense: self.svfs.len() };
        self.kinds.push(FilterKind::StateVariable);
        self.svfs.push(filter);
        handle
    }
    pub(crate) fn push_native(&mut self) -> usize {
        let addressed = self.kinds.len();
        self.kinds.push(FilterKind::NativeControl);
        addressed
    }
    pub(crate) fn requires_expression(&self) -> bool {
        self.svfs.iter().any(|filter| filter.requires_expression())
    }
    pub(crate) fn finish(self) -> PreparedFilters {
        PreparedFilters { kinds: self.kinds.into_boxed_slice(), svfs: self.svfs.into_boxed_slice() }
    }
}

/// Immutable designs are owned once. Native addresses do not reserve an SVF
/// payload; each rendering cache still owns exactly its previous window.
#[derive(Default)]
pub(crate) struct PreparedFilters {
    kinds: Box<[FilterKind]>,
    svfs: Box<[PreparedSvf]>,
}
impl PreparedFilters {
    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.kinds.is_empty()
    }
    pub(crate) fn kind(&self, addressed: usize) -> Option<FilterKind> {
        self.kinds.get(addressed).copied()
    }
    pub(crate) fn trace_parameters(&self, handle: SvfHandle) -> [(&'static str, PreparedParameter); 2] {
        self.svfs[handle.dense].trace_parameters()
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PreparedSvf {
    mode: SvfMode,
    rate: f64,
    cutoff: PreparedParameter,
    q: PreparedParameter,
}

impl PreparedSvf {
    fn trace_parameters(self) -> [(&'static str, PreparedParameter); 2] {
        [("cutoff_hz", self.cutoff), ("q", self.q)]
    }
    fn requires_expression(self) -> bool {
        self.cutoff.requires_expression() || self.q.requires_expression()
    }
}

#[derive(Clone, Copy, Default, PartialEq)]
pub(crate) struct Coefficients {
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
                a3: core_one_pole_coefficient(rate, hz),
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
    filter: PreparedSvf,
    owner: Option<crate::ExpressionId>,
    start: Option<u64>,
    filled: usize,
    /// Every filled entry equals the first: the block runs on hoisted values.
    pub(crate) uniform: bool,
    pub(crate) coefficients: [Coefficients; BLOCK],
    last_values: Option<[f64; 2]>,
    last: Coefficients,
}
impl FilterCache {
    pub fn new(filter: PreparedSvf) -> Self {
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

    pub(crate) fn one_pole_high(&self) -> Option<bool> {
        self.filter.mode.one_pole_high()
    }

    /// The response's output mix: `[m0, mk, m2]`.
    pub(crate) fn mix(&self) -> [f64; 3] {
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
                self.last =
                    Coefficients::new(self.filter.mode, self.filter.rate, values[0], values[1]);
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
            one_pole(state, block, len, high, |i| {
                coefficients[if uniform { 0 } else { i }].a3
            });
        } else if self.uniform {
            let c = self.coefficients[0];
            run(state, block, len, mix, |_| c);
        } else {
            let coefficients = &self.coefficients;
            run(state, block, len, mix, |i| coefficients[i]);
        }
    }
}

/// The existing core-law conversion, reused by explicit width-preserving stages.
/// Callers validate 0 < hz < rate/2 before publishing a plan.
pub(crate) fn core_one_pole_coefficient(rate: f64, hz: f64) -> f64 {
    -(-std::f64::consts::TAU * hz / rate).exp_m1()
}
/// Existing segment-end denormal history policy, shared without changing cadence.
pub(crate) fn core_one_pole_history(value: f64) -> f64 { flush(value) }

/// `y += (x - y) b` per channel in `state[0]`; the band-state row stays zero.
#[inline(always)]
pub(crate) fn one_pole_sample(input: f64, state: f64, b: f64, high: bool) -> (f64, f64) {
    let next = state + (input - state) * b;
    (if high { input - next } else { next }, next)
}

#[inline(always)]
pub(crate) fn sample(
    x: f64,
    [ic1, ic2]: [f64; 2],
    [a1, a2, a3, km]: [f64; 4],
    [m0, m2]: [f64; 2],
) -> (f64, [f64; 2]) {
    let v3 = x - ic2;
    let band = a1 * ic1 + a2 * v3;
    let low = ic2 + a2 * ic1 + a3 * v3;
    (
        m0 * x + km * band + m2 * low,
        [2. * band - ic1, 2. * low - ic2],
    )
}

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
        (*l, yl) = one_pole_sample(*l, yl, b, high);
        (*r, yr) = one_pole_sample(*r, yr, b, high);
    }
    state[0] = [yl, yr].map(core_one_pole_history);
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
            let (y, next) = sample(
                x[ch],
                [s0[ch], s1[ch]],
                [c.a1, c.a2, c.a3, mk * c.k],
                [m0, m2],
            );
            (s0[ch], s1[ch]) = (next[0], next[1]);
            y
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

    pub(crate) fn one_pole_high(self) -> Option<bool> {
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
    /// Logical-address reset defaults for native knob modulation, not SVF scopes.
    native: Box<[bool]>,
    shared: Box<[FilterCache]>,
    expressions: Box<[FilterCache]>,
    stride: usize,
    /// Cutoff and Q factors of the voice being rendered (per-voice modulation).
    /// Set around one voice's render; 1.0 uses the cached shared coefficients.
    pub modulation: [f64; 2],
    addressed_modulation: Box<[[f64; 4]]>,
    modulation_program: Option<u32>,
}
impl FilterBank {
    pub fn new(filters: &PreparedFilters, expressions: usize) -> Result<Self, Error> {
        let mut scopes = Vec::with_capacity(filters.svfs.len());
        let mut shared = Vec::new();
        let mut per_expression = Vec::new();
        for &filter in &filters.svfs {
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
            native: filters.kinds.iter().map(|kind| *kind == FilterKind::NativeControl).collect(),
            shared: shared.into_boxed_slice(),
            expressions: caches.into_boxed_slice(),
            stride,
            modulation: [1.0; 2],
            modulation_program: None,
            addressed_modulation: filters
                .kinds
                .iter()
                .map(|kind| {
                    if *kind == FilterKind::NativeControl {
                        [0.; 4]
                    } else {
                        [1., 1., 0., 0.]
                    }
                })
                .collect(),
        })
    }
}

pub(crate) struct FilterContext<'a> {
    pub tempo: f64,
    pub bank: &'a mut FilterBank,
    pub expression: Option<(crate::ExpressionId, crate::Expression)>,
    /// A bus's reverbs and convolutions, by `PreparedProcessor::Space` index;
    /// empty elsewhere.
    pub spaces: &'a mut [super::super::Space],
}
/// Which cache a voice's filter resolved to for this block.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CacheRef {
    Shared(usize),
    Expression(usize),
}

impl FilterBank {
    pub(crate) fn clear_source_modulation(&mut self) {
        self.modulation = [1.; 2];
        self.modulation_program = None;
        for (index, factors) in self.addressed_modulation.iter_mut().enumerate() {
            *factors = if self.native[index] { [0.; 4] } else { [1., 1., 0., 0.] };
        }
    }
    pub(crate) fn set_addressed_modulation(
        &mut self,
        modulation: &crate::voice_mod::VoiceModulation,
        state: &crate::voice_mod::VoiceModState,
        voice: usize,
    ) {
        state.fill_filter_factors(
            modulation,
            voice,
            &mut self.addressed_modulation,
            &mut self.modulation_program,
            |i| self.native[i],
        );
    }

    pub(crate) fn native_knobs(&self, index: usize) -> [f64; 4] {
        self.addressed_modulation[index]
    }
    /// Fill `index`'s coefficients for one voice's block; see [`FilterContext::process`].
    pub(crate) fn prepare(
        &mut self,
        index: SvfHandle,
        len: usize,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<(crate::ExpressionId, crate::Expression)>,
    ) -> CacheRef {
        let cache = match self.scopes[index.dense] {
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

    pub(crate) fn cache(&self, cache: CacheRef) -> &FilterCache {
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
        index: SvfHandle,
        state: &mut [[f64; 2]; 2],
        block: &mut Planar,
        len: usize,
        parameters: &[ControlRamp],
        at: u64,
    ) {
        let cache = match self.bank.scopes[index.dense] {
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
        let [cutoff, q] = std::array::from_fn(|i| {
            self.bank.modulation[i] * self.bank.addressed_modulation[index.addressed][i]
        });
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
        let filter = cache.filter;
        if filter.cutoff.projected(parameters) || filter.q.projected(parameters) {
            // Voice offsets must neither reuse nor overwrite a sibling's coefficients.
            let mut projected = FilterCache::new(filter);
            projected.process(state, block, len, parameters, at, expression);
        } else {
            cache.process(state, block, len, parameters, at, expression);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prepared(cutoff_hz: Parameter) -> PreparedSvf {
        StateVariableFilter { mode: SvfMode::LowPass, cutoff_hz, q: Parameter::Constant(0.7) }
            .compile(48_000, &mut Vec::new()).unwrap()
    }

    /// Public addresses include native stages; cache/design indices do not.
    /// The oracle derives coefficients directly from the authored parameters.
    #[test]
    fn interleaved_filter_addresses_select_their_dense_design_and_scope() {
        let mut builder = FilterRegistryBuilder::default();
        assert_eq!(builder.push_native(), 0);
        let first = builder.push_svf(prepared(Parameter::Constant(700.)));
        assert_eq!(builder.push_native(), 2);
        let expressive = builder.push_svf(prepared(Parameter::Expression {
            source: crate::ExpressionSource::Pressure, low: 400., high: 4000.,
        }));
        let last = builder.push_svf(prepared(Parameter::Constant(2600.)));
        assert_eq!([first.addressed(), expressive.addressed(), last.addressed()], [1, 3, 4]);
        assert_eq!([first.dense, expressive.dense, last.dense], [0, 1, 2]);
        assert!(builder.requires_expression());
        let filters = builder.finish();
        for (index, kind) in [FilterKind::NativeControl, FilterKind::StateVariable,
            FilterKind::NativeControl, FilterKind::StateVariable, FilterKind::StateVariable]
            .into_iter().enumerate() {
            assert_eq!(filters.kind(index), Some(kind));
        }
        assert_eq!(filters.kind(5), None);
        for (handle, expected) in [(first, 700.), (last, 2600.)] {
            assert!(matches!(filters.trace_parameters(handle)[0],
                ("cutoff_hz", PreparedParameter::Constant(value)) if value == expected));
        }
        let mut bank = FilterBank::new(&filters, 2).unwrap();
        assert_eq!(bank.scopes.len(), 3);
        assert_eq!(bank.shared.len(), 2);
        assert_eq!(bank.expressions.len(), 2);
        assert_eq!(bank.stride, 1);
        assert_eq!(&*bank.native, &[true, false, true, false, false]);
        assert_eq!(&*bank.addressed_modulation,
            &[[0.; 4], [1., 1., 0., 0.], [0.; 4], [1., 1., 0., 0.], [1., 1., 0., 0.]]);
        for (handle, hz, slot) in [(first, 700., 0), (last, 2600., 1)] {
            let cache = bank.prepare(handle, 7, &[], 0, None);
            assert!(matches!(cache, CacheRef::Shared(index) if index == slot));
            let actual = bank.cache(cache).coefficients[0];
            let expected = Coefficients::new(SvfMode::LowPass, 48_000., hz, 0.7);
            assert_eq!([actual.a1, actual.a2, actual.a3, actual.k].map(f64::to_bits),
                [expected.a1, expected.a2, expected.a3, expected.k].map(f64::to_bits));
        }
    }

    /// Same frame and recycled physical slot must still invalidate the window
    /// when the expression generation changes. A native address precedes it.
    #[test]
    fn dense_expression_cache_reuse_keeps_the_full_owner_identity() {
        let mut builder = FilterRegistryBuilder::default();
        builder.push_native();
        let handle = builder.push_svf(prepared(Parameter::Expression {
            source: crate::ExpressionSource::Pressure, low: 400., high: 4000.,
        }));
        let filters = builder.finish();
        let mut bank = FilterBank::new(&filters, 1).unwrap();
        for (generation, pressure, hz) in [(1, 0, 400.), (2, u32::MAX, 4000.), (3, 0, 400.)] {
            let id = crate::ExpressionId(crate::Handle { runtime: 1, index: 0, generation });
            let expression = crate::Expression { pressure, ..Default::default() };
            let cache = bank.prepare(handle, 7, &[], 0, Some((id, expression)));
            assert!(matches!(cache, CacheRef::Expression(0)));
            let actual = bank.cache(cache);
            assert_eq!(actual.owner, Some(id));
            let expected = Coefficients::new(SvfMode::LowPass, 48_000., hz, 0.7);
            let actual = actual.coefficients[0];
            assert_eq!([actual.a1, actual.a2, actual.a3, actual.k].map(f64::to_bits),
                [expected.a1, expected.a2, expected.a3, expected.k].map(f64::to_bits));
        }
    }

    /// Modulation must read the logical address, even when its dense index is
    /// different. An independent constant-design filter is the PCM oracle.
    #[test]
    fn dense_svf_process_reads_addressed_modulation_factors() {
        let mut builder = FilterRegistryBuilder::default();
        builder.push_native();
        let handle = builder.push_svf(prepared(Parameter::Constant(700.)));
        let filters = builder.finish();
        let mut bank = FilterBank::new(&filters, 0).unwrap();
        bank.addressed_modulation[handle.addressed()] = [1.5, 2., 0., 0.];
        let mut actual: Planar = std::array::from_fn(|ch|
            std::array::from_fn(|i| ((i + ch * 3) as f64 * 0.13).sin() * 0.1));
        let mut expected = actual;
        let mut state = [[0.; 2]; 2];
        let mut expected_state = state;
        FilterContext { tempo: 120., bank: &mut bank, expression: None, spaces: &mut [] }
            .process(handle, &mut state, &mut actual, BLOCK, &[], 0);
        let oracle = StateVariableFilter { mode: SvfMode::LowPass,
            cutoff_hz: Parameter::Constant(1050.), q: Parameter::Constant(1.4) }
            .compile(48_000, &mut Vec::new()).unwrap();
        FilterCache::new(oracle).process(&mut expected_state, &mut expected, BLOCK, &[], 0, None);
        assert_eq!(actual, expected);
        assert_eq!(state, expected_state);
    }

    #[test]
    fn svf_typed_registry_layout_receipt() {
        use std::mem::size_of;
        eprintln!("svf layout: handle={} kind={} design={} registry={} scope={} bank={} cache={}",
            size_of::<SvfHandle>(), size_of::<FilterKind>(), size_of::<PreparedSvf>(),
            size_of::<PreparedFilters>(), size_of::<Scope>(), size_of::<FilterBank>(), size_of::<FilterCache>());
        assert_eq!(size_of::<FilterKind>(), 1);
    }

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
                    SvfMode::OnePoleLowPass | SvfMode::OnePoleHighPass => {
                        unreachable!("not in this list")
                    }
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
