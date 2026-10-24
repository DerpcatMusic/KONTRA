//! Physical source-zone mutation requirements; no library or source-name rules.
use crate::{builtins::Builtin, hir::*};
use std::collections::{BTreeMap, BTreeSet};

/// Script-only saved-row transactions do not replace prepared physical routes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoneRoutingCapability {
    Immutable,
    RequiresPreparedRoutes,
}

/// Complete destination domain needed before a callback may change zone routing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ZoneRoutingRequirements {
    pub writes: bool,
    pub destination_groups: Vec<u32>,
    pub dynamic_groups: bool,
    pub writes_keys: bool,
}

pub(crate) fn requirements(hir: &Hir) -> ZoneRoutingRequirements {
    struct Scan<'a> {
        hir: &'a Hir,
        out: ZoneRoutingRequirements,
        groups: BTreeSet<u32>,
    }
    impl Scan<'_> {
        fn integer(&self, arg: &Arg) -> Option<i32> {
            let variable = |id: VarId| match self.hir.vars.get(id.0 as usize)?.home {
                Home::Const(index) => match self.hir.consts.get(index as usize)? {
                    Const::Int(value) => Some(*value),
                    _ => None,
                },
                _ => None,
            };
            match arg {
                Arg::Expr(Expr {
                    kind: ExprKind::Int(value),
                    ..
                }) => Some(*value),
                Arg::Expr(Expr {
                    kind: ExprKind::Load(id),
                    ..
                })
                | Arg::Var(id, _) => variable(*id),
                _ => None,
            }
        }
        fn call(&mut self, builtin: Builtin, args: &[Arg]) {
            if builtin == Builtin::SetZonePar {
                self.out.writes = true;
                match args
                    .get(1)
                    .and_then(|a| self.integer(a))
                    .and_then(|v| crate::eval::symbol_name(self.hir, v))
                    .as_deref()
                {
                    Some("$ZONE_PAR_GROUP") => {
                        if let Some(group) = args
                            .get(2)
                            .and_then(|a| self.integer(a))
                            .and_then(|n| u32::try_from(n).ok())
                        {
                            self.groups.insert(group);
                        } else {
                            self.out.dynamic_groups = true;
                        }
                    }
                    Some("$ZONE_PAR_LOW_KEY" | "$ZONE_PAR_HIGH_KEY") => self.out.writes_keys = true,
                    // A dynamic selector can name either a group or a key write.
                    _ => {
                        self.out.dynamic_groups = true;
                        self.out.writes_keys = true;
                    }
                }
            }
            for arg in args {
                self.arg(arg);
            }
        }
        fn arg(&mut self, arg: &Arg) {
            match arg {
                Arg::Expr(expr) => self.expr(expr),
                Arg::Place(Place::Elem(_, expr)) => self.expr(expr),
                _ => {}
            }
        }
        fn expr(&mut self, expr: &Expr) {
            match &expr.kind {
                ExprKind::Builtin(b, args) => self.call(*b, args),
                ExprKind::Neg(e)
                | ExprKind::BitNot(e)
                | ExprKind::Not(e)
                | ExprKind::Cast(e)
                | ExprKind::LoadElem(_, e)
                | ExprKind::SysElem(_, e) => self.expr(e),
                ExprKind::Arith(_, a, b)
                | ExprKind::Compare(_, a, b)
                | ExprKind::Logic(_, a, b) => {
                    self.expr(a);
                    self.expr(b);
                }
                ExprKind::Concat(values) => {
                    for value in values {
                        self.expr(value);
                    }
                }
                _ => {}
            }
        }
        fn body(&mut self, body: &[Stmt]) {
            for stmt in body {
                match &stmt.kind {
                    StmtKind::Builtin(b, args) => self.call(*b, args),
                    StmtKind::Assign(place, value) => {
                        if let Place::Elem(_, index) = place {
                            self.expr(index);
                        }
                        self.expr(value);
                    }
                    StmtKind::Fill(_, values) => {
                        for value in values {
                            self.expr(value);
                        }
                    }
                    StmtKind::UiParams(_, values) => {
                        for (_, value) in values {
                            self.expr(value);
                        }
                    }
                    StmtKind::If(test, yes, no) => {
                        self.expr(test);
                        self.body(yes);
                        self.body(no);
                    }
                    StmtKind::While(test, body) => {
                        self.expr(test);
                        self.body(body);
                    }
                    StmtKind::Select(value, cases) => {
                        self.expr(value);
                        for case in cases {
                            self.body(&case.body);
                        }
                    }
                    // Every immutable function is scanned once below. This also
                    // conservatively covers future runtime dispatch/reachability.
                    StmtKind::Call(_) => {}
                }
            }
        }
    }
    let mut scan = Scan {
        hir,
        out: Default::default(),
        groups: Default::default(),
    };
    for callback in &hir.callbacks {
        scan.body(&callback.body);
    }
    for function in &hir.functions {
        scan.body(&function.body);
    }
    scan.out.destination_groups = scan.groups.into_iter().collect();
    scan.out
}

