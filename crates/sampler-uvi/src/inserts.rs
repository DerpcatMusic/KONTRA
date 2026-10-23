//! UVI insert effects as IR processors: aux-bus chains, effect racks and
//! keygroup inserts. Exact catalog fields with supported native DSP lanes
//! receive live controls in `engine_parameters`; other writes stay reported.
use super::{Translation, engine_parameters, number, path};
use roxmltree::Node;
use sampler_ir as ir;

fn db(db: f64) -> ir::Gain {
    ir::Gain::Linear(10f64.powf(db / 20.0))
}

/// Where an insert element's processors sit in its chain, so a script that
/// writes the insert's parameters can reach them. Every insert element has an
/// entry, a bypassed or unmodeled one with `count` 0. A DigitalEq's range is
/// one processor per enabled non-flat band in band order, then its
/// OverallGain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertNode {
    /// The element's `roxmltree::NodeId::get_usize()`.
    pub node: usize,
    pub chain: ir::ChainRef,
    pub first: usize,
    pub count: usize,
}

/// An insert's processors before its chain exists: element, first, count.
pub(super) type Placed = (usize, usize, usize);

impl Translation {
    /// One actual Program/Layer FX owner and one memoryless outlet on the
    /// same owner graph. The explicit current Core stereo port is independent
    /// of source file cardinality and of any native descriptor enum.
    pub(super) fn global_scope_bus(
        &mut self, parent:Node, output:ir::Output, input_owner:ir::SourceOwnerId,
    )->Result<(ir::BusRef,ir::BusRef),String>{
        let at=path(parent);let node=parent.id().get_usize();
        let owner=ir::SourceOwnerId::UviNode(u32::try_from(node).map_err(|_|format!("{at}: scope owner identity overflow"))?);
        let gain=number(parent,"Gain",1.)?;let pan=number(parent,"Pan",0.)?;
        if !gain.is_finite() || !(0. ..=f64::from(f32::MAX)).contains(&gain)
            || !pan.is_finite() || !(-1. ..=1.).contains(&pan){return Err(format!("{at}: global outlet scalar outside supported real domain"));}
        let mut modulation=super::modulation::Modulation::default();
        for connection in super::connections(parent).filter(|connection|matches!(connection.attribute("Destination"),Some("Gain"|"Pan"))){
            self.connect_scoped(connection,&mut modulation)?;
        }
        // This finite source family has a proved default-state controller
        // owner. Note/Script/LFO sources require their actual creator/context
        // law; no zero-filled voice or copied member state is substituted.
        let mut sources=Vec::new();
        for &route in &modulation.routes {
            let route=&self.ir.routes[route.0];
            for source in std::iter::once(route.source).chain(route.scale.map(|scale|scale.source)) {
                if !matches!(self.ir.modulators[source.0].source,ir::ModulationSource::Controller(number)if number<128)
                    &&!matches!(self.ir.modulators[source.0].source,ir::ModulationSource::ConstantProperty{..}|ir::ModulationSource::Constant|ir::ModulationSource::Script(_)|ir::ModulationSource::ScriptBipolar(_)) {
                    let source = match &self.ir.modulators[source.0].source {
                        ir::ModulationSource::Script(id) => format!("ScriptEventModulation EventId={id} Bipolar=0"),
                        ir::ModulationSource::ScriptBipolar(id) => format!("ScriptEventModulation EventId={id} Bipolar=1"),
                        ir::ModulationSource::Envelope(_) => "Envelope".into(),
                        ir::ModulationSource::Breakpoints(_) => "Breakpoints".into(),
                        ir::ModulationSource::Follower(_) => "EnvelopeFollower".into(),
                        ir::ModulationSource::Glide(_) => "Glide".into(),
                        ir::ModulationSource::Lfo(_) => "LFO".into(),
                        ir::ModulationSource::Velocity => "VoiceParam Velocity".into(),
                        ir::ModulationSource::Key => "VoiceParam Key".into(),
                        ir::ModulationSource::PitchBend => "PitchBend".into(),
                        ir::ModulationSource::ChannelPressure => "ChannelPressure".into(),
                        ir::ModulationSource::PolyPressure => "PolyPressure".into(),
                        ir::ModulationSource::Timbre => "Timbre".into(),
                        ir::ModulationSource::Random => "Random".into(),
                        ir::ModulationSource::ReleaseCounter(_) => "ReleaseCounter".into(),
                        ir::ModulationSource::Controller(cc) => format!("Controller {cc}"),
                        ir::ModulationSource::Constant | ir::ModulationSource::ConstantProperty { .. } => "Constant".into(),
                    };
                    return Err(format!("{at}: live global Gain/Pan requires the actual shared owner modulator; source {source}, destination {:?}, scope {:?}; shared source lifecycle/input actor is not implemented", route.target, self.ir.modulators[route.source.0].scope));
                }
                if !sources.contains(&source){sources.push(source);}
            }
        }
        let gain_key=engine_parameters::scope_gain_key(node);let pan_key=engine_parameters::scope_pan_key(node);
        let mute_key=engine_parameters::scope_mute_key(node);let midi_key=engine_parameters::scope_midi_mute_key(node);
        let muted=number(parent,"Mute",0.)?!=0.;let midi_muted=number(parent,"MidiMute",0.)?!=0.;
        let gain_control=ir::ControlRef(self.ir.controls.len());
        let pan_control=ir::ControlRef(self.ir.controls.len()+1);
        let mute_control=ir::ControlRef(self.ir.controls.len()+2);
        for (key,label,min,max,default)in [
            (gain_key.clone(),"Scope Gain",0.,f64::from(f32::MAX),gain),
            (pan_key.clone(),"Scope Pan",-1.,1.,pan),
        ]{self.ir.controls.push(ir::Control{key,label:label.into(),value:ir::ControlValue::Continuous{min,max,default,unit:ir::ControlUnit::None},automation:ir::Automation::None});}
        for (key,label,default)in [(mute_key.clone(),"Scope output Mute",muted),(midi_key,"Scope MidiMute reception",midi_muted)]{
            self.ir.controls.push(ir::Control{key,label:label.into(),value:ir::ControlValue::Toggle{default},automation:ir::Automation::None});
        }
        let input=ir::BusRef(self.ir.buses.len());let outlet=ir::BusRef(input.0+1);
        let (mut processors,mut placed)=self.inserts(parent,false,None)?;
        if !processors.is_empty(){
            let count=u16::try_from(processors.len()).map_err(|_|format!("{at}: global insert count overflow"))?;
            processors.insert(0,ir::Processor::Mix{count,address:ir::SlotAddress{group:-1,slot:-1,generic:i32::try_from(node).map_err(|_|format!("{at}: global insert address overflow"))?},dry:0.,wet:1.,bypass:number(parent,"BypassInsertFX",0.)?!=0.});
            for (_,first,_)in &mut placed{*first+=1;}
        }
        let has_chain=!processors.is_empty();let chain=ir::ChainRef(self.ir.chains.len());
        self.ir.chains.push(ir::Chain{scope:ir::Scope::Bus(input),pre_amplitude:processors,post_amplitude:Vec::new()});self.place(chain,placed);
        self.ir.buses.push(ir::Bus{name:format!("{} FX",parent.attribute("Name").unwrap_or(parent.tag_name().name())),chain:Some(chain),sends:vec![],output:ir::Output::Bus(outlet),gain:ir::Gain::UNITY});
        self.ir.buses.push(ir::Bus{name:format!("{} outlet",parent.attribute("Name").unwrap_or(parent.tag_name().name())),chain:None,sends:vec![],output,gain:ir::Gain::UNITY});
        let identity=ir::ProcessingSignalNode::Matrix(ir::ChannelMatrix{input:ir::SignalLayout{channels:2},output:ir::SignalLayout{channels:2},cells:vec![1.,0.,0.,1.],arithmetic:ir::MatrixArithmetic::SeparateMulAddF32});
        self.ir.processing_buses.push(ir::ProcessingBus{bus:input,owner:Some(owner),signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes:vec![if has_chain{ir::ProcessingSignalNode::BusChain}else{identity}],controls:vec![]},output:ir::SignalLayout{channels:2}});
        let mut nodes=vec![ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:ir::SignalParameter{constant:gain,control:Some(gain_key),expression:None},arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32})];
        if modulation.gain!=1.{nodes.push(ir::ProcessingSignalNode::Gain(ir::ChannelGain{gain:ir::SignalParameter{constant:modulation.gain,control:None,expression:None},arithmetic:ir::ChannelGainArithmetic::CoreGainF64ThenF32}));}
        let pan_node=nodes.len();
        nodes.push(ir::ProcessingSignalNode::Pan(ir::ChannelPan{offset:modulation.pan,position:ir::SignalParameter{constant:pan,control:Some(pan_key),expression:None},law:ir::PanLaw::RaisedCosineCoreF64,expression:false}));
        nodes.push(ir::ProcessingSignalNode::Mute(ir::ChannelMute{muted,control:Some(mute_key)}));
        let mute_node=nodes.len()-1;
        for (field,node,control)in [(ir::SourceOwnerOutputField::Gain,0,gain_control),(ir::SourceOwnerOutputField::Pan,pan_node,pan_control),(ir::SourceOwnerOutputField::Mute,mute_node,mute_control)]{
            self.ir.source_indices.output_controls.push(ir::SourceOwnerOutputControl{owner,field,target:ir::SourceOwnerControlTarget::Bus{bus:outlet,node},control});
        }
        self.ir.processing_buses.push(ir::ProcessingBus{bus:outlet,owner:Some(owner),signal:ir::ProcessingSignal{source_law:ir::ProcessingSourceLaw::CoreF64GainAmplitudeF32Outlet,input:ir::SignalLayout{channels:2},nodes,controls:vec![]},output:ir::SignalLayout{channels:2}});
        if !modulation.routes.is_empty(){
            self.ir.source_indices.global_modulation.push(ir::SourceGlobalModulation{owner,input_owner,outlet,sources,routes:modulation.routes});
        }
        Ok((input,outlet))
    }

    /// Record `placed` entries (relative to one chain) once the chain is `chain`.
    pub(super) fn place(&mut self, chain: ir::ChainRef, placed: Vec<Placed>) {
        self.insert_nodes
            .extend(placed.into_iter().map(|(node, first, count)| InsertNode {
                node,
                chain,
                first,
                count,
            }));
    }

    /// The enabled inserts of `parent` (an AuxEffect or a Keygroup), in order.
    /// `voice` chains have no convolution; `key` is the middle key of a
    /// keygroup, for key-tracked cutoffs.
    pub(super) fn inserts(
        &mut self,
        parent: Node,
        voice: bool,
        key: Option<u8>,
    ) -> Result<(Vec<ir::Processor>, Vec<Placed>), String> {
        let mut out = Vec::new();
        let mut placed = Vec::new();
        let Some(inserts) = parent.children().find(|n| n.has_tag_name("Inserts")) else {
            return Ok((out, placed));
        };
        // A scripted program's inserts sit inside a Mix so `setParameter
        // ("Bypass")` can reach them at runtime (core `slot_control` path), and
        // its bus-scope inserts get the control-driven stages that
        // `engine_parameters::lanes` addresses.
        let scripted = !self.ir.behaviors.is_empty();
        let global=matches!(parent.tag_name().name(),"Program"|"Layer");
        let live = (scripted || global) && !voice;
        // An `.fxps` effect preset (`<FXPreset><Effect/></FXPreset>`) stands for its effect.
        let nodes: Vec<Node> = inserts
            .children()
            .filter(|n| n.is_element())
            .flat_map(|n| {
                if n.has_tag_name("FXPreset") { n.children().filter(|c| c.is_element()).collect() } else { vec![n] }
            })
            .collect();
        for node in nodes {
            let slot = out.len();
            let id = node.id().get_usize();
            let bypassed = number(node, "Bypass", 0.0)? != 0.0;
            // A keygroup chain is retained once even without an initial script.
            // A saved bypass must not erase the processor/history/control home.
            let mix = (scripted || voice || global) && engine_parameters::bypassable(node);
            if bypassed && !mix {
                // Exactly silent: nothing to translate (a script that enables it
                // reports its write).
                placed.push((id, slot, 0));
                self.used.insert(node.id());
                continue;
            }
            if mix {
                out.push(ir::Processor::Mix {
                    count: 0,
                    address: ir::SlotAddress {
                        group: -1,
                        slot: -1,
                        generic: i32::try_from(id).unwrap_or(-1),
                    },
                    dry: 0.0,
                    wet: 1.0,
                    bypass: bypassed,
                });
            }
            let first = out.len();
            let at = path(node);
            match node.tag_name().name() {
                "Gain" => {
                    out.push(ir::Processor::Gain(ir::Gain::Linear(number(
                        node, "Volume", 1.0,
                    )?)));
                }
                "GainMatrix" => {
                    // Gain_i_j: input i to output j. Channels 1 and 2 are the
                    // stereo pair; a stereo signal never feeds the others.
                    let g = |i: usize, j: usize| {
                        number(node, &format!("Gain_{i}_{j}"), f64::from(u8::from(i == j)))
                    };
                    out.push(ir::Processor::StereoMatrix([
                        [g(1, 1)?, g(2, 1)?],
                        [g(1, 2)?, g(2, 2)?],
                    ]));
                    let mut beyond = false;
                    for i in 1..=12 {
                        for j in 1..=12 {
                            if (i > 2 || j > 2) && g(i, j)? != f64::from(u8::from(i == j)) {
                                beyond = true;
                            }
                        }
                    }
                    // A keygroup's complete matrix is retained by the normal
                    // source recipe. Geometry resolves after decode, then the
                    // actual N-port graph must compile or loading fails.
                    if beyond && !voice {
                        self.unsupported(&at, "GainMatrix beyond the first two channels", "");
                    }
                }
                "DigitalEq" => self.digital_eq(node, &at, live, &mut out)?,
                "ThreeBandShelves" => self.three_band_shelves(node, &mut out)?,
                "TwoBandEq" | "ThreeBandEq" | "ParametricEQ" | "Parametric" => self.legacy_eq(node, &at, &mut out)?,
                "WaveShaper" => {
                    // DSP_FORMAT_SPECIFICATION "WaveShaper rectifier kernels":
                    // internal modes 6 (full) and 7 (half). Public numbering
                    // through the saved Mode is assumed equal to them.
                    // ponytail: unverified mapping; other modes, the pre/post
                    // filters, Amount, Knee and oversampling are not modelled.
                    let mode = number(node, "Mode", 0.0)?;
                    let rectifier = match mode {
                        6.0 => ir::Rectifier::Full,
                        7.0 => ir::Rectifier::Half,
                        _ => {
                            self.unsupported(&at, "WaveShaper mode", mode);
                            out.truncate(slot);
                            placed.push((id, slot, 0));
                            continue;
                        }
                    };
                    for (name, why, off) in [
                        ("Mix", "WaveShaper mix", 1.0),
                        ("PreFreq", "WaveShaper pre filter", 20000.0),
                        ("PostFreq", "WaveShaper post filter", 20.0),
                    ] {
                        let v = number(node, name, off)?;
                        if v != off {
                            self.unsupported(&at, why, v);
                        }
                    }
                    out.push(ir::Processor::Gain(db(number(node, "InputGain", 0.0)?)));
                    out.push(ir::Processor::Color(ir::fx::color::Config::rectifier(rectifier)));
                    out.push(ir::Processor::Gain(db(number(node, "OutputGain", 0.0)?)));
                }
                "BitCrusher" => {
                    // Kernel 0x1412d1770; the public fields are the kernel's
                    // members unchanged (ctor defaults 22050, 16, 0, 1; setters
                    // 0x141041710.., Cutoff 0x1412d1730 -> tilt filter).
                    let hertz = number(node, "EffectiveSampleRate", 22050.0)?;
                    let bits = number(node, "BitSize", 16.0)?;
                    let drive = number(node, "Drive", 0.0)?;
                    let cutoff = number(node, "Cutoff", 0.0)?;
                    let mix = number(node, "Mix", 1.0)?;
                    if !(hertz >= 1.0
                        && (1.0..=32.0).contains(&bits)
                        && (0.0..=1.0).contains(&drive)
                        && (-1.0..=1.0).contains(&cutoff)
                        && (0.0..=1.0).contains(&mix))
                    {
                        self.unsupported(&at, "BitCrusher value outside its public range", "");
                        out.truncate(slot);
                        placed.push((id, slot, 0));
                        continue;
                    }
                    out.push(ir::Processor::Color(ir::fx::color::Config::bit_crusher(
                        hertz,
                        bits,
                        drive,
                        cutoff as f32,
                        mix,
                    )));
                }
                "FormantCrusher" => {
                    out.push(ir::Processor::Color(ir::fx::color::Config::formant_crusher(
                        number(node, "Morph", 0.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "Q", 10.0)?.clamp(10.0, 20.0) as f32,
                        number(node, "Formant", 0.0)?.clamp(-1.0, 1.0) as f32,
                        number(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "Bite", 0.0)?.clamp(0.0, 1.0) as f32,
                        number(node, "FilterA", 0.0)?.clamp(0.0, 10.0) as u8,
                        number(node, "FilterB", 2.0)?.clamp(0.0, 10.0) as u8,
                    )));
                }
                "DiodeClipper" => {
                    use ir::fx::color::{Config, Curve, Diode};
                    let diode = Diode {
                        drive: number(node, "Drive", 0.0)?.clamp(0.0, 30.0) as f32,
                        tone: number(node, "Tone", 20_000.0)?.clamp(100.0, 20_000.0) as f32,
                        high_pass: number(node, "HighPass", 1.0)?.clamp(1.0, 20_000.0) as f32,
                        asymmetry: number(node, "Asymmetry", 0.0)?.clamp(0.0, 1.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Diode(diode), ..Config::IDENTITY }));
                }
                "ToneStack" => {
                    use ir::fx::color::{Config, Curve, ToneStack};
                    let unit = |name, default| -> Result<f32, String> { Ok(number(node, name, default)?.clamp(0.0, 1.0) as f32) };
                    let stack = ToneStack {
                        bass: unit("Bass", 0.5)?,
                        mid: unit("Mid", 0.5)?,
                        treble: unit("Treble", 0.5)?,
                        gain: unit("Gain", 0.3)?,
                        model: number(node, "Model", 0.0)?.clamp(0.0, 25.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::ToneStack(stack), ..Config::IDENTITY }));
                }
                "Exciter" => {
                    use ir::fx::color::{Config, Curve, Exciter};
                    let exciter = Exciter {
                        mode: number(node, "Mode", 1.0)?.clamp(0.0, 1.0) as u8,
                        input_gain: number(node, "InputGain", 0.0)?.clamp(-30.0, 20.0) as f32,
                        amount: number(node, "Amount", 0.0)?.clamp(0.0, 15.0) as f32,
                        mix: number(node, "Mix", 100.0)?.clamp(0.0, 100.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                        oversampling: number(node, "Oversampling", 0.0)?.clamp(0.0, 4.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Exciter(exciter), ..Config::IDENTITY }));
                }
                "Widener" => {
                    use ir::fx::color::{Config, Widener};
                    let widener = Widener {
                        delay: number(node, "Delay", 0.0)?.clamp(-15.0, 15.0) as f32,
                        width: number(node, "Width", 0.0)?.clamp(-1.0, 1.0) as f32,
                        shelf_hertz: number(node, "HighShelfFreqSide", 10_000.0)?.clamp(2000.0, 20_000.0) as f32,
                        shelf_db: number(node, "HighSelfGainSide", 0.0)?.clamp(-15.0, 15.0) as f32,
                    };
                    out.push(ir::Processor::Color(Config { widener: Some(widener), ..Config::IDENTITY }));
                }
                "Redux2" => {
                    use ir::fx::color::{Config, Redux};
                    let redux = Redux {
                        frequency: number(node, "Frequency", 44_100.0)?.clamp(200.0, 44_100.0) as f32,
                        bits: number(node, "Bits", 32.0)?.clamp(1.0, 32.0) as f32,
                        jitter: number(node, "Jitter", 0.0)?.clamp(0.0, 1.0) as f32,
                        dither: number(node, "Dither", 0.0)?.clamp(0.0, 2.0) as f32,
                        mu_law: number(node, "QuantizeLaw", 0.0)? != 0.0,
                        // Type is registered inline (not in the parameter table); 1 holds.
                        impulses: number(node, "Type", 1.0)? == 0.0,
                        antialias: number(node, "Antialiasing", 1.0)? != 0.0,
                        band_limit: number(node, "BandLimitFreq", 20_000.0)?.clamp(200.0, 20_000.0) as f32,
                    };
                    let position = number(node, "FilterPosition", 0.0)?.clamp(0.0, 2.0) as u8;
                    let filter = (position != 0).then_some(()).map(|()| -> Result<_, String> {
                        Ok((
                            number(node, "FilterMode", 0.0)?.clamp(0.0, 2.0) as u8,
                            number(node, "FilterFreq", 10_000.0)?.clamp(20.0, 20_000.0) as f32,
                            number(node, "FilterRes", 0.0)?.clamp(0.0, 1.0) as f32,
                            position == 2,
                        ))
                    }).transpose()?;
                    let mix = number(node, "Mix", 1.0)?.clamp(0.0, 1.0) as f32;
                    out.push(ir::Processor::Color(Config::redux(redux, filter, mix)));
                }
                "GarageFuzz" | "SmoothOverdrive" | "StraightFuzz" => {
                    // Native drive engines (color chain Curve::Drive with the HIIR stage).
                    use ir::fx::color::{Config, Curve, Drive, DriveKind};
                    let drive = Drive {
                        kind: match node.tag_name().name() {
                            "GarageFuzz" => DriveKind::Garage,
                            "SmoothOverdrive" => DriveKind::Smooth,
                            _ => DriveKind::Straight,
                        },
                        input_gain: number(node, "InputGain", 0.0)?.clamp(-30.0, 20.0) as f32,
                        overdrive: number(node, "Overdrive", 0.0)?.clamp(0.0, 10.0) as f32,
                        trash: number(node, "Trash", 0.0)?.clamp(0.0, 1.0) as f32,
                        mix: number(node, "Mix", 100.0)?.clamp(0.0, 100.0) as f32,
                        output_gain: number(node, "OutputGain", 0.0)?.clamp(-40.0, 10.0) as f32,
                        oversampling: number(node, "Oversampling", 0.0)?.clamp(0.0, 4.0) as u8,
                    };
                    out.push(ir::Processor::Color(Config { curve: Curve::Drive(drive), ..Config::IDENTITY }));
                }
                k if crate::fx::dynamics::handles(k) => {
                    crate::fx::dynamics::translate(self, node, &at, &mut out)?;
                }
                tag if super::fx::filter::handles(tag) => {
                    match super::fx::filter::translate(node, key, |name, default| {
                        number(node, name, default)
                    })? {
                        Ok(done) => {
                            out.extend(done.processors);
                            for (feature, value) in done.unsupported {
                                self.unsupported(&at, feature, value);
                            }
                        }
                        Err(gap) => {
                            self.unsupported(&at, &format!("{} ({})", gap.name, gap.reason), "");
                            out.truncate(slot);
                            placed.push((id, slot, 0));
                            continue;
                        }
                    }
                }
                "Convolver" | "SampledReverb" if !voice => {
                    let processor = self.impulse_player(node, &at)?;
                    out.push(processor);
                }
                "PlainReverb" | "GateReverb" | "FilteredReverb" | "DelayedReverb" | "SparkVerb" | "Bloom" | "Diffusion"
                    if !voice =>
                {
                    out.extend(self.algorithmic_reverb(node, &at)?);
                }
                "TrackDelay" | "FxDelay" | "FatDelay" | "SimpleDelay" | "StereoDelay" | "PingPongDelay" | "DualDelay" | "DiffuseDelay"
                | "DualDelayX" | "TapeEcho" | "DiffuseDelayNested" | "Buzz" | "Chorus" | "Flanger" | "AnalogChorus" | "AnalogFlanger"
                | "WhiteChorus" | "Ensemble" | "Phaser" | "CrossPhaser" | "Phasor" | "PhasorFilter" => {
                    if let Some(config) = self.timemod(node, &at)? {
                        out.push(ir::Processor::TimeMod(config));
                    }
                }
                "EffectRack" => {
                    // Chains are parallel branches summed at their own gains
                    // (one is just a serial section at that gain). A scripted
                    // bus keeps every chain, bypassed or silent, because its
                    // gain stage is a control the script can raise; a chain's
                    // bypass is that gain at zero.
                    let chains: Vec<_> = node
                        .children()
                        .filter(|n| n.has_tag_name("Chains"))
                        .flat_map(|c| c.children().filter(|c| c.has_tag_name("AuxEffect")))
                        .filter(|c| {
                            live || (number(*c, "Bypass", 0.0).is_ok_and(|b| b == 0.0)
                                && number(*c, "Gain", 1.0).is_ok_and(|g| g != 0.0))
                        })
                        .collect();
                    // A rack sums its active branches. Removing every silent
                    // branch must leave zero, rather than the unprocessed input.
                    if chains.is_empty() {
                        out.push(ir::Processor::Gain(ir::Gain::Linear(0.0)));
                    }
                    for (n, chain) in chains.iter().enumerate() {
                        let off = live && number(*chain, "Bypass", 0.0)? != 0.0;
                        let gain = ir::Gain::Linear(if off {
                            0.0
                        } else {
                            number(*chain, "Gain", 1.0)?
                        });
                        let (inner, entries) = self.inserts(*chain, voice, key)?;
                        if let [_] = chains[..] {
                            out.push(ir::Processor::Gain(gain));
                        } else if live {
                            out.push(ir::Processor::Branch {
                                count: (inner.len() + 1) as u16,
                                gain: ir::Gain::UNITY,
                                first: n == 0,
                                last: n + 1 == chains.len(),
                            });
                            out.push(ir::Processor::Gain(gain));
                        } else {
                            out.push(ir::Processor::Branch {
                                count: inner.len() as u16,
                                gain,
                                first: n == 0,
                                last: n + 1 == chains.len(),
                            });
                        }
                        if live {
                            placed.push((chain.id().get_usize(), out.len() - 1, 1));
                        }
                        let base = out.len();
                        placed.extend(entries.into_iter().map(|(n, f, c)| (n, base + f, c)));
                        out.extend(inner);
                    }
                }
                other => {
                    if let Some(why) = crate::fx::color::unproved(other) {
                        self.unsupported(&at, other, why);
                    }
                    out.truncate(slot);
                    placed.push((id, slot, 0));
                    continue;
                }
            }
            if live && engine_parameters::lane_family(node) {
                // Each unit of the element listens at its own lane slot.
                for (k, p) in out[first..].iter_mut().enumerate() {
                    *p = p.at(engine_parameters::lane_slot(id, k));
                }
            }
            let count = out.len() - first;
            if count == 0 && mix {
                out.truncate(slot);
                placed.push((id, slot, 0));
            } else {
                if let (true, Some(ir::Processor::Mix { count: c, .. })) = (mix, out.get_mut(slot))
                {
                    *c = count as u16;
                }
                placed.push((id, first, count));
            }
            self.used.insert(node.id());
        }
        Ok((out, placed))
    }
}

