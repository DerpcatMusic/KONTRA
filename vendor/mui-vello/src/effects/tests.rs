use super::pool::{ContentState, bucket};
use mui_weld::analytic::{PARAM_BYTES, dirty_ranges};
#[test]
fn aborted_encoder_does_not_mark_texture_rendered() {
    let a = [1; PARAM_BYTES];
    let b = [2; PARAM_BYTES];
    let mut state = ContentState {
        uploaded: Some(a),
        rendered: Some(a),
        pending: None,
    };
    state.uploaded = Some(b);
    state.pending = Some(b);
    state.aborted();
    assert!(state.needs_render(&b));
    assert_eq!(dirty_ranges(state.uploaded.as_ref(), &b).count(), 0);
}
#[test]
fn submission_commits_but_does_not_claim_gpu_completion() {
    let p = [3; PARAM_BYTES];
    let mut s = ContentState {
        pending: Some(p),
        ..Default::default()
    };
    assert!(s.needs_render(&p));
    s.submitted();
    assert!(!s.needs_render(&p));
    assert!(s.pending.is_none());
}
#[test]
fn return_to_old_value_after_abort_reuses_the_old_texture() {
    let a = [1; PARAM_BYTES];
    let mut s = ContentState {
        rendered: Some(a),
        uploaded: Some([2; PARAM_BYTES]),
        pending: Some([2; PARAM_BYTES]),
    };
    s.aborted();
    assert!(!s.needs_render(&a));
    assert_eq!(
        dirty_ranges(s.uploaded.as_ref(), &a).collect::<Vec<_>>(),
        vec![0..PARAM_BYTES]
    );
}
#[test]
fn size_buckets_are_bounded_and_cover_request() {
    for v in 1..4097 {
        let b = bucket([v, v], 4096);
        assert!(b[0] >= v && b[0] <= 4096);
        assert_eq!(b[0] % 64, 0);
    }
}
#[test]
fn non_multiple_device_limit_does_not_overflow() {
    assert_eq!(bucket([65535, 1], 65535), [65535, 64]);
}
#[test]
fn sparse_dirty_ranges_are_coalesced_without_painting_everything() {
    let old = [0; PARAM_BYTES];
    let mut new = old;
    new[16] = 1;
    new[32] = 2;
    new[80] = 3;
    assert_eq!(
        dirty_ranges(Some(&old), &new).collect::<Vec<_>>(),
        vec![16..48, 80..96]
    );
}
/// This is the real WGSL parser/type/uniformity check, not a string assertion.
/// It runs without an adapter; native pixel tests are a separate opt-in binary.
#[test]
fn naga_validates_the_shader_and_the_uniform_abi() {
    let module = naga::front::wgsl::parse_str(super::WELD_SHADER)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::WELD_SHADER)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("WGSL type/uniformity validation");
    for name in ["vs_main", "fs_hybrid"] {
        assert!(module.entry_points.iter().any(|e| e.name == name));
    }
    let uniform = module
        .global_variables
        .iter()
        .map(|(_, v)| v)
        .find(|v| v.name.as_deref() == Some("u"))
        .expect("u");
    let naga::TypeInner::Struct { members, span } = &module.types[uniform.ty].inner else {
        panic!("Params must be a struct")
    };
    assert_eq!(*span, PARAM_BYTES as u32);
    assert_eq!(
        members.iter().map(|m| m.offset).collect::<Vec<_>>(),
        [0, 16, 32, 48]
    );
    let naga::TypeInner::Array { stride, .. } = &module.types[members[3].ty].inner else {
        panic!("sources must be an array")
    };
    assert_eq!(*stride, 96);
}

#[test]
fn naga_validates_crisp_boundary_uniform_stride() {
    let m = naga::front::wgsl::parse_str(super::WELD_SHADER).expect("WGSL parse");
    let b = m
        .global_variables
        .iter()
        .map(|(_, v)| v)
        .find(|v| v.name.as_deref() == Some("boundary"))
        .expect("boundary binding");
    assert_eq!(b.binding.as_ref().unwrap().binding, 1);
    let naga::TypeInner::Struct { members, span } = &m.types[b.ty].inner else {
        panic!("boundary struct")
    };
    assert_eq!(*span, mui_weld::boundary::BOUNDARY_BYTES as u32);
    let naga::TypeInner::Array { stride, .. } = &m.types[members[0].ty].inner else {
        panic!("edges")
    };
    assert_eq!(*stride, 48);
}

/// A path converts once per `Arc`, however many entries share it; a
/// failed conversion leaves nothing behind, and an unused one ages out.
#[test]
fn a_shared_path_is_converted_once() {
    use mui_geometry::Path;
    use std::sync::Arc;
    let mut c = super::Converted::default();
    let pill = Arc::new(Path::capsule(20., 60.).unwrap());
    let first = c.get(&pill).unwrap().clone();
    // Poison the kept conversion: the same `Arc` must come back as is.
    c.map.values_mut().next().unwrap().bez.truncate(0);
    assert!(c.get(&pill.clone()).unwrap().elements().is_empty());
    let wider = Arc::new(Path::capsule(20., 80.).unwrap());
    assert_ne!(c.get(&wider).unwrap(), &first);
    let mut bad = (*pill).clone();
    if let mui_geometry::PathCommand::ArcTo(arc) = &mut bad.commands[1] {
        arc.radius *= 2.;
    }
    assert!(c.get(&Arc::new(bad)).is_err());
    assert_eq!(c.map.len(), 2);
    for _ in 0..=2 * super::AGE {
        c.tick();
        c.get(&wider).unwrap();
    }
    assert_eq!(c.map.len(), 1, "the unused pill aged out");
}

/// A long path's fill is encoded once per colour and handed back while the
/// colour stays; placed by `append`, it is the encoding a fresh fill writes.
#[test]
fn a_replayed_fill_is_kept_per_colour_and_encodes_as_a_fill() {
    use mui_geometry::Path;
    use std::sync::Arc;
    use vello::peniko::{Color, Fill};
    let mut c = super::Converted::default();
    let pill = Arc::new(Path::capsule(20., 60.).unwrap());
    let red = Color::from_rgb8(255, 0, 0);
    let kept = c.fill(&pill, red).unwrap() as *const vello::Scene;
    assert_eq!(kept, c.fill(&pill, red).unwrap() as *const _);
    let at = crate::kurbo::Affine::translate((10.25, 3.5)) * crate::kurbo::Affine::scale(1.5);
    for color in [Color::from_rgb8(0, 0, 255), red] {
        let mut replayed = vello::Scene::new();
        replayed.append(c.fill(&pill, color).unwrap(), Some(at));
        let mut fresh = vello::Scene::new();
        fresh.fill(Fill::NonZero, at, color, None, c.get(&pill).unwrap());
        let (a, b) = (replayed.encoding(), fresh.encoding());
        assert!(a.path_tags == b.path_tags);
        assert_eq!(a.path_data, b.path_data);
        assert!(a.draw_tags == b.draw_tags);
        assert_eq!(a.draw_data, b.draw_data);
        assert_eq!(a.transforms, b.transforms);
        assert_eq!(a.styles, b.styles);
    }
}
