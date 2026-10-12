//! KSP source identity is preserved independently of a control/widget binding.
use crate::hir::{Hir, Home, Ty, VarId};
use sampler_core::{ArrayFileArray, ArrayFileKind, Error, Text};

pub(crate) fn array(hir: &Hir, var: VarId) -> Result<ArrayFileArray, Error> {
    let v = &hir.vars[var.0 as usize];
    let kind = match v.ty {
        Ty::Int => ArrayFileKind::Integer,
        Ty::Real => ArrayFileKind::Real,
        Ty::Str => ArrayFileKind::Text,
        _ => return Err(Error::InvalidInput),
    };
    let (offset, len) = match v.home {
        Home::Cells { offset, len } | Home::Texts { offset, len } => (offset, len),
        _ => return Err(Error::InvalidInput),
    };
    let array = ArrayFileArray {
        key: var.0,
        name: Text::try_new(&v.name)?,
        kind,
        offset,
        len,
    };
    array.validate()?;
    Ok(array)
}
