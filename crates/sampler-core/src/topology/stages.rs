//! Width-preserving authored core-law stages. Native sampler laws are separate.
use crate::dsp::control::{ControlRamp, PreparedParameter};
use crate::dsp::svf::{core_one_pole_coefficient, core_one_pole_history, one_pole_sample};
use crate::{ControlDomain, ControlId, ControlRange, Error, Prepared};
use sampler_ir as ir;

pub(crate) enum Stage {
    Rectifier(crate::dsp::Rectifier),
    /// Existing typed Toggle projection, inverted only at this outlet.
    Mute { gain:PreparedParameter },
    StaticPan([f64;2]),
    Pan { position:StageParameter, offset:f64, law:ir::PanLaw, expression:bool },
    Gain {
        gain: StageParameter,
    },
    UviOnePole {frequency:StageParameter,tracking:f32,high:bool,history:usize,channels:usize,cadence:ir::UviOnePoleCadence},
    OnePole {
        cutoff: StageParameter,
        high: bool,
        history: usize,
        channels: usize,
    },
}
/// Immutable routing metadata: the base is still the ordinary generation lane.
#[derive(Clone, Copy)]
pub(crate) struct StageParameter {
    pub(super) base: PreparedParameter,
    expression: Option<ExpressionFactor>,
}
#[derive(Clone, Copy)]
struct ExpressionFactor { mapping: PreparedParameter, octaves: bool }
impl StageParameter {
    fn factor(self, parameters: &[ControlRamp], at: u64, expression: &crate::Expression) -> f64 {
        self.expression.map_or(1., |factor| {
            let value = factor.mapping.value(parameters, at, Some(expression));
            if factor.octaves { value.exp2() } else { value }
        })
    }
}
pub(super) fn parameter(
    plan: &Prepared,
    definition: &ir::SignalParameter,
    parameters: &mut Vec<ControlRange>,
    domains: &mut Vec<(ControlId, ControlDomain)>,
    valid: impl Fn(f64) -> bool,
    cutoff: bool,
) -> Result<StageParameter, Error> {
    if !valid(definition.constant) { return Err(Error::InvalidInput); }
    let (base, bounds) = if let Some(key) = &definition.control {
        let id = crate::lower::ir_control_id(key);
        let domain = plan.controls[plan.control_index(id)?].domain;
        let ControlDomain::Real { min, max } = domain else { return Err(Error::InvalidInput); };
        if !valid(min) || !valid(max) { return Err(Error::InvalidInput); }
        plan.real_control(id)?;
        let lane = parameters.len();
        parameters.push(ControlRange { control: id, low: min, high: max, ramp_frames: 0 });
        domains.push((id, domain));
        (PreparedParameter::Control(lane), [min, max])
    } else { (PreparedParameter::Constant(definition.constant), [definition.constant; 2]) };
    let expression = definition.expression.map(|binding| {
        let (zero, one, octaves) = match (cutoff, binding.law) {
            (false, ir::SignalExpressionLaw::LinearFactor { zero, one }) if zero >= 0. && one >= 0. => (zero, one, false),
            (true, ir::SignalExpressionLaw::Octaves { zero, one }) => (zero, one, true),
            _ => return Err(Error::InvalidInput),
        };
        if !zero.is_finite() || !one.is_finite() || !(one - zero).is_finite() { return Err(Error::InvalidInput); }
        // Full constant/control-domain x expression-endpoint admission, including
        // the actual sample-rate cutoff bound supplied by the caller. No clamp.
        for endpoint in [zero, one] {
            let factor = if octaves { endpoint.exp2() } else { endpoint };
            if !factor.is_finite() || (octaves && factor <= 0.) { return Err(Error::InvalidInput); }
            for base in [definition.constant, bounds[0], bounds[1]] {
                if !valid(base * factor) { return Err(Error::InvalidInput); }
            }
        }
        let source = match binding.source {
            ir::SignalExpressionSource::Pressure => crate::ExpressionSource::Pressure,
            ir::SignalExpressionSource::Timbre => crate::ExpressionSource::Timbre,
        };
        Ok(ExpressionFactor { mapping: PreparedParameter::Expression { source, low: zero, high: one }, octaves })
    }).transpose()?;
    Ok(StageParameter { base, expression })
}
impl Stage {
    pub(crate) fn compile(
        plan: &Prepared,
        definition: &ir::ProcessingSignalNode,
        channels: usize,
        histories: &mut usize,
        parameters: &mut Vec<ControlRange>,
        domains: &mut Vec<(ControlId, ControlDomain)>,
    ) -> Result<(Self, u32), Error> {
        let finite = |value: f64| value.is_finite() && value.abs() <= f64::from(f32::MAX);
        match definition {
            ir::ProcessingSignalNode::Mute(mute) => {
                let gain=if let Some(key)=&mute.control {
                    let id=crate::lower::ir_control_id(key);
                    let domain=plan.controls[plan.control_index(id)?].domain;
                    if domain!=ControlDomain::Toggle {return Err(Error::InvalidInput);}
                    let lane=parameters.len();
                    parameters.push(ControlRange{control:id,low:1.,high:0.,ramp_frames:0});
                    domains.push((id,domain));
                    PreparedParameter::Control(lane)
                }else{PreparedParameter::Constant(if mute.muted{0.}else{1.})};
                Ok((Self::Mute{gain},0))
            }
            ir::ProcessingSignalNode::Rectifier(mode) => Ok((Self::Rectifier(match mode {
                ir::Rectifier::Full=>crate::dsp::Rectifier::Full,ir::Rectifier::Half=>crate::dsp::Rectifier::Half}),0)),
            ir::ProcessingSignalNode::Pan(pan) => {
                if channels!=2 || !pan.offset.is_finite() || pan.offset.abs()>f64::from(f32::MAX) || pan.position.expression.is_some() {return Err(Error::InvalidInput);}
                let position=parameter(plan,&pan.position,parameters,domains,
                    |value|value.is_finite() && (-1. ..=1.).contains(&value),false)?;
                if pan.position.control.is_none() && !pan.expression {
                    return Ok((Self::StaticPan(pan.law.gains((pan.position.constant+pan.offset).clamp(-1.,1.))),0));
                }
                Ok((Self::Pan {position,offset:pan.offset,law:pan.law,expression:pan.expression},0))
            }
            ir::ProcessingSignalNode::Gain(gain) => {
                match gain.arithmetic {
                    ir::ChannelGainArithmetic::CoreGainF64ThenF32 => {}
                }
                Ok((
                    Self::Gain {
                        gain: parameter(plan, &gain.gain, parameters, domains, finite, false)?,
                    },
                    0,
                ))
            }
            ir::ProcessingSignalNode::UviOnePole(filter)=>{
                if !filter.tracking.is_finite() || !(0. ..=1.).contains(&filter.tracking){return Err(Error::InvalidInput);}
                let frequency=parameter(plan,&filter.frequency,parameters,domains,|value|finite(value)&&value>0.,true)?;
                let history=*histories;
                // Actual f32 native scalars are stored exactly in the existing
                // f64 history owner. No independent cache or callback allocation.
                let words=8usize.checked_add(if channels==2{8}else{channels}).ok_or(Error::Capacity)?;
                *histories=history.checked_add(words).ok_or(Error::Capacity)?;
                Ok((Self::UviOnePole{frequency,tracking:filter.tracking as f32,high:filter.mode==ir::ChannelOnePoleMode::HighPass,
                    history,channels,cadence:filter.cadence},filter.tail_frames))
            }
            ir::ProcessingSignalNode::OnePole(filter) => {
                match filter.arithmetic {
                    ir::ChannelOnePoleArithmetic::CoreOnePoleF64 => {}
                }
                let valid =
                    |value: f64| finite(value) && value > 0. && value < f64::from(plan.rate) * 0.5;
                let cutoff = parameter(plan, &filter.cutoff_hz, parameters, domains, valid, true)?;
                let history = *histories;
                *histories = history.checked_add(channels).ok_or(Error::Capacity)?;
                Ok((
                    Self::OnePole {
                        cutoff,
                        high: filter.mode == ir::ChannelOnePoleMode::HighPass,
                        history,
                        channels,
                    },
                    filter.tail_frames,
                ))
            }
            ir::ProcessingSignalNode::Matrix(_) | ir::ProcessingSignalNode::Mix(_) | ir::ProcessingSignalNode::BusChain | ir::ProcessingSignalNode::UviLayout(_) => Err(Error::InvalidInput),
        }
    }
    pub(crate) fn render(
        &self,
        block: &mut [f32],
        channels: usize,
        frames: usize,
        stride: usize,
        histories: &mut [f64],
        parameters: &[ControlRamp],
        rate: u32,
        at: u64,
        expression: &crate::Expression,
    ) -> Result<(), Error> {
        let end = channels
            .checked_sub(1)
            .and_then(|c| c.checked_mul(stride))
            .and_then(|n| n.checked_add(frames))
            .ok_or(Error::Capacity)?;
        if frames > stride || block.len() < end {
            return Err(Error::InvalidInput);
        }
        at.checked_add(frames.saturating_sub(1) as u64)
            .ok_or(Error::ClockOverflow)?;
        match *self {
            Self::Mute{gain} => {
                for frame in 0..frames {
                    let value=gain.value(parameters,at+frame as u64,None);
                    if value!=0. && value!=1. {return Err(Error::InvalidInput);}
                    if value==0. {for channel in 0..channels {block[channel*stride+frame]=0.;}}
                }
            }
            Self::StaticPan(gains) => {
                if channels!=2{return Err(Error::InvalidInput);}
                for channel in 0..2{for frame in 0..frames{let index=channel*stride+frame;
                    block[index]=(f64::from(block[index])*gains[channel])as f32;
                    if !block[index].is_finite(){return Err(Error::InvalidInput);}
                }}
            }
            Self::Pan { position,offset,law,expression:home } => {
                if channels!=2 {return Err(Error::InvalidInput);}
                let mut curve=crate::dsp::control::PanCoefficients::new(law);
                for frame in 0..frames {
                    let value=(position.base.value(parameters,at+frame as u64,None)
                        + offset + if home {expression.pan}else{0.}).clamp(-1.,1.);
                    let gains=curve.at(value);
                    for channel in 0..2 {let index=channel*stride+frame;
                        block[index]=(f64::from(block[index])*gains[channel]) as f32;
                        if !block[index].is_finite(){return Err(Error::InvalidInput);}
                    }
                }
            }
            Self::Rectifier(mode) => {
                for channel in 0..channels {for frame in 0..frames {
                    let i=channel*stride+frame;
                    block[i]=mode.apply(f64::from(block[i])) as f32;
                }}
            }
            Self::Gain { gain } => {
                let factor = gain.factor(parameters, at, expression);
                for frame in 0..frames {
                    let gain = gain.base.value(parameters, at + frame as u64, None) * factor;
                    for channel in 0..channels {
                        let value = (f64::from(block[channel * stride + frame]) * gain) as f32;
                        if !value.is_finite() {
                            return Err(Error::InvalidInput);
                        }
                        block[channel * stride + frame] = value;
                    }
                }
            }
            Self::UviOnePole{frequency,tracking,high,history,channels:expected,cadence}=>{
                if channels!=expected{return Err(Error::InvalidInput);}
                if frames==0{return Ok(());}
                let words=8usize.checked_add(if channels==2{8}else{channels}).ok_or(Error::Capacity)?;
                let end=history.checked_add(words).ok_or(Error::Capacity)?;
                let state=histories.get_mut(history..end).ok_or(Error::InvalidInput)?;
                let first=state[6]==0. && cadence==ir::UviOnePoleCadence::FirstSetup32;
                state[6]=1.;
                let mut begin=0;
                // This models the ACTUAL first-setup versus already-prepared
                // invocation branches. No invented absolute clock%32 domain.
                while begin<frames {
                    let count=if first{(frames-begin).min(32)}else{frames-begin};
                    let time=at+begin as u64;
                    let hz=(frequency.base.value(parameters,time,None)*frequency.factor(parameters,time,expression))as f32;
                    let note=state[4]as f32;
                    if state[5]==0. || state[1]as f32!=hz || state[3]as f32!=note {
                        let q=((note-60.)*tracking)*f32::from_bits(0x3daaaaab);
                        let f=(2.0f32.powf(q)*hz).min((rate as f32)*0.5);
                        let exponent=-(f64::from(f)*f64::from_bits(0x401921fb54442d18)/f64::from(rate));
                        let a=exponent.exp()as f32;
                        if !a.is_finite(){return Err(Error::InvalidInput);}
                        state[0]=f64::from(a);state[1]=f64::from(hz);state[3]=f64::from(note);state[5]=1.;
                    }
                    let a=state[0]as f32;let b=1.0f32-a;
                    if channels==2 {
                        let aa=a*a;let ab=a*b;
                        for channel in 0..2 {
                            let base=8+4*channel;
                            let(mut x2,mut x1,mut y2,mut y1)=(state[base]as f32,state[base+1]as f32,state[base+2]as f32,state[base+3]as f32);
                            let mut frame=begin;let end=begin+count;
                            while frame+4<=end {
                                let i=channel*stride+frame;let x=[block[i],block[i+1],block[i+2],block[i+3]];
                                let y0=((x[0]*b)+(x1*ab))+(y2*aa);
                                let y_1=((x[1]*b)+(x[0]*ab))+(y1*aa);
                                let y_2=((x[1]*ab)+(x[2]*b))+(y0*aa);
                                let y_3=((x[2]*ab)+(x[3]*b))+(y_1*aa);
                                for(offset,value)in[y0,y_1,y_2,y_3].into_iter().enumerate(){block[i+offset]=if high{x[offset]-value}else{value};}
                                x2=x[2];x1=x[3];y2=y_2;y1=y_3;frame+=4;
                            }
                            while frame<end {
                                let i=channel*stride+frame;let x=block[i];let y=y1+(x-y1)*b;
                                block[i]=if high{x-y}else{y};x2=x1;x1=x;y2=y1;y1=y;frame+=1;
                            }
                            for(offset,value)in[x2,x1,y2,y1].into_iter().enumerate(){state[base+offset]=f64::from(value);}
                        }
                    }else{
                        for channel in 0..channels {
                            let mut y=state[8+channel]as f32;
                            for frame in begin..begin+count {
                                let i=channel*stride+frame;let x=block[i];y=y+(x-y)*b;block[i]=if high{x-y}else{y};
                            }state[8+channel]=f64::from(y);
                        }
                    }
                    if block.chunks(stride).take(channels).any(|row|row[begin..begin+count].iter().any(|sample|!sample.is_finite())){return Err(Error::InvalidInput);}
                    if count==0{break;}begin+=count;
                }
            }
            Self::OnePole {
                cutoff,
                high,
                history,
                channels: expected,
            } => {
                if channels != expected {
                    return Err(Error::InvalidInput);
                }
                let end = history.checked_add(channels).ok_or(Error::Capacity)?;
                let state = histories.get_mut(history..end).ok_or(Error::InvalidInput)?;
                let factor = cutoff.factor(parameters, at, expression);
                let mut previous = None;
                let mut b = 0.;
                for frame in 0..frames {
                    let hz = cutoff.base.value(parameters, at + frame as u64, None) * factor;
                    if previous != Some(hz) {
                        b = core_one_pole_coefficient(f64::from(rate), hz);
                        previous = Some(hz);
                    }
                    for channel in 0..channels {
                        let input = f64::from(block[channel * stride + frame]);
                        let (output, next) = one_pole_sample(input, state[channel], b, high);
                        let output = output as f32;
                        if !next.is_finite() || !output.is_finite() {
                            return Err(Error::InvalidInput);
                        }
                        state[channel] = next;
                        block[channel * stride + frame] = output;
                    }
                }
                if frames != 0 {
                    for value in state {
                        *value = core_one_pole_history(*value);
                    }
                }
            }
        }
        Ok(())
    }
}


