//! Characterize real decode completion across same-authority UI invalidation.
//! The private result channel is intercepted only on this test's worker instance.
use super::super::{ir_view::Assets, theme};
use super::*;
use moose::mui::mui::prelude::*;
use std::time::{Duration, Instant};

#[allow(dead_code)]
#[path = "../../../crates/sampler-kontakt/tests/support/snapshot_reuse.rs"]
mod instrument_fixture;

fn fixture() -> (tempfile::TempDir, PathBuf, ir::Interface) {
    let directory = instrument_fixture::directory();
    let path = directory.path().join("authored.nki");
    std::fs::write(
        &path,
        instrument_fixture::base_instrument("In-flight art", &[None; 5]),
    )
    .unwrap();
    let pictures = directory.path().join("Resources/pictures");
    std::fs::create_dir_all(&pictures).unwrap();
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, 4, 4);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let pixels: Vec<_> = (0..4u8)
            .flat_map(|y| (0..4u8).flat_map(move |x| [40 + x * 20, 50 + y * 20, 180, 255]))
            .collect();
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&pixels)
            .unwrap();
    }
    std::fs::write(pictures.join("canvas.png"), bytes).unwrap();
    let face = ir::Interface {
        source: ir::Source::Ksp { slot: 0 },
        performance: true,
        pages: vec![ir::Page {
            size: ir::Size {
                width: 2.,
                height: 2.,
            },
            background: ir::Background {
                image: Some(ir::AssetRef(0)),
                ..Default::default()
            },
            ..Default::default()
        }],
        assets: vec![ir::Asset {
            path: "Resources/pictures/canvas.png".into(),
            kind: ir::AssetKind::Image(ir::ImageMeta {
                size: Some(ir::Size {
                    width: 4.,
                    height: 4.,
                }),
                ..Default::default()
            }),
        }],
        ..Default::default()
    };
    (directory, path, face)
}

fn pixels(face: &ir::Interface, picture: Arc<Picture>, presentation: ir::Presentation) -> Vec<u8> {
    let mut assets = Assets::default();
    assets.sync(face, presentation, |_| Some(picture.clone()));
    let mut ui = theme::ui();
    for _ in 0..2 {
        let root = super::super::ir_view::view(
            &mut ui,
            face,
            ir::PageRef(0),
            &assets,
            presentation,
            1.,
            &mut Values::default(),
        );
        ui.frame(root, Some(Size::new(2., 2.)), Input::default(), 1. / 60.)
            .unwrap();
    }
    super::super::tests::pixels(&ui, 2, 2)
}

fn settle(worker: &mut Preparation, face: &ir::Interface) {
    let started = Instant::now();
    loop {
        worker.prepare(
            &face,
            ir::PageRef(0),
            ir::Presentation::Bitmap,
            1.,
            &Values::default(),
        );
        if worker.pending() == 0 {
            break;
        }
        assert!(started.elapsed() < Duration::from_secs(10));
        thread::sleep(Duration::from_millis(1));
    }
    assert!(worker.failures().is_empty());
}

#[test]
fn settled_real_cache_hit_marks_unchanged_and_emits_no_entries() {
    let (_directory, path, face) = fixture();
    let mut worker = Preparation::new_for_program(&path, 0);
    settle(&mut worker, &face);
    assert_eq!(
        (
            worker.scan.lookups,
            worker.scan.decodes,
            worker.scan.preparation_completed
        ),
        (1, 1, 1)
    );
    let bytes = worker.bytes();
    let before = (
        worker.scan.lookups,
        worker.scan.decodes,
        worker.scan.preparation_completed,
    );
    let mut unchanged_calls = 0;
    let mut reemitted_entries = 0;
    for _ in 0..4 {
        let emitted = worker.prepare(
            &face,
            ir::PageRef(0),
            ir::Presentation::Bitmap,
            1.,
            &Values::default(),
        );
        unchanged_calls += usize::from(worker.unchanged());
        reemitted_entries += emitted.len();
    }
    assert_eq!(
        (unchanged_calls, reemitted_entries),
        (4, 0),
        "four settled identical requests must skip Assets map clearing/repopulation and emit no duplicate prepared entries"
    );
    assert_eq!(worker.bytes(), bytes);
    assert_eq!(
        (
            worker.scan.lookups,
            worker.scan.decodes,
            worker.scan.preparation_completed
        ),
        before
    );
}

