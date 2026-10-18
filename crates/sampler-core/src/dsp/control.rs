use crate::{ControlId, Error, Prepared};
use crate::control::ControlProjection;

/// Map a control's declared range to destination units, with a sample-clock ramp.
/// Integer, real and toggle controls share the same value owner; constant domains
/// select `low`. The processor field defines the units and validates its range.
#[derive(Clone, Copy, Debug)]
pub struct ControlRange {
    pub control: ControlId,
    pub low: f64,
    pub high: f64,
    pub ramp_frames: u32,
}
impl ControlRange {
    pub(super) fn valid(self) -> bool {
        self.low.is_finite() && self.high.is_finite() && (self.high - self.low).is_finite()
    }

    pub(super) fn target(self, normalized: f64) -> f64 {
        interpolate(self.low, self.high, normalized)
    }
}

/// A processor parameter, in the units declared by its destination field.
#[derive(Clone, Copy, Debug)]
pub enum Parameter {
    Constant(f64),
    Control(ControlRange),
    /// Immediate event-rate mapping of the retained note's full-resolution value.
    /// Valid in voice scope; a summed bus has no single note-expression owner.
    Expression {
        source: crate::ExpressionSource,
        low: f64,
        high: f64,
    },
}

impl Parameter {
    pub(super) fn bounds(self) -> [f64; 2] {
        match self {
            Self::Constant(value) => [value; 2],
            Self::Control(binding) => [binding.low, binding.high],
            Self::Expression { low, high, .. } => [low, high],
        }
    }

    pub(super) fn valid(self) -> bool {
        match self {
            Self::Constant(value) => value.is_finite(),
            Self::Control(binding) => binding.valid(),
            Self::Expression { low, high, .. } => {
                low.is_finite() && high.is_finite() && (high - low).is_finite()
            }
        }
    }

    pub(super) fn compile(self, bindings: &mut Vec<ControlRange>) -> PreparedParameter {
        match self {
            Self::Constant(value) => PreparedParameter::Constant(value),
            Self::Expression { source, low, high } => {
                PreparedParameter::Expression { source, low, high }
            }
            Self::Control(binding) => {
                let lane = bindings.len();
                bindings.push(binding);
                PreparedParameter::Control(lane)
            }
        }
    }
}

#[cfg(test)]
std::thread_local! {
    pub(super) static PARAMETER_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    pub(super) static RAMP_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Copy)]
pub(crate) enum PreparedParameter {
    Constant(f64),
    Control(usize),
    Expression {
        source: crate::ExpressionSource,
        low: f64,
        high: f64,
    },
}
impl PreparedParameter {
    pub(super) fn projected(self, parameters: &[ControlRamp]) -> bool {
        matches!(self, Self::Control(lane) if (parameters[lane].modulation.is_some()||parameters[lane].owner_projection.is_some()))
    }
    /// A held shared Cutoff factor on the genuine lane. The processor, which
    /// knows its ACTUAL render chunk, owns the legacy midpoint/base law.
    pub(crate) fn scoped_cutoff_factor(self,parameters:&[ControlRamp])->Option<f64>{
        let Self::Control(lane)=self else{return None;};
        let projection=parameters.get(lane)?.owner_projection?;
        (projection.multiply&&projection.clamp.is_some()&&projection.from==projection.to).then_some(projection.from)
    }
    /// Read the same accepted base trajectory (or ordered fixed-fold result)
    /// without multiplying that shared factor a second time.
    pub(crate) fn scoped_cutoff_base(self,parameters:&[ControlRamp],at:u64,expression:Option<&crate::Expression>)->f64{
        if self.scoped_cutoff_factor(parameters).is_some(){
            if let Self::Control(lane)=self{let mut base=parameters[lane];base.owner_projection=None;return base.value(at);}
        }
        self.value(parameters,at,expression)
    }
    pub fn requires_expression(self) -> bool {
        matches!(self, Self::Expression { .. })
    }
    pub fn value(
        self,
        parameters: &[ControlRamp],
        at: u64,
        expression: Option<&crate::Expression>,
    ) -> f64 {
        #[cfg(test)]
        PARAMETER_READS.with(|n| n.set(n.get() + 1));
        match self {
            Self::Constant(value) => value,
            Self::Control(lane) => parameters[lane].value(at),
            Self::Expression { source, low, high } => {
                let expression = expression.expect("prepared voice-scoped parameter");
                let value = match source {
                    crate::ExpressionSource::Pressure => expression.pressure,
                    crate::ExpressionSource::Timbre => expression.timbre,
                };
                interpolate(low, high, f64::from(value) / f64::from(u32::MAX))
            }
        }
    }
}