#[cfg(test)]
mod pan_preparation_work_tests {
    use super::*;
    #[test]
    fn unbound_signal_pan_precomputes_curve_before_render() {
        let plan=crate::Prepared::new(48000,vec![],vec![],0).unwrap();
        let definition=ir::ProcessingSignalNode::Pan(ir::ChannelPan{offset:0.125,position:ir::SignalParameter{constant:0.25,control:None,expression:None},law:ir::PanLaw::RaisedCosineCoreF64,expression:false});
        let mut words=0;let mut parameters=Vec::new();let mut domains=Vec::new();
        let(stage,tail)=Stage::compile(&plan,&definition,2,&mut words,&mut parameters,&mut domains).unwrap();
        let Stage::StaticPan(gains)=stage else{panic!("unbound signal Pan retained a render-time curve")};
        assert_eq!(tail,0);assert_eq!(words,0);assert!(parameters.is_empty()&&domains.is_empty());
        let expected=(0.375*std::f64::consts::FRAC_PI_2).cos().powi(2);
        assert!((gains[0]-expected).abs()<1e-15);assert_eq!(gains[1],1.);
        let mut block=[0.25f32;128];block[64..].fill(-0.125);
        Stage::StaticPan(gains).render(&mut block,2,64,64,&mut[],&[],48000,0,&Default::default()).unwrap();
        for frame in 0..64{assert!((block[frame]-((0.25*expected)as f32)).abs()<1e-7);assert_eq!(block[64+frame],-0.125);}
    }
}