/// Off-audio initialization only. Runtime uses generation-owned prepared routes.
/// An override can replace one authoritative home but can never create an ID.
pub(crate) fn write_initial(
    original: &BTreeMap<u32, [i32; 3]>,
    overrides: &mut BTreeMap<u32, [i32; 3]>,
    groups: usize,
    zone: i32,
    parameter: sampler_core::ZoneParameter,
    value: i32,
) -> Result<(), &'static str> {
    let zone =
        u32::try_from(zone).map_err(|_| "zone identity must be a nonnegative source-defined ID")?;
    let original = original
        .get(&zone)
        .ok_or("zone identity is unavailable in this source")?;
    let mut next = overrides.get(&zone).copied().unwrap_or(*original);
    match parameter {
        sampler_core::ZoneParameter::Group => {
            if usize::try_from(value)
                .ok()
                .is_none_or(|group| group >= groups)
            {
                return Err("destination physical group is unavailable in this source");
            }
            next[0] = value;
        }
        sampler_core::ZoneParameter::LowKey | sampler_core::ZoneParameter::HighKey => {
            if !(0..=127).contains(&value) {
                return Err("zone key is outside 0..=127");
            }
            next[if parameter == sampler_core::ZoneParameter::LowKey {
                1
            } else {
                2
            }] = value;
            if next[1] > next[2] {
                return Err("zone key range is inverted");
            }
        }
    }
    overrides.insert(zone, next);
    Ok(())
}

