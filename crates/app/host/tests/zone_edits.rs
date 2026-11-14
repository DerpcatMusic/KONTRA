//! `set_zone_par` outside the engine's construction: it queues and lands
//! through the plan swap; a recall's lands at once, as a load's does.
#![cfg(test)]

use std::sync::Arc;

use kontra_engine::{Engine, Plan, Saved, ZoneEdit, ZoneFacts};
use kontra_stream::{Store, StoreConfig};

/// An engine running `source` over one zone (id 0, group 0) in two groups;
/// the cell and the UI id of `$g`.
fn engine(source: &str) -> (Engine, u32, i32) {
    let script = kontra_ksp::compile(source, &[]).unwrap();
    let cell = script
        .variables
        .iter()
        .find(|v| &*v.name == "$g")
        .and_then(|v| match v.cells {
            kontra_ksp::Cells::Int { base, .. } => Some(base),
            _ => None,
        })
        .unwrap();
    let var = script.variables.iter().position(|v| &*v.name == "$g");
    let id = kontra_ksp::FIRST_UI_ID + i32::try_from(var.unwrap()).unwrap();
    let mut plan = Plan::new(vec![], 16);
    plan.groups = vec![
        kontra_engine::Group::standard(),
        kontra_engine::Group::standard(),
    ];
    plan.zone_facts = vec![ZoneFacts {
        id: 0,
        name: "z".into(),
        group: 0,
        keys: 0..=127,
        velocities: 1..=127,
        root: 60,
        start: 0,
        end: 0,
        start_range: 0,
        rate: 48_000,
        analysis: None,
    }];
    plan.scripts = vec![kontra_host::script::plan(&script, &[], &[])];
    let store = Store::new(StoreConfig::DEFAULT).unwrap();
    (
        Engine::new(Arc::new(plan), store.client(), 48_000),
        cell,
        id,
    )
}

/// The queued edits, applied to a copy of the plan and swapped in, as the
/// host does.
fn swap(e: &mut Engine, edits: &[ZoneEdit]) {
    let mut plan = Plan::clone(e.plan());
    let done: Vec<_> = (edits.iter())
        .map(|x| (*x, plan.edit_zone(x.zone, x.par, x.value)))
        .collect();
    e.swap_plan(&mut Arc::new(plan), &done);
}

fn drained(e: &mut Engine) -> Vec<ZoneEdit> {
    let mut out = Vec::new();
    e.drain_zone_edits(|x| {
        out.push(x);
        true
    });
    out
}

fn group(e: &Engine) -> u32 {
    e.plan().zone_facts[0].group
}

/// A recall that keeps the scripts runs `persistence_changed` as a load
/// runs it, off the audio thread (KSP manual: loading a snapshot stops the
/// audio engine): its `set_zone_par` lands at once, nothing queues.
#[test]
fn a_recalls_zone_edit_lands_at_once() {
    let (mut e, cell, _) = engine(
        "on init
            declare $g
            declare $id
            make_persistent($g)
        end on
        on persistence_changed
            $id := set_zone_par(0, $ZONE_PAR_GROUP, $g)
        end on",
    );
    let saved: Box<[Saved]> = vec![Saved {
        ints: vec![(cell, 1)],
        texts: vec![],
    }]
    .into();
    e.recall(saved);
    assert_eq!(group(&e), 1);
    assert!(drained(&mut e).is_empty());
}

/// 0 → 1, then 1 → 0 a block later while the first awaits its swap: both
/// queue, and the zone ends in group 0 (Codex r4547342729).
#[test]
fn an_edit_undoing_one_awaiting_its_swap_still_queues() {
    let (mut e, _, id) = engine(
        "on init
            declare ui_knob $g(0, 1, 1)
            declare $id
        end on
        on ui_control($g)
            $id := set_zone_par(0, $ZONE_PAR_GROUP, $g)
        end on",
    );
    e.set_control(0, id, 1);
    let first = drained(&mut e);
    e.set_control(0, id, 0);
    let second = drained(&mut e);
    assert_eq!(second.len(), 1, "the undoing edit was dropped");
    swap(&mut e, &[first, second].concat());
    assert_eq!(group(&e), 0);
}
