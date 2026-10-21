//! Physical effect ownership selects the file table; indices never cross it.
use crate::{SnapshotState, effects::ImpulseRequest};
use ni_file::kontakt::objects::Program;
use std::collections::{BTreeSet, HashMap};

#[derive(Default)]
pub(super) struct EffectFileOrigins {
    snapshot_generic: BTreeSet<i32>,
    snapshot_groups: usize,
}

impl EffectFileOrigins {
    pub(super) fn overlay(program: &mut Program, snapshot: Option<&SnapshotState>) -> Self {
        let Some(snapshot) = snapshot else {
            return Self::default();
        };
        let mut origins = Self {
            snapshot_groups: snapshot.groups.len(),
            ..Self::default()
        };
        // Replacement follows each native chunk kind's physical occurrence.
        // A shorter/script-only snapshot leaves every unmatched base record owned
        // by the base table, including its convolution indices.
        for kind in [crate::effects::RACK, crate::effects::BUS] {
            let mut saved = snapshot.effects.iter().filter(|(id, _)| *id == kind);
            for (index, child) in program
                .0
                .children
                .iter_mut()
                .filter(|c| c.id == kind)
                .enumerate()
            {
                let Some((_, bytes)) = saved.next() else {
                    break;
                };
                child.data = bytes.clone();
                let generic = if kind == crate::effects::RACK {
                    [1, 0, 2].get(index).copied()
                } else {
                    i32::try_from(index).ok().and_then(|i| i.checked_add(1000))
                };
                if let Some(generic) = generic {
                    origins.snapshot_generic.insert(generic);
                }
            }
        }
        origins
    }