impl Stage {
    pub(crate) fn prepare_native_note(&self,histories:&mut[f64],note:f64)->Result<(),Error>{
        if let Self::UviOnePole{history,..}=*self {
            if !note.is_finite() || !(0. ..128.).contains(&note){return Err(Error::InvalidInput);}
            *histories.get_mut(history+4).ok_or(Error::InvalidInput)?=f64::from(note as f32);
        }Ok(())
    }
    pub(crate) fn scope_control_lane(&self,field:crate::SourceOwnerControl)->Option<usize>{
        if field==crate::SourceOwnerControl::Pan && !matches!(self,Self::Pan{expression:true,..}){return None;}
        if field==crate::SourceOwnerControl::OutputMute{return None;}
        self.owner_control_lane(field)
    }
    pub(crate) fn owner_outlet_kind(&self)->Option<crate::SourceOwnerControl>{
        match self{Self::Gain{..}=>Some(crate::SourceOwnerControl::Gain),Self::Pan{..}|Self::StaticPan(_)=>Some(crate::SourceOwnerControl::Pan),Self::Mute{..}=>Some(crate::SourceOwnerControl::OutputMute),_=>None}
    }
    pub(crate) fn owner_control_lane(&self,field:crate::SourceOwnerControl)->Option<usize>{
        let base=match (self,field){
            (Self::Gain{gain,..},crate::SourceOwnerControl::Gain)=>gain.base,
            (Self::Pan{position,..},crate::SourceOwnerControl::Pan)=>position.base,
            (Self::Mute{gain},crate::SourceOwnerControl::OutputMute)=>*gain,
            _=>return None,
        };
        if let PreparedParameter::Control(lane)=base{Some(lane)}else{None}
    }
}
