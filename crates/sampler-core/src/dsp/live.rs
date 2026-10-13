//! Control-driven 2x2 stereo matrix for scripted insert effects: each cell follows a control.
//! (The control-driven biquad moved into `eq_family::generic`.)
use super::{Planar, kernels, control::{ControlRamp, PreparedParameter}};

/// A 2x2 matrix (rows are output L/R, columns input L/R) with control cells.
pub(super) fn matrix(
    cells: &[PreparedParameter; 4],
    parameters: &[ControlRamp],
    block: &mut Planar,
    len: usize,
    at: u64,
) {
    let [left, right] = block;
    for (i, (l, r)) in left[..len].iter_mut().zip(&mut right[..len]).enumerate() {
        let c = cells.map(|cell| cell.value(parameters, at + i as u64, None));
        [*l, *r] = kernels::matrix([*l, *r], [[c[0], c[1]], [c[2], c[3]]]);
    }
}
