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
fn held_real_completion_survives_unrelated_roster_or_presentation_change_once() {
    use crate::sound::resource_revision::ResourceRevision;
    let mut observed_work = Vec::new();
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
        let original_epoch = first.epoch;
        let encoded_bytes = first.scan.encoded_bytes;
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
        // On the old behavior, retain the real redundant completion so RED
        // reports actual two-read/two-decode work, not merely an epoch number.
        let current = if worker.epoch.load(Ordering::Acquire) != original_epoch {
            let redundant = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
            assert_eq!(
                first.key, redundant.key,
                "wallpaper key/bytes/role did not change"
            );
            redundant
        } else {
            first
        };
        assert_eq!(worker.worker_identity(), worker_id);
        assert_eq!(
            ResourceRevision::capture(&path, 0, &resources),
            Some(authority)
        );
        observed_work.push((current.scan.lookups, current.scan.decodes));
        if observed_work.last() != Some(&(1, 1)) {
            // Observe both real RED cases before the aggregate assertion. No
            // successful case skips its publication/pixel assertions below.
            continue;
        }
        assert_eq!(current.scan.encoded_bytes, encoded_bytes);
        assert_eq!(current.epoch, original_epoch);
        assert_eq!(
            (worker.pending(), worker.bytes()),
            (1, 0),
            "the original required request remains pending until its real completion is published"
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
            (1, 1, 1),
            "one resource read/decode and one accepted prepared key"
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
    assert_eq!(
        observed_work,
        vec![(1, 1), (1, 1)],
        "unrelated roster and presentation changes must each preserve the required real pending crop without rereading/redecoding"
    );
}

#[test]
fn metadata_changes_with_obsolete_pending_geometry_keep_the_full_epoch_fence() {
    for changed in [
        "index", "path", "kind", "frame", "target", "window", "plane",
    ] {
        let (_directory, path, mut face) = fixture();
        if changed == "frame"
            && let ir::AssetKind::Image(meta) = &mut face.assets[0].kind
        {
            meta.frames = 2;
            meta.size.as_mut().unwrap().height = 2.;
        }
        let mut worker = Preparation::new_for_program(&path, 0);
        let (relay, intercepted) = mpsc::channel();
        let decoded = std::mem::replace(&mut worker.results, intercepted);
        let values = Values::default();
        worker.prepare(&face, ir::PageRef(0), ir::Presentation::Bitmap, 1., &values);
        let obsolete = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(obsolete.picture.is_some());
        // Keep metadata invalidation explicit: this does not authorize a new
        // wanted-only resize/page cancellation policy.
        face.assets.push(ir::Asset {
            path: "unused.png".into(),
            ..face.assets[0].clone()
        });
        let mut scale = 1.;
        match changed {
            "index" => {
                face.assets.swap(0, 1);
                face.pages[0].background.image = Some(ir::AssetRef(1));
            }
            "path" => face.assets[0].path = "Resources/pictures/CANVAS.png".into(),
            "kind" => {
                if let ir::AssetKind::Image(meta) = &mut face.assets[0].kind {
                    meta.margins.left = 1;
                }
            }
            "frame" => face.pages[0].background.frame = 1,
            "target" => scale = 0.5,
            "window" => face.pages[0].background.origin_y = 1,
            "plane" => {
                face.pages[0].background.image = None;
                let mut label = ir::Widget::new(
                    "$control",
                    ir::PageRef(0),
                    ir::Rect::new(0, 0, 2, 2),
                    ir::Kind::Label,
                );
                label
                    .images
                    .push(ir::ImageUse::new(ir::AssetRef(0), ir::Role::Background));
                face.widgets.push(label);
            }
            _ => unreachable!(),
        }
        worker.prepare(
            &face,
            ir::PageRef(0),
            ir::Presentation::Bitmap,
            scale,
            &values,
        );
        let current = decoded.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(
            current.picture.is_some(),
            "{changed}: valid authored resource request"
        );
        assert_ne!(
            obsolete.key, current.key,
            "{changed}: request is genuinely obsolete"
        );
        assert_ne!(
            obsolete.epoch, current.epoch,
            "{changed}: conservative epoch fence remains"
        );
        relay.send(obsolete).unwrap();
        assert!(
            worker
                .prepare(
                    &face,
                    ir::PageRef(0),
                    ir::Presentation::Bitmap,
                    scale,
                    &values
                )
                .is_empty()
        );
        assert_eq!(
            (worker.pending(), worker.bytes()),
            (1, 0),
            "{changed}: stale completion cannot publish or clear current pending work"
        );
        relay.send(current).unwrap();
        assert_eq!(
            worker
                .prepare(
                    &face,
                    ir::PageRef(0),
                    ir::Presentation::Bitmap,
                    scale,
                    &values
                )
                .len(),
            1
        );
        assert_eq!(worker.pending(), 0);
        assert_eq!(
            (
                worker.scan.lookups,
                worker.scan.decodes,
                worker.scan.preparation_completed
            ),
            (2, 2, 1),
            "{changed}: required new geometry decodes once; old result remains rejected"
        );
    }
}

#[test]
fn repeated_uses_with_aliased_asset_entries_dispatch_each_exact_key_once() {
    let (_directory, path, mut face) = fixture();
    face.pages[0].background.image = None;
    face.assets.push(face.assets[0].clone());
    // This is valid IR: different asset indices share a descriptor. Equal
    // requests for index0 must become adjacent even when index1 intervenes.
    for (n, asset) in [0, 1, 0].into_iter().enumerate() {
        let mut label = ir::Widget::new(
            format!("$art_{n}"),
            ir::PageRef(0),
            ir::Rect::new(0, 0, 4, 4),
            ir::Kind::Label,
        );
        label
            .images
            .push(ir::ImageUse::new(ir::AssetRef(asset), ir::Role::Background));
        face.widgets.push(label);
    }
    face.validate().unwrap();
    let mut worker = Preparation::new_for_program(&path, 0);
    let (capture, dispatched) = mpsc::sync_channel(1);
    // Inspect the actual production admission batch without decoding or adding
    // a worker hook. Only this test instance's sender is redirected.
    worker.jobs = capture;
    worker.prepare(
        &face,
        ir::PageRef(0),
        ir::Presentation::Bitmap,
        1.,
        &Values::default(),
    );
    let batch = dispatched.recv_timeout(Duration::from_secs(10)).unwrap();
    let unique: HashSet<_> = batch.iter().map(|request| request.key.clone()).collect();
    assert_eq!(
        (batch.len(), unique.len(), worker.pending()),
        (2, 2, 2),
        "three uses of two exact asset keys must not dispatch a duplicate hidden by pending's HashSet"
    );
    assert!(unique.iter().any(|key| key.asset == 0));
    assert!(unique.iter().any(|key| key.asset == 1));
}
