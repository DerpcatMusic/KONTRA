use super::*;

fn group(assignment: i32) -> Group {
    let mut public = 0u32.to_le_bytes().to_vec();
    for v in [1f32, 0., 1.] {
        public.extend(v.to_le_bytes());
    }
    public.extend([1, 0, 0, 0]);
    public.extend(0i32.to_le_bytes());
    public.extend((-1i16).to_le_bytes());
    public.extend(assignment.to_le_bytes());
    public.extend(0i32.to_le_bytes());
    public.extend([0, 0]);
    public.extend(0i32.to_le_bytes());
    Group(StructuredObject {
        version: 0x95,
        private_data: vec![],
        public_data: public,
        children: vec![ni_file::kontakt::Chunk {
            id: 0x38,
            data: 0u32.to_le_bytes().to_vec(),
        }],
    })
}

fn limit(voices: i32, kill: i16, fade: i32) -> Vec<u8> {
    let mut wire = vec![0, 0x60, 0];
    wire.extend(0u32.to_le_bytes());
    wire.extend(kill.to_le_bytes());
    wire.push(1);
    for v in [voices, fade, -1] {
        wire.extend(v.to_le_bytes());
    }
    wire
}

#[test]
fn omitted_voice_group_override_still_binds_its_native_shared_limit() {
    let mut out = Translation {
        ir: ir::Instrument::default(),
        assets: HashMap::new(),
        locations: vec![],
        start_criteria: vec![],
        voice_groups: vec![],
        snapshot_groups: vec![],
        engine: vec![],
        dynamic: false,
        eq_mod_slots: HashMap::new(),
        cutoff_octaves: HashMap::new(),
        route_intensity: HashMap::new(),
        intensity_scales: Vec::new(),
        send_taps: vec![],
        #[cfg(feature = "scan")]
        target_outcomes: HashMap::new(),
    };
    out.ir.kontakt_objects = Some(Box::new(ir::kontakt::Objects {
        program: crate::objects::program(0xae, &Default::default()),
        voice_groups: None,
        groups: vec![],
        zones: vec![],
    }));
    let mut wire = limit(32, 1, 42);
    let mut mask = [0; 16];
    mask[8] = 1; // Only physical override 64 is serialized.
    wire.extend(mask);
    wire.extend(limit(7, 2, 99));
    out.voice_groups(&wire).unwrap();
    for (index, assignment) in [128, 128, 65, 0, 129].into_iter().enumerate() {
        out.group(index, &group(assignment)).unwrap();
    }
    let native = out.ir.groups[0]
        .voice_limit
        .expect("clear mask bit is a predefined native voice group");
    assert_eq!(
        out.ir.groups[1].voice_limit,
        Some(native),
        "the same source voice group shares one limit"
    );
    assert_eq!(
        out.ir.voice_limits[native],
        ir::VoiceLimit {
            voices: 1,
            kill: ir::Kill::Oldest,
            prefer_released: true,
            fade: ir::Time::Milliseconds(10.),
            exclusive_set: None,
        }
    );
    let explicit = out.ir.groups[2].voice_limit.unwrap();
    assert_ne!(explicit, native);
    assert_eq!(out.ir.voice_limits[explicit].voices, 7);
    assert_eq!(out.ir.voice_limits[explicit].kill, ir::Kill::Newest);
    assert_eq!(
        out.ir.voice_limits.len(),
        2,
        "unused defaults need no runtime storage"
    );
    assert!(out.ir.groups[3].voice_limit.is_none());
    assert!(out.ir.groups[4].voice_limit.is_none());
    assert_eq!(
        out.ir
            .unsupported
            .iter()
            .filter(|u| u.feature == "undefined voice group")
            .count(),
        1
    );
    let source = out.ir.kontakt_objects.unwrap().voice_groups.unwrap();
    assert!(
        source.groups[127].is_none(),
        "authored mask holes remain absent overrides"
    );
    assert_eq!(source.groups[64].as_ref().unwrap().max_num_voices, 7);
}
