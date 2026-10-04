#![cfg(feature = "cpu")]
use mui_material::Capture;
use mui_scene::{ResolvedScene, prelude::*};
use mui_vello::{
    Cache, Cpu,
    vello_cpu::{Pixmap, RenderContext, Resources},
};

fn raster(scene: &ResolvedScene) -> Vec<[u8; 4]> {
    let mut ctx = RenderContext::new(120, 80);
    let mut resources = Resources::default();
    mui_vello::paint(
        &mut Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut Cache::default(),
        },
        scene,
        mui_vello::kurbo::Affine::IDENTITY,
    )
    .unwrap();
    ctx.flush();
    let mut pix = Pixmap::new(120, 80);
    ctx.render(&mut pix, &mut resources);
    pix.take_unpremultiplied()
        .iter()
        .map(|p| [p.r, p.g, p.b, p.a])
        .collect()
}

#[test]
fn extracted_transparent_layer_reassembles_and_keeps_rounded_clipping_and_text() {
    let child = col([
        block(70., 30.).fill(Role::Primary).id("unrelated"),
        text("MUI").text_size(12.).fill(Role::Ink),
    ])
    .size(60., 50.)
    .radius(12.)
    .clip()
    .id("panel");
    let tree = stack([child])
        .size(120., 80.)
        .pad(10.)
        .fill(Role::Background)
        .id("root");
    let mut spec = SceneSpec::new(tree);
    spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let scene = resolve(&spec).unwrap();
    let full = raster(&scene);
    let layer = raster(&scene.isolate(&["panel"]).unwrap());
    let background = raster(&scene.without(&["panel"]).unwrap());
    assert!(layer.iter().any(|p| p[3] == 0));
    assert!(layer.iter().any(|p| p[3] > 0));
    for ((want, over), under) in full.iter().zip(&layer).zip(&background) {
        let a = f64::from(over[3]) / 255.;
        for c in 0..3 {
            let assembled =
                (f64::from(over[c]) * a + f64::from(under[c]) * (1. - a)).round() as i16;
            assert!(
                (assembled - i16::from(want[c])).abs() <= 2,
                "lost clipping, text, or backdrop: {want:?} vs {over:?} over {under:?}"
            );
        }
    }
}

#[test]
fn ordered_fragments_preserve_late_floats_over_other_parts() {
    let tree = stack([
        stack([
            block(60., 40.).fill(Role::Primary),
            block(20., 20.)
                .fill(Role::Danger)
                .offset(8., 8.)
                .float()
                .id("socket"),
        ])
        .size(60., 40.)
        .id("panel"),
        block(90., 8.)
            .fill(Role::Success)
            .offset(0., 15.)
            .id("cable"),
    ])
    .size(120., 80.)
    .fill(Role::Background)
    .id("root");
    let scene = resolve(&SceneSpec::new(tree)).unwrap();
    let layers = scene.capture_layers(&["panel"]).unwrap();
    assert!(
        layers
            .iter()
            .filter(|p| p.part.as_deref() == Some("panel"))
            .count()
            > 1
    );
    let mut assembled = vec![[0.; 4]; 120 * 80];
    for layer in layers {
        for (dst, src) in assembled.iter_mut().zip(raster(&layer.scene)) {
            let a = f64::from(src[3]) / 255.;
            for c in 0..3 {
                dst[c] = f64::from(src[c]) * a + dst[c] * (1. - a);
            }
            dst[3] = a + dst[3] * (1. - a);
        }
    }
    for (want, got) in raster(&scene).iter().zip(assembled) {
        for c in 0..3 {
            assert!(
                (f64::from(want[c]) * f64::from(want[3]) / 255. - got[c]).abs() <= 3.,
                "{want:?} vs {got:?}"
            );
        }
    }
    assert!(scene.capture_layers(&["panel", "socket"]).is_err());
}