fn interpolate(low: f64, high: f64, normalized: f64) -> f64 {
    if normalized == 0. {
        low
    } else if normalized == 1. {
        high
    } else {
        (low + (high - low) * normalized).clamp(low.min(high), low.max(high))
    }
}

fn normalized(value: ControlProjection) -> f64 {
    match value {
        ControlProjection::Integer { min, max, value } => {
            let span = i128::from(max) - i128::from(min);
            if span == 0 {
                0.
            } else {
                (i128::from(value) - i128::from(min)) as f64 / span as f64
            }
        }
        ControlProjection::Real { min, max, value } => {
            if min == max {
                0.
            } else if (max - min).is_finite() {
                (value - min) / (max - min)
            } else {
                (value * 0.5 - min * 0.5) / (max * 0.5 - min * 0.5)
            }
        }
        ControlProjection::Toggle(value) => f64::from(value),
    }
    .clamp(0., 1.)
}

/// One trajectory per prepared binding, shared by all voices in that generation.
/// Evaluation is a function of absolute sample time, never voice/render call count.
#[derive(Clone, Copy, Default)]
pub(crate) struct ControlRamp {
    from: f64,
    target: f64,
    start: u64,
    frames: u32,
    modulation: Option<(f64, [f64; 2])>,
    owner_projection:Option<OwnerProjection>,
}
#[derive(Clone,Copy)]
struct OwnerProjection {from:f64,to:f64,begin:u64,end:u64,multiply:bool,clamp:Option<[f64;2]>}
impl OwnerProjection {
    fn value(self,at:u64)->f64{
        if self.from==self.to||at<=self.begin{return self.from;}if at>=self.end{return self.to;}
        self.from+(self.to-self.from)*(at-self.begin)as f64/(self.end-self.begin)as f64
    }
    fn held(self,at:u64,len:usize)->bool{self.from==self.to || at>=self.end || at.saturating_add(len.saturating_sub(1)as u64)<=self.begin}
}
impl ControlRamp {
    pub(super) fn held(self, at: u64, len: usize) -> Option<f64> {
        let first = self.value(at);
        if self.owner_projection.is_some_and(|p|!p.held(at,len)){return None;}
        // A product of two moving ramps can return to the same endpoint.
        // Only a held owner factor can use the old monotone base shortcut.
        if at.saturating_sub(self.start) >= u64::from(self.frames) || self.from == self.target {
            return Some(first);
        }
        // A linear ramp plus a held modulation offset and clamp is monotone.
        (first == self.value(at + len.saturating_sub(1) as u64)).then_some(first)
    }

