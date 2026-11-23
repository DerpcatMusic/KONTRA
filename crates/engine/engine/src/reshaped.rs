//! Which of a group's routes reshape its timed sources, found once at load,
//! so a voice's refresh visits those routes instead of every mod per source.

use kontra_ir as ir;

use crate::Group;

/// One group's intensity routes (by the source they scale) and LFO phase
/// and rate routes (by LFO), as indexes into its mods, in mod order. Bypass
/// is left to the reader (a script may toggle it); a route's kind and
/// sources never change after load.
#[derive(Debug, Default)]
pub(crate) struct Reshaped {
    intensity: Box<[(ir::Source, usize)]>,
    lfo: Box<[(u8, usize)]>,
    envelope: Box<[(u8, usize)]>,
}

impl Reshaped {
    /// Not real-time.
    pub(crate) fn of(group: &Group) -> Self {
        let routes = group.mods.iter().enumerate();
        Self {
            intensity: (routes.clone())
                .filter_map(|(i, m)| match m.route.target {
                    ir::ParamAddr::RouteIntensity { of } => Some((of, i)),
                    _ => None,
                })
                .collect(),
            envelope: (routes.clone())
                .filter_map(|(i, m)| match m.route.target {
                    ir::ParamAddr::EnvelopeStage {
                        envelope: Some(n), ..
                    } => Some((n, i)),
                    _ => None,
                })
                .collect(),
            lfo: routes
                .filter_map(|(i, m)| match m.route.target {
                    ir::ParamAddr::LfoParam {
                        lfo,
                        param: ir::LfoParam::Phase | ir::LfoParam::Rate,
                    } => Some((lfo, i)),
                    _ => None,
                })
                .collect(),
        }
    }

    /// The mods that may scale `of`.
    pub(crate) fn scaling(&self, of: ir::Source) -> impl Iterator<Item = usize> + '_ {
        (self.intensity.iter())
            .filter(move |&&(o, _)| o == of)
            .map(|&(_, i)| i)
    }

    /// The mods that may shape breakpoint envelope `n`'s stages.
    pub(crate) fn enveloping(&self, n: u8) -> impl Iterator<Item = usize> + '_ {
        (self.envelope.iter())
            .filter(move |&&(e, _)| e == n)
            .map(|&(_, i)| i)
    }

    /// The mods that may move LFO `n`'s phase or rate.
    pub(crate) fn moving(&self, n: u8) -> impl Iterator<Item = usize> + '_ {
        (self.lfo.iter())
            .filter(move |&&(l, _)| l == n)
            .map(|&(_, i)| i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_route_is_found_under_its_source_bypassed_or_not() {
        let mut group = Group::standard();
        let route = |r: ir::ModRoute| ir::Mod {
            name: String::new(),
            slot: 0,
            target: 0,
            bypass: true,
            route: r,
        };
        let (velocity, depth) = (ir::Source::Velocity, 1.0);
        let envelope = ir::Source::Envelope(40);
        group.mods = vec![
            route(ir::ModRoute::new(
                velocity,
                ir::ParamAddr::LfoParam {
                    lfo: 2,
                    param: ir::LfoParam::Phase,
                },
                depth,
            )),
            route(ir::ModRoute::new(
                velocity,
                ir::ParamAddr::RouteIntensity { of: envelope },
                depth,
            )),
        ];
        let r = Reshaped::of(&group);
        assert_eq!(r.moving(2).collect::<Vec<_>>(), [0]);
        assert_eq!(r.moving(1).count(), 0);
        assert_eq!(r.scaling(envelope).collect::<Vec<_>>(), [1]);
        assert_eq!(r.scaling(ir::Source::Lfo(0)).count(), 0);
    }
}