/// Decode each impulse that names an asset through `read` (rate and stereo
/// frames), before unused assets are dropped. A file that cannot be read
/// leaves the unit-impulse placeholder (the wet path then passes the input)
/// and is reported.
pub(super) fn fill_impulses(
    instrument: &mut ir::Instrument,
    locations: &[String],
    mut read: impl FnMut(usize) -> Result<(u32, Vec<[f32; 2]>), String>,
) {
    for index in 0..instrument.impulses.len() {
        let Some(asset) = instrument.impulses[index].asset.take() else {
            continue;
        };
        match read(asset.0) {
            Ok((rate, frames)) if !frames.is_empty() && frames.len() <= 1 << 24 => {
                let impulse = &mut instrument.impulses[index];
                impulse.rate = rate;
                impulse.left = frames.iter().map(|f| f[0]).collect();
                impulse.right = frames.iter().map(|f| f[1]).collect();
                if let Some(Some(shape)) = instrument.impulse_shapes.get(index) {
                    let (left, right) = sampler_core::shape_impulse(rate, &impulse.left, &impulse.right, shape);
                    (impulse.left, impulse.right) = (left, right);
                }
            }
            other => instrument.unsupported.push(ir::Unsupported {
                location: locations.get(asset.0).cloned().unwrap_or_default(),
                feature: "unreadable impulse response (passed through)".into(),
                value: other.err().unwrap_or_else(|| "empty or too long".into()),
                reason: ir::Reason::InvalidValue,
            }),
        }
    }
}