    #[cfg(test)]
    pub(super) fn test_ramp(from: f64, target: f64, start: u64, frames: u32) -> Self {
        Self {
            from,
            target,
            start,
            frames,
            modulation: None,owner_projection:None,
        }
    }
    pub(crate) fn value(self, at: u64) -> f64 {
        #[cfg(test)]
        RAMP_READS.with(|n| n.set(n.get() + 1));
        let elapsed = at.saturating_sub(self.start);
        let value = if elapsed >= u64::from(self.frames) {
            self.target
        } else {
            (self.from + (self.target - self.from) * (elapsed as f64 / f64::from(self.frames)))
                .clamp(self.from.min(self.target), self.from.max(self.target))
        };
        let value=match self.modulation {
            None => value,
            Some((delta,[low,high]))=>(value+delta).clamp(low,high),
        };
        match self.owner_projection {
            None=>value,
            Some(p)if p.multiply=>{let value=value*p.value(at);p.clamp.map_or(value,|[low,high]|value.clamp(low,high))},
            Some(p)=>value+p.value(at), // Pan clamps once in the actual home.
        }
    }
    pub(crate) fn project_owner(&mut self,from:f64,to:f64,begin:u64,end:u64,multiply:bool){
        self.owner_projection=Some(OwnerProjection{from,to,begin,end,multiply,clamp:None});
    }
    /// Project onto a COPY of the genuine lane, never its retained public base.
    /// Current historical fixed-fold homes have zero-ramp real controls. A
    /// moving base with fixed clamp steps needs its real piecewise law and is
    /// refused, not replaced by interpolation of folded endpoints.
    pub(crate) fn project_processor(mut self,factor:f64,fixed:&[sampler_ir::SourceFrequencyFold],law:sampler_ir::SourceProcessingParameterLaw,begin:u64,end:u64,rate:u32)->Result<Self,Error>{
        // Same positive exp2 factor domain as the physical Cutoff law: finite
        // semitone sums can underflow to zero or overflow to +Inf. The actual
        // legacy coefficient path clamps the multiplied Hz, not the factor.
        if factor.is_nan()||factor<0.||end<begin||rate==0{return Err(Error::InvalidInput);}
        match law {
            sampler_ir::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint=>if !fixed.is_empty(){return Err(Error::InvalidInput);},
            sampler_ir::SourceProcessingParameterLaw::HistoricalUviFrequencyThreeDecadeCore=>{
                if !fixed.is_empty(){
                    let len=usize::try_from(end.saturating_sub(begin)).map_err(|_|Error::Capacity)?.max(1);
                    let mut base=self.held(begin,len).ok_or(Error::InvalidInput)?;
                    for step in fixed {base=match *step {
                        sampler_ir::SourceFrequencyFold::MultiplyClamp(value)=>{if !value.is_finite()||value<=0.{return Err(Error::InvalidInput);}(base*value).clamp(20.,20000.)},
                        sampler_ir::SourceFrequencyFold::LowerClamp=>20.,sampler_ir::SourceFrequencyFold::UpperClamp=>20000.,
                    };}
                    self.from=base;self.target=base;self.start=begin;self.frames=0;self.modulation=None;self.owner_projection=None;
                }
            },
        }
        // A neutral route is genuinely unmodulated in the old consumer: keep
        // its ordinary base ramp and coefficient/cache path, without even the
        // nonneutral Hz clamp (notably near the admitted sample-rate limit).
        if factor==1.{return Ok(self);}
        let low=20.;let high=20000.0_f64.min(f64::from(rate)*0.49);
        if high<low||!self.value(begin).is_finite()||!self.value(end).is_finite()||self.value(begin)<=0.||self.value(end)<=0.{return Err(Error::InvalidInput);}
        self.project_owner(factor,factor,begin,end,true);
        self.owner_projection.as_mut().ok_or(Error::InvalidInput)?.clamp=Some([low,high]);Ok(self)
    }

    pub(crate) fn add_modulation(&mut self, delta: f64, binding: ControlRange) {
        let sum = self.modulation.map_or(0., |(delta, _)| delta) + delta;
        self.modulation = Some((
            sum,
            [binding.low.min(binding.high), binding.low.max(binding.high)],
        ));
    }
    fn set(&mut self, at: u64, target: f64, frames: u32) {
        if target != self.target {
            self.from = self.value(at);
            self.target = target;
            self.start = at;
            self.frames = frames;
        }
    }
}