/// Finite control-thread counterpart of runtime zone batches. It keeps the
/// shared async identity domain and committed readback separate from pending writes.
#[derive(Clone, Default)]
pub(crate) struct InitialZoneBatch {
    homes: BTreeMap<u32, [i32; 3]>,
    jobs: Vec<(i32, u32)>,
}
impl InitialZoneBatch {
    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }
    pub fn queue(
        &mut self,
        original: &BTreeMap<u32, [i32; 3]>,
        committed: &BTreeMap<u32, [i32; 3]>,
        groups: usize,
        midi: &mut sampler_core::MidiObject,
        zone: i32,
        parameter: sampler_core::ZoneParameter,
        value: i32,
    ) -> Result<i32, &'static str> {
        let capacity = sampler_core::source_zone_job_capacity(original.len())
            .map_err(|_| "source zone batch budget exceeds the absolute job limit")?;
        if self.jobs.len() >= capacity {
            return Err("initial zone batch capacity exceeded");
        }
        // Validation observes this owner's pending earlier writes, but ordinary
        // get_zone_par still observes committed homes until the boundary.
        let id = u32::try_from(zone).map_err(|_| "invalid physical source zone")?;
        let current = self
            .homes
            .get(&id)
            .or_else(|| committed.get(&id))
            .or_else(|| original.get(&id))
            .ok_or("physical source zone unavailable")?;
        if !original.contains_key(&id) {
            return Err("physical source zone unavailable");
        }
        let mut next = *current;
        match parameter {
            sampler_core::ZoneParameter::Group => {
                if usize::try_from(value).ok().is_none_or(|g| g >= groups) {
                    return Err("destination physical group unavailable");
                }
                next[0] = value;
            }
            sampler_core::ZoneParameter::LowKey | sampler_core::ZoneParameter::HighKey => {
                if !(0..=127).contains(&value) {
                    return Err("zone key is outside 0..=127");
                }
                next[if parameter == sampler_core::ZoneParameter::LowKey {
                    1
                } else {
                    2
                }] = value;
            }
        }
        // Control thread only. Preallocate the same complete source-sized job
        // budget as runtime lowering before publishing the first positive ID.
        if self.jobs.capacity() < capacity {
            self.jobs
                .try_reserve_exact(capacity - self.jobs.len())
                .map_err(|_| "initial zone job allocation failed")?;
        }
        let async_id = midi
            .allocate_async_id()
            .map_err(|_| "initial zone async identity exhausted")?;
        self.homes.insert(id, next);
        self.jobs.push((async_id, id));
        Ok(async_id)
    }
    pub fn commit(&mut self, committed: &mut BTreeMap<u32, [i32; 3]>) -> Vec<(i32, bool)> {
        // Match the live consumer: coalesce a complete home per zone before
        // publishing it. Every job for an invalid final home fails together.
        let mut valid = BTreeSet::new();
        for (zone, home) in std::mem::take(&mut self.homes) {
            if home[1] <= home[2] {
                valid.insert(zone);
                committed.insert(zone, home);
            }
        }
        self.jobs
            .drain(..)
            .map(|(id, zone)| (id, valid.contains(&zone)))
            .collect()
    }
    pub fn abort(&mut self) -> Vec<(i32, bool)> {
        self.homes.clear();
        self.jobs.drain(..).map(|(id, _)| (id, false)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn analyze(source: &str) -> Hir {
        let mut symbols = crate::lexer::Interner::default();
        let mut tokens = crate::lexer::lex(source, &mut symbols).unwrap();
        crate::lexer::preprocess(&mut tokens, &symbols, &Default::default()).unwrap();
        let ast = crate::parser::parse(&tokens, &symbols).unwrap();
        crate::sema::analyze(
            ast,
            &symbols,
            crate::sema::Budget {
                variables: crate::Limits::LIBRARY.variables,
                array_cells: crate::Limits::LIBRARY.array_cells,
            },
            &[],
        )
        .unwrap()
    }
    #[test]
    fn constant_source_destinations_include_expression_calls_and_function_bodies() {
        let hir = analyze(
            "on init declare $job declare $id := 73 end on
            function route
                $job := set_zone_par($id,$ZONE_PAR_GROUP,0)
                set_zone_par($id,$ZONE_PAR_LOW_KEY,0)
                set_zone_par($id,$ZONE_PAR_HIGH_KEY,127)
            end function
            on persistence_changed
                set_zone_par(91,$ZONE_PAR_GROUP,1)
                call route
            end on",
        );
        assert_eq!(
            requirements(&hir),
            ZoneRoutingRequirements {
                writes: true,
                destination_groups: vec![0, 1],
                dynamic_groups: false,
                writes_keys: true,
            }
        );
    }
    #[test]
    fn variable_group_or_selector_requires_complete_physical_group_domain() {
        for source in [
            "on init declare $id := 73 declare $group := 1 end on on note set_zone_par($id,$ZONE_PAR_GROUP,$group) end on",
            "on init declare $parameter := $ZONE_PAR_LOW_KEY end on on note set_zone_par(73,$parameter,1) end on",
        ] {
            let need = requirements(&analyze(source));
            assert!(need.writes && need.dynamic_groups);
        }
    }
    #[test]
    fn getter_only_needs_no_mutable_route_bank() {
        assert_eq!(
            requirements(&analyze(
                "on init declare $g := get_zone_par(73,$ZONE_PAR_GROUP) end on"
            )),
            ZoneRoutingRequirements::default()
        );
    }
    #[test]
    fn init_write_replaces_only_an_existing_physical_home() {
        let original = [(73, [1, 36, 80]), (91, [0, 0, 127])].into();
        let mut writes = BTreeMap::new();
        for (parameter, value) in [
            (sampler_core::ZoneParameter::Group, 0),
            (sampler_core::ZoneParameter::LowKey, 0),
            (sampler_core::ZoneParameter::HighKey, 127),
        ] {
            write_initial(&original, &mut writes, 2, 73, parameter, value).unwrap();
        }
        assert_eq!(writes, [(73, [0, 0, 127])].into());
        assert_eq!(original[&73], [1, 36, 80]);
        assert_eq!(original[&91], [0, 0, 127]);
    }
    #[test]
    fn invalid_zone_group_or_range_never_changes_a_prior_home() {
        let original = [(73, [1, 36, 80])].into();
        let mut writes = BTreeMap::new();
        write_initial(
            &original,
            &mut writes,
            2,
            73,
            sampler_core::ZoneParameter::Group,
            0,
        )
        .unwrap();
        let before = writes.clone();
        for (zone, parameter, value) in [
            (0, sampler_core::ZoneParameter::Group, 0),
            (72, sampler_core::ZoneParameter::Group, 0),
            (73, sampler_core::ZoneParameter::Group, 2),
            (73, sampler_core::ZoneParameter::Group, -1),
            (73, sampler_core::ZoneParameter::LowKey, -1),
            (73, sampler_core::ZoneParameter::HighKey, 128),
            (73, sampler_core::ZoneParameter::LowKey, 81),
            (73, sampler_core::ZoneParameter::HighKey, 35),
        ] {
            assert!(write_initial(&original, &mut writes, 2, zone, parameter, value).is_err());
            assert_eq!(writes, before);
        }
    }
    #[test]
    fn fabricated_override_cannot_create_authority_for_an_absent_source_id() {
        let original = [(73, [1, 36, 80])].into();
        let mut writes: BTreeMap<u32, [i32; 3]> = [(72, [0, 0, 127])].into();
        let before = writes.clone();
        assert!(
            write_initial(
                &original,
                &mut writes,
                2,
                72,
                sampler_core::ZoneParameter::Group,
                1
            )
            .is_err()
        );
        assert_eq!(writes, before);
    }
    #[test]
    fn initial_async_batch_keeps_readback_old_until_boundary_and_completes_once() {
        let original = [(73, [1, 36, 80])].into();
        let mut committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        let mut ids = Vec::new();
        for (parameter, value) in [
            (sampler_core::ZoneParameter::Group, 0),
            (sampler_core::ZoneParameter::LowKey, 0),
            (sampler_core::ZoneParameter::HighKey, 127),
        ] {
            ids.push(
                batch
                    .queue(&original, &committed, 2, &mut midi, 73, parameter, value)
                    .unwrap(),
            );
        }
        assert!(committed.is_empty());
        assert_eq!(original[&73], [1, 36, 80]);
        assert_eq!(ids, vec![1, 2, 3]);
        assert_eq!(
            batch.commit(&mut committed),
            ids.into_iter().map(|id| (id, true)).collect::<Vec<_>>()
        );
        assert_eq!(committed[&73], [0, 0, 127]);
        assert!(batch.commit(&mut committed).is_empty());
    }
    #[test]
    fn initial_rejected_batch_admission_preserves_async_identity_and_prior_jobs() {
        let original = [(73, [1, 36, 80])].into();
        let committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                73,
                sampler_core::ZoneParameter::Group,
                0,
            )
            .unwrap();
        let high = midi.async_id_high_water();
        assert!(
            batch
                .queue(
                    &original,
                    &committed,
                    2,
                    &mut midi,
                    72,
                    sampler_core::ZoneParameter::Group,
                    1
                )
                .is_err()
        );
        assert_eq!(midi.async_id_high_water(), high);
        assert_eq!(batch.jobs, [(1, 73)]);
        assert_eq!(batch.homes, [(73, [0, 36, 80])].into());
    }
    #[test]
    fn initial_batch_capacity_does_not_lose_or_reuse_any_returned_async_id() {
        let original: BTreeMap<u32, [i32; 3]> = [(73, [1, 36, 80])].into();
        let mut committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        let capacity = sampler_core::source_zone_job_capacity(original.len()).unwrap();
        for _ in 0..capacity {
            batch
                .queue(
                    &original,
                    &committed,
                    2,
                    &mut midi,
                    73,
                    sampler_core::ZoneParameter::Group,
                    0,
                )
                .unwrap();
        }
        let high = midi.async_id_high_water();
        assert!(
            batch
                .queue(
                    &original,
                    &committed,
                    2,
                    &mut midi,
                    73,
                    sampler_core::ZoneParameter::Group,
                    1
                )
                .is_err()
        );
        assert_eq!(midi.async_id_high_water(), high);
        assert_eq!(
            batch.commit(&mut committed),
            (1..=capacity as i32)
                .map(|id| (id, true))
                .collect::<Vec<_>>()
        );
        assert_eq!(committed[&73], [0, 36, 80]);
    }
    #[test]
    fn pending_transient_key_order_commits_only_the_valid_final_range() {
        let original = [(73, [1, 36, 80])].into();
        let mut committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                73,
                sampler_core::ZoneParameter::LowKey,
                100,
            )
            .unwrap();
        assert!(committed.is_empty());
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                73,
                sampler_core::ZoneParameter::HighKey,
                110,
            )
            .unwrap();
        assert_eq!(batch.commit(&mut committed), [(1, true), (2, true)]);
        assert_eq!(committed[&73], [1, 100, 110]);
    }
    #[test]
    fn invalid_final_zone_preserves_its_home_without_rolling_back_an_independent_valid_zone() {
        let original = [(73, [1, 36, 80]), (91, [0, 0, 127])].into();
        let mut committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                91,
                sampler_core::ZoneParameter::Group,
                1,
            )
            .unwrap();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                73,
                sampler_core::ZoneParameter::LowKey,
                100,
            )
            .unwrap();
        assert_eq!(batch.commit(&mut committed), [(1, true), (2, false)]);
        assert_eq!(committed, [(91, [1, 0, 127])].into());
        assert_eq!(original[&73], [1, 36, 80]);
        assert_eq!(original[&91], [0, 0, 127]);
        assert_eq!(midi.async_id_high_water(), 2);
        assert!(batch.is_empty());
        assert!(batch.commit(&mut committed).is_empty());
    }
    #[test]
    fn source_sized_batch_plus_one_keeps_ids_and_per_zone_final_home_atomicity() {
        use sampler_core::ZoneParameter::{Group, HighKey, LowKey};
        let count = 100u32;
        let original: BTreeMap<_, _> = (0..count).map(|i| (i * 11, [0, 40, 80])).collect();
        let mut committed = BTreeMap::new();
        let capacity = sampler_core::source_zone_job_capacity(original.len()).unwrap();
        assert_eq!(capacity, 300);
        let mut midi = sampler_core::MidiObject::default();
        assert_eq!(midi.allocate_async_id().unwrap(), 1); // shared non-zone async domain
        let mut batch = InitialZoneBatch::default();
        let mut expected = Vec::new();
        for index in 0..count {
            let valid = index != count - 1;
            for (parameter, value) in [
                (Group, 1),
                (LowKey, 100),
                (HighKey, if valid { 110 } else { 60 }),
            ] {
                let id = batch
                    .queue(
                        &original,
                        &committed,
                        2,
                        &mut midi,
                        (index * 11) as i32,
                        parameter,
                        value,
                    )
                    .unwrap();
                expected.push((id, valid));
            }
        }
        assert_eq!(
            expected.iter().map(|r| r.0).collect::<Vec<_>>(),
            (2..=301).collect::<Vec<_>>()
        );
        assert!(
            batch
                .queue(&original, &committed, 2, &mut midi, 0, Group, 0)
                .is_err()
        );
        assert_eq!(midi.async_id_high_water(), 301);
        assert!(committed.is_empty());
        assert_eq!(batch.commit(&mut committed), expected);
        assert_eq!(committed.len(), 99);
        assert_eq!(committed.get(&0), Some(&[1, 100, 110]));
        assert!(!committed.contains_key(&((count - 1) * 11)));
        assert!(batch.commit(&mut committed).is_empty());
        assert_eq!(midi.allocate_async_id().unwrap(), 302);
        for id in 303..=602 {
            assert_eq!(
                batch
                    .queue(&original, &committed, 2, &mut midi, 0, Group, 0)
                    .unwrap(),
                id
            );
        }
        let before = committed.clone();
        assert_eq!(
            batch.abort(),
            (303..=602).map(|id| (id, false)).collect::<Vec<_>>()
        );
        assert_eq!(committed, before);
        assert!(batch.abort().is_empty());
        assert_eq!(midi.allocate_async_id().unwrap(), 603);
    }
}