    pub(super) fn name<'a>(
        &self,
        request: ImpulseRequest,
        base: &'a HashMap<u32, String>,
        snapshot: Option<&'a SnapshotState>,
    ) -> Result<&'a str, String> {
        let snapshot_owned = if request.address.group >= 0 {
            usize::try_from(request.address.group).is_ok_and(|i| i < self.snapshot_groups)
        } else {
            self.snapshot_generic.contains(&request.address.generic)
        };
        let (origin, files) = if snapshot_owned {
            (
                "snapshot",
                &snapshot
                    .ok_or("missing snapshot resource authority")?
                    .other_files,
            )
        } else {
            ("instrument", base)
        };
        let name = u32::try_from(request.index)
            .ok()
            .and_then(|i| files.get(&i))
            .ok_or_else(|| {
                format!(
                    "index {} is not in the {origin} other-files table",
                    request.index
                )
            })?;
        if name.is_empty() {
            return Err(format!(
                "index {} has an empty {origin} file reference",
                request.index
            ));
        }
        Ok(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ni_file::kontakt::{Chunk, StructuredObject};

    fn snapshot(effects: Vec<(u16, Vec<u8>)>, files: &[(u32, &str)]) -> SnapshotState {
        SnapshotState {
            instrument: String::new(),
            persistent: vec![],
            effects,
            groups: vec![],
            other_files: files
                .iter()
                .map(|(id, name)| (*id, (*name).into()))
                .collect(),
        }
    }
    fn program() -> Program {
        Program(StructuredObject {
            version: 0xaf,
            public_data: vec![],
            private_data: vec![],
            children: [0x3a, 0x45, 0x3a, 0x45, 0x3a]
                .into_iter()
                .enumerate()
                .map(|(i, id)| Chunk {
                    id,
                    data: vec![i as u8],
                })
                .collect(),
        })
    }
    fn request(generic: i32, index: i32) -> ImpulseRequest {
        ImpulseRequest {
            index,
            address: sampler_ir::SlotAddress {
                group: -1,
                slot: 3,
                generic,
            },
        }
    }

    #[test]
    fn same_index_in_mixed_base_and_snapshot_racks_never_collides() {
        let mut program = program();
        let saved = snapshot(
            vec![(0x45, vec![91]), (0x3a, vec![90])],
            &[(0, "snapshot-impulse.wav")],
        );
        let origins = EffectFileOrigins::overlay(&mut program, Some(&saved));
        let base = [(0, "base-impulse.wav".into())].into();
        for generic in [1, 1000] {
            assert_eq!(
                origins
                    .name(request(generic, 0), &base, Some(&saved))
                    .unwrap(),
                "snapshot-impulse.wav"
            );
        }
        for generic in [0, 2, 1001] {
            assert_eq!(
                origins
                    .name(request(generic, 0), &base, Some(&saved))
                    .unwrap(),
                "base-impulse.wav"
            );
        }
        assert_eq!(
            program
                .0
                .children
                .iter()
                .map(|c| c.data[0])
                .collect::<Vec<_>>(),
            [90, 91, 2, 3, 4]
        );
    }

    #[test]
    fn missing_or_empty_snapshot_table_cannot_borrow_a_base_index() {
        let base = [(0, "base-impulse.wav".into())].into();
        for entries in [vec![], vec![(0, "")], vec![(1, "different.wav")]] {
            let saved = snapshot(vec![(0x3a, vec![90])], &entries);
            let origins = EffectFileOrigins::overlay(&mut program(), Some(&saved));
            assert!(origins.name(request(1, 0), &base, Some(&saved)).is_err());
            assert!(origins.name(request(1, -1), &base, Some(&saved)).is_err());
            assert_eq!(
                origins.name(request(0, 0), &base, Some(&saved)).unwrap(),
                "base-impulse.wav"
            );
        }
    }

    #[test]
    fn script_only_snapshot_preserves_every_base_effect_namespace() {
        let saved = snapshot(vec![], &[(0, "snapshot-impulse.wav")]);
        let mut original = program();
        let before = original
            .0
            .children
            .iter()
            .map(|c| c.data.clone())
            .collect::<Vec<_>>();
        let origins = EffectFileOrigins::overlay(&mut original, Some(&saved));
        let base = [(0, "base-impulse.wav".into())].into();
        for generic in [1, 0, 2, 1000, 1001] {
            assert_eq!(
                origins
                    .name(request(generic, 0), &base, Some(&saved))
                    .unwrap(),
                "base-impulse.wav"
            );
        }
        assert_eq!(
            original
                .0
                .children
                .iter()
                .map(|c| c.data.clone())
                .collect::<Vec<_>>(),
            before
        );
    }

    #[test]
    fn production_convolution_loader_and_recipes_retain_physical_request_identity() {
        fn slot(slot: usize) -> crate::effects::Slot {
            let mut public = 1f32.to_le_bytes().to_vec();
            public.extend(0i32.to_le_bytes()); // partition size, not a file index
            for value in [0f32, 1., 20., 20_000., 1., 20., 20_000., 0.5] {
                public.extend(value.to_le_bytes());
            }
            public.extend([0; 5]);
            public.extend(0u32.to_le_bytes()); // envelope x
            public.extend(0u32.to_le_bytes()); // envelope dB
            public.extend(0i32.to_le_bytes()); // same native index in both origins
            crate::effects::Slot {
                slot,
                module: 0x16,
                version: 0x70,
                bypass: false,
                output_gain: 1.,
                dry_level: 0.,
                output_set: false,
                public,
            }
        }
        let saved = snapshot(vec![(0x3a, vec![90])], &[(0, "snapshot-impulse.wav")]);
        let origins = EffectFileOrigins::overlay(&mut program(), Some(&saved));
        let base = [(0, "base-impulse.wav".into())].into();
        let racks = vec![
            ("instrument insert".into(), vec![slot(2)]),
            ("instrument main".into(), vec![slot(5)]),
        ];
        let mut requests = Vec::new();
        let mut load = |request| {
            let name = origins.name(request, &base, Some(&saved))?;
            let amplitude = if name == "snapshot-impulse.wav" {
                0.25
            } else {
                0.75
            };
            requests.push((request, amplitude));
            Ok((48_000, vec![[amplitude; 2]; 16]))
        };
        let mut instrument = sampler_ir::Instrument::default();
        let mut recipes = Vec::new();
        let (notes, _) = crate::effects::instrument_buses_with_recipes(
            &mut instrument,
            &racks,
            &[],
            false,
            &mut load,
            Some(&mut recipes),
        );
        assert!(notes.is_empty(), "{notes:?}");
        assert_eq!(requests.len(), 2);
        assert_eq!(
            (
                requests[0].0.address.generic,
                requests[0].0.address.slot,
                requests[0].1
            ),
            (1, 2, 0.25)
        );
        assert_eq!(
            (
                requests[1].0.address.generic,
                requests[1].0.address.slot,
                requests[1].1
            ),
            (2, 5, 0.75)
        );
        for ((request, params), (loaded, amplitude)) in recipes.iter().zip(&requests) {
            assert_eq!(request, loaded);
            assert_eq!(params.ir_index, 0);
            let expected =
                crate::effects::restore_impulse(params, (48_000, vec![[*amplitude; 2]; 16]))
                    .unwrap();
            let index = requests.iter().position(|(r, _)| r == request).unwrap();
            assert_eq!(instrument.impulses[index], expected);
        }
    }
}