impl Prepared {
    pub(crate) fn validate_dsp_controls(&self) -> Result<(), Error> {
        self.voice_modulation
            .validate_dsp_targets(&self.filters, self.dsp_bindings.len())?;
        self.processing_modulation.validate_dsp_targets(&self.filters,self.dsp_bindings.len())?;
        self.validate_source_owner_output_controls()?;
        self.validate_processing_parameters()?;
        for binding in self.dsp_bindings.iter().chain(self.buses.parameters.iter()) {
            self.control_index(binding.control)?;
        }
        Ok(())
    }
}

pub(crate) fn initial_parameters(plan: &Prepared, bindings: &[ControlRange]) -> Box<[ControlRamp]> {
    bindings.iter().map(|binding| initial_parameter(plan, binding)).collect()
}
/// Same initialization law for one scoped graph parameter, without allocating
/// a temporary ramp array for controls belonging to excluded regions.
pub(crate) fn initial_parameter(plan: &Prepared, binding: &ControlRange) -> ControlRamp {
    let index = plan.control_index(binding.control).unwrap();
    let target = binding.target(normalized(plan.control_layout.initial(index)));
    ControlRamp { from: target, target, start: 0, frames: 0, modulation: None,owner_projection:None }
}

impl super::DspState {
    pub(crate) fn edit_control(
        &mut self,
        plan: &Prepared,
        id: ControlId,
        value: ControlProjection,
        at: u64,
    ) {
        edit_parameters(
            &mut self.parameters,
            &plan.dsp_bindings,
            &plan.dsp_controls,
            id,
            value,
            at,
        );
        edit_parameters(
            &mut self.buses.parameters,
            &plan.buses.parameters,
            &plan.buses.controls,
            id,
            value,
            at,
        );
    }
}

fn edit_parameters(
    parameters: &mut [ControlRamp],
    bindings: &[ControlRange],
    controls: &[(ControlId, usize)],
    control: ControlId,
    value: ControlProjection,
    at: u64,
) {
    let from = controls.partition_point(|(id, _)| *id < control);
    let until = controls.partition_point(|(id, _)| *id <= control);
    let value = normalized(value);
    for &(_, lane) in &controls[from..until] {
        let binding = bindings[lane];
        parameters[lane].set(at, binding.target(value), binding.ramp_frames);
    }
}

#[cfg(test)]
mod projection_tests {
    use super::*;

    #[test]
    fn typed_projection_retains_full_width_extreme_equal_and_signed_zero_laws() {
        for (projection,expected) in [
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:i64::MIN},0.0f64),
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:i64::MAX},1.),
            (ControlProjection::Integer {min:i64::MIN,max:i64::MAX,value:0},0.5),
            (ControlProjection::Integer {min:42,max:42,value:42},0.),
            (ControlProjection::Real {min:-f64::MAX,max:f64::MAX,value:0.},0.5),
            (ControlProjection::Real {min:-f64::MAX,max:f64::MAX,value:f64::MAX},1.),
            (ControlProjection::Real {min:0.,max:1.,value:-0.},-0.),
            (ControlProjection::Real {min:0.25,max:0.25,value:0.25},0.),
            (ControlProjection::Real {min:f64::from_bits(1),max:f64::from_bits(2),value:f64::from_bits(2)},1.),
            (ControlProjection::Toggle(false),0.),(ControlProjection::Toggle(true),1.),
        ] { assert_eq!(normalized(projection).to_bits(),expected.to_bits()); }
    }

    #[test]
    fn held_controls_keep_ramp_edges_and_clamped_plateaus() {
        let ramp = ControlRamp::test_ramp(0., 2., 10, 100);
        assert_eq!(ramp.held(0, 10), Some(0.));
        assert_eq!(ramp.held(10, 32), None);
        assert_eq!(ramp.held(100, 32), None);
        assert_eq!(ramp.held(110, 32), Some(2.));
        let projected = ControlRamp {
            modulation: Some((-1., [0., 2.])),
            ..ramp
        };
        assert_eq!(projected.held(10, 32), Some(0.));
        assert_eq!(projected.held(40, 32), None);
        assert_eq!(projected.held(110, 32), Some(1.));
    }

    #[test]
    fn projection_clamps_after_the_base_ramp_and_ordered_route_sum() {
        let base = ControlRamp {
            from: 0.,
            target: 2.,
            start: 0,
            frames: 100,
            modulation: None,owner_projection:None,
        };
        let binding = ControlRange {
            control: ControlId(1),
            low: 0.,
            high: 2.,
            ramp_frames: 0,
        };
        let mut projected = base;
        projected.add_modulation(-1., binding);
        assert_eq!(projected.value(25), 0.);
        assert_eq!(projected.value(75), 0.5);
        assert_eq!(base.value(75), 1.5);
        projected.add_modulation(1., binding);
        assert_eq!(projected.value(25), base.value(25));
        let mut projected = base;
        for delta in [1., 2f64.powi(-54), -1.] {
            projected.add_modulation(delta, binding);
        }
        assert_eq!(
            projected.value(0),
            0.,
            "saved route order cannot be reassociated"
        );
    }
}