#[cfg(test)]
mod zero_identity_tests {
    use super::*;
    #[test]
    fn explicit_zero_is_a_real_home_and_missing_id_has_no_sentinel_alias() {
        let original = [(0, [1, 36, 80]), (7, [0, 0, 127]), (2018, [1, 60, 60])].into();
        let mut committed = BTreeMap::new();
        write_initial(
            &original,
            &mut committed,
            2,
            0,
            sampler_core::ZoneParameter::Group,
            1,
        )
        .unwrap();
        assert_eq!(committed[&0], [1, 36, 80]);
        let before = committed.clone();
        assert!(
            write_initial(
                &original,
                &mut committed,
                2,
                -1,
                sampler_core::ZoneParameter::Group,
                0
            )
            .is_err()
        );
        assert!(
            write_initial(
                &original,
                &mut committed,
                2,
                1,
                sampler_core::ZoneParameter::Group,
                0
            )
            .is_err()
        );
        assert_eq!(committed, before);
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        let id = batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                0,
                sampler_core::ZoneParameter::Group,
                0,
            )
            .unwrap();
        assert!(id > 0);
        assert_eq!(committed[&0], [1, 36, 80]);
        assert_eq!(batch.commit(&mut committed), vec![(id, true)]);
        assert_eq!(committed[&0], [0, 36, 80]);
    }
}