#[test]
fn empty_owner_metadata_wanted_and_real_completion_transitions_publish_once() {
    let (_directory, path, mut face) = fixture();
    let mut worker = Preparation::new_for_program(&path, 0);
    let values = Values::default();
    let empty = ir::Interface::default();
    assert!(
        worker
            .prepare(
                &empty,
                ir::PageRef(0),
                ir::Presentation::Bitmap,
                1.,
                &values
            )
            .is_empty()
    );
    assert!(
        !worker.unchanged(),
        "first empty owner still publishes its initial state"
    );
    assert!(
        worker
            .prepare(
                &empty,
                ir::PageRef(0),
                ir::Presentation::Bitmap,
                1.,
                &values
            )
            .is_empty()
    );
    assert!(worker.unchanged());

    let (relay, intercepted) = mpsc::channel();
    let decoded = std::mem::replace(&mut worker.results, intercepted);
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(
        !worker.unchanged(),
        "new metadata/wanted key clears the previous live state"
    );
    let first = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(
        worker.unchanged(),
        "same in-flight request without a completion is unchanged"
    );
    relay.send(first).unwrap();
    assert_eq!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .len(),
        1
    );
    assert!(
        !worker.unchanged(),
        "a result arriving after an unchanged call must publish"
    );
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(worker.unchanged());

    // Request-affecting metadata changes still invalidate once, even when the
    // encoded path and bytes stay the same. No epoch/cancel policy is changed.
    if let ir::AssetKind::Image(meta) = &mut face.assets[0].kind {
        meta.margins.left = 1;
    }
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(!worker.unchanged());
    relay
        .send(decoded.recv_timeout(Duration::from_secs(10)).unwrap())
        .unwrap();
    assert_eq!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .len(),
        1
    );
    assert!(!worker.unchanged());
    let before = (
        worker.scan.lookups,
        worker.scan.decodes,
        worker.scan.preparation_completed,
    );
    assert_eq!(before, (2, 2, 2));
    face.pages[0].background.image = None;
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(
        !worker.unchanged(),
        "removing the wanted set must clear the old live pixels"
    );
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(worker.unchanged());
    face.pages[0].background.image = Some(ir::AssetRef(0));
    assert_eq!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .len(),
        1
    );
    assert!(
        !worker.unchanged(),
        "returning to a resident key must republish it"
    );
    assert!(
        worker
            .prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values)
            .is_empty()
    );
    assert!(worker.unchanged());
    assert_eq!(
        (
            worker.scan.lookups,
            worker.scan.decodes,
            worker.scan.preparation_completed
        ),
        before
    );
}

#[test]
fn held_real_completion_characterizes_same_key_requeue_without_admitting_old_epoch() {
    use crate::sound::resource_revision::ResourceRevision;
    for change_presentation in [false, true] {
        let (_directory, path, mut face) = fixture();
        let resources = sampler_kontakt::Resources::of(&path);
        let authority = ResourceRevision::capture(&path, 0, &resources).unwrap();
        let mut worker = Preparation::new_for_program(&path, 0);
        let worker_id = worker.worker_identity();
        // The real decoder still sends into its original bounded queue. Hold
        // those Results here; only this test instance consumes the relay below.
        let (relay, intercepted) = mpsc::channel();
        let decoded = std::mem::replace(&mut worker.results, intercepted);
        let values = Values::default();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        let first = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(first.picture.is_some());
        assert_eq!((first.scan.lookups, first.scan.decodes), (1, 1));
        assert_eq!((worker.pending(), worker.bytes()), (1, 0));
        let presentation = if change_presentation {
            ir::Presentation::Vector
        } else {
            // No filesystem/resource change; the extra descriptor is unrequested.
            face.assets.push(ir::Asset {
                path: "unused.png".into(),
                ..face.assets[0].clone()
            });
            ir::Presentation::Bitmap
        };
        worker.prepare(&face, ir::PageRef(0), presentation, 1., &values);
        let current = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(
            first.key, current.key,
            "wallpaper key/bytes/role did not change"
        );
        assert_ne!(first.epoch, current.epoch);
        assert_eq!(worker.worker_identity(), worker_id);
        assert_eq!(
            ResourceRevision::capture(&path, 0, &resources),
            Some(authority)
        );
        assert_eq!(
            (current.scan.lookups, current.scan.decodes),
            (2, 2),
            "current full-list/presentation invalidation reissues this in-flight non-strip crop, including its resource read/decode"
        );
        assert_eq!(current.scan.encoded_bytes, first.scan.encoded_bytes * 2);

        relay.send(first).unwrap();
        assert!(
            worker
                .prepare(&face, ir::PageRef(0), presentation, 1., &values)
                .is_empty()
        );
        assert_eq!(
            (worker.pending(), worker.bytes()),
            (1, 0),
            "old pixels cannot publish or clear the current pending key"
        );
        let current_picture = current.picture.as_ref().unwrap().clone();
        relay.send(current).unwrap();
        let published = worker.prepare(&face, ir::PageRef(0), presentation, 1., &values);
        assert_eq!(published.len(), 1);
        assert!(Arc::ptr_eq(
            published[0].1.as_ref().unwrap(),
            &current_picture
        ));
        assert_eq!(worker.pending(), 0);
        assert_eq!(
            (
                worker.scan.lookups,
                worker.scan.decodes,
                worker.scan.preparation_completed
            ),
            (2, 2, 1),
            "Source work includes the discarded decode; published prepared-key count does not"
        );
        let settled = (
            worker.scan.lookups,
            worker.scan.decodes,
            worker.scan.preparation_completed,
        );
        for _ in 0..3 {
            worker.prepare(&face, ir::PageRef(0), presentation, 1., &values);
        }
        assert_eq!(
            (
                worker.scan.lookups,
                worker.scan.decodes,
                worker.scan.preparation_completed
            ),
            settled
        );

        let fresh = Source::for_program(&path, 0)
            .unwrap()
            .load(&face.assets[0])
            .unwrap();
        assert_eq!(
            pixels(&face, current_picture, presentation),
            pixels(&face, fresh, presentation),
            "settled production painter matches a fresh full-canvas read"
        );
    }
}