/// Render-call-local curve reuse. This value is never retained in a plan,
/// processor, expression owner, snapshot or control publication.
pub(crate) struct PanCoefficients {
    law:sampler_ir::PanLaw,
    previous:Option<f64>,
    gains:[f64;2],
    #[cfg(test)]
    evaluations:usize,
}
impl PanCoefficients {
    pub(crate) fn new(law:sampler_ir::PanLaw)->Self {
        Self{law,previous:None,gains:[1.;2],#[cfg(test)]evaluations:0}
    }
    pub(crate) fn at(&mut self,position:f64)->[f64;2] {
        if self.previous!=Some(position) {
            self.gains=self.law.gains(position);
            self.previous=Some(position);
            #[cfg(test)]
            {self.evaluations+=1;}
        }
        self.gains
    }
}

#[cfg(test)]
mod pan_coefficient_work_tests {
    use super::*;
    #[test]
    fn pan_coefficient_work_is_bounded_by_changed_combined_positions() {
        for law in [sampler_ir::PanLaw::Balance,sampler_ir::PanLaw::EqualPower,sampler_ir::PanLaw::RaisedCosineCoreF64] {
            let mut fixed=PanCoefficients::new(law);
            for _ in 0..4096{assert_eq!(fixed.at(0.375),law.gains(0.375));}
            assert_eq!(fixed.evaluations,1);
            let mut ramp=PanCoefficients::new(law);
            for frame in 0..64 {
                // Same scalar addition/clamp sequence used by the actual
                // renderers; the saturated suffix has only one curve value.
                let value=(-0.75+(frame as f64)*0.25+0.125+0.125).clamp(-1.,1.);
                assert_eq!(ramp.at(value),law.gains(value));
            }
            assert_eq!(ramp.evaluations,7);
            let mut next_call=PanCoefficients::new(law);
            assert_eq!(next_call.at(0.375),law.gains(0.375));
            assert_eq!(next_call.evaluations,1);
        }
    }
}

#[cfg(test)]
mod scoped_projection_tests {
    use super::*;
    #[test]
    fn raw_pan_projection_is_not_clamped_before_constant_and_expression_home() {
        let mut ramp=ControlRamp::test_ramp(0.8,0.8,0,0);
        ramp.project_owner(-0.4,0.6,0,64,false);
        assert_eq!(ramp.held(0,64),None);
        for at in 0..64 {
            let routed=-0.4+at as f64/64.;
            assert!((ramp.value(at)-(0.8+routed)).abs()<1e-15);
            let complete=(ramp.value(at)-0.5+0.25).clamp(-1.,1.);
            assert!((complete-(0.8+routed-0.5+0.25).clamp(-1.,1.)).abs()<1e-15);
        }
        assert!(ramp.value(64)>1.,"the actual Pan home owns the only final clamp");
        assert_eq!(ramp.held(64,64),Some(1.4));
    }
    #[test]
    fn moving_gain_base_and_owner_factor_do_not_claim_equal_endpoint_hold() {
        let mut ramp=ControlRamp::test_ramp(1.,2.,0,64);
        ramp.project_owner(2.,1.,0,64,true);
        assert_eq!(ramp.value(0),ramp.value(64));
        assert_eq!(ramp.held(0,65),None);
        assert_eq!(ramp.value(32),2.25);
        assert_eq!(ramp.held(64,64),Some(2.));
    }
    #[test]
    fn held_scalar_projection_preserves_held_base_and_absolute_offsets() {
        let mut ramp=ControlRamp::test_ramp(0.25,0.25,97,0);
        ramp.project_owner(0.5,0.5,97,128,false);
        assert_eq!(ramp.held(97,31),Some(0.75));
        let p=PreparedParameter::Control(0);
        assert!(p.projected(&[ramp]));
        assert_eq!(p.value(&[ramp],109,None),0.75);
    }
    #[test]
    fn processor_projection_keeps_legacy_positive_overflow_underflow_hz_clamp(){
        let raw=ControlRamp::test_ramp(1000.,1000.,0,0);
        for factor in [0.,f64::INFINITY,1.,2.] {
            let projected=raw.project_processor(factor,&[],sampler_ir::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint,64,128,48000).unwrap();
            let expected=(1000.*factor).clamp(20.,20000.);
            for at in [64,65,97,127,128]{assert_eq!(projected.value(at),expected);}
            assert_eq!(raw.value(97),1000.);assert_eq!(projected.held(64,64),Some(expected));
        }
    }
    #[test]
    fn historical_fixed_steps_clamp_in_original_order_before_dynamic_cutoff(){
        use sampler_ir::{SourceFrequencyFold as F,SourceProcessingParameterLaw as L};
        let raw=ControlRamp::test_ramp(1000.,1000.,0,0);
        let p=raw.project_processor(2.,&[F::MultiplyClamp(100.),F::MultiplyClamp(0.01)],L::HistoricalUviFrequencyThreeDecadeCore,0,64,48000).unwrap();
        assert_eq!(p.value(32),400.);assert_eq!(raw.value(32),1000.);
        let p=raw.project_processor(1.,&[F::LowerClamp,F::UpperClamp,F::MultiplyClamp(0.01)],L::HistoricalUviFrequencyThreeDecadeCore,0,64,48000).unwrap();assert_eq!(p.value(32),200.);
        assert!(raw.project_processor(1.,&[F::LowerClamp],L::LegacyCutoffSemitoneMidpoint,0,64,48000).is_err());
    }
    #[test]
    fn historical_fixed_clamp_refuses_a_moving_base_without_publishing_projection(){
        let raw=ControlRamp::test_ramp(1000.,2000.,0,64);
        assert!(raw.project_processor(1.,&[sampler_ir::SourceFrequencyFold::MultiplyClamp(2.)],sampler_ir::SourceProcessingParameterLaw::HistoricalUviFrequencyThreeDecadeCore,0,64,48000).is_err());
        assert_eq!(raw.value(32),1500.);assert!(raw.owner_projection.is_none());
    }

    #[test]
    fn neutral_scoped_cutoff_preserves_unmodulated_base_without_non_neutral_hz_clamp(){
        let raw=ControlRamp::test_ramp(20000.,20000.,0,0);
        let neutral=raw.project_processor(1.,&[],sampler_ir::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint,0,64,40500).unwrap();
        assert_eq!(neutral.value(32),20000.);assert!(PreparedParameter::Control(0).scoped_cutoff_factor(&[neutral]).is_none());
        let routed=raw.project_processor(2.,&[],sampler_ir::SourceProcessingParameterLaw::LegacyCutoffSemitoneMidpoint,0,64,40500).unwrap();
        assert_eq!(routed.value(32),19845.);assert_eq!(PreparedParameter::Control(0).scoped_cutoff_factor(&[routed]),Some(2.));
        assert_eq!(PreparedParameter::Control(0).scoped_cutoff_base(&[routed],32,None),20000.);
    }

}