#[cfg(test)]
mod failure_boundary_tests {
    use super::*;
    #[test]
    fn abort_retains_each_id_once_without_applying_any_pending_home() {
        let original = [(0, [1, 36, 80]), (7, [1, 36, 80])].into();
        let committed = BTreeMap::new();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        for zone in [0, 7] {
            batch
                .queue(
                    &original,
                    &committed,
                    2,
                    &mut midi,
                    zone,
                    sampler_core::ZoneParameter::Group,
                    0,
                )
                .unwrap();
        }
        assert_eq!(batch.abort(), vec![(1, false), (2, false)]);
        assert!(batch.is_empty());
        assert!(batch.commit(&mut BTreeMap::new()).is_empty());
        assert!(committed.is_empty());
        assert_eq!(midi.async_id_high_water(), 2);
    }
    #[test]
    fn invalid_final_home_fails_only_its_own_jobs_and_never_changes_that_zone() {
        let original: BTreeMap<u32, [i32; 3]> = [(0, [1, 36, 80]), (7, [1, 36, 80])].into();
        let mut committed = original.clone();
        let mut midi = sampler_core::MidiObject::default();
        let mut batch = InitialZoneBatch::default();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                0,
                sampler_core::ZoneParameter::LowKey,
                100,
            )
            .unwrap();
        batch
            .queue(
                &original,
                &committed,
                2,
                &mut midi,
                7,
                sampler_core::ZoneParameter::Group,
                0,
            )
            .unwrap();
        assert_eq!(batch.commit(&mut committed), vec![(1, false), (2, true)]);
        assert_eq!(committed[&0], original[&0]);
        assert_eq!(committed[&7], [0, 36, 80]);
    }
}
