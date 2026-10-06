//! Control-side constant evaluation and bounded integer-array preparation.
use super::{Error, Instruction, Kind, Parser};
use sampler_core::ScriptArray;

impl Parser<'_> {
    pub(super) fn constant(&mut self) -> Result<i32, Error> {
        let start = self.code.len();
        self.scalar(0)?;
        let mut values = Vec::new();
        for op in &self.code[start..] {
            match *op {
                Instruction::SetLocal { local, value } => {
                    values.resize(values.len().max(usize::from(local) + 1), 0);
                    values[usize::from(local)] = i32::try_from(value)
                        .map_err(|_| self.error("constant exceeds signed 32-bit range"))?;
                }
                Instruction::Binary32 {
                    lhs,
                    rhs,
                    operation,
                } => {
                    values[usize::from(lhs)] =
                        operation.apply(values[usize::from(lhs)], values[usize::from(rhs)]);
                }
                Instruction::Unary32 { local, operation } => {
                    let value = &mut values[usize::from(local)];
                    *value = operation.apply(*value);
                }
                _ => return Err(self.error("constant integer expression required")),
            }
        }
        let result = values[0];
        self.code.truncate(start);
        Ok(result)
    }

    pub(super) fn array_declaration(&mut self) -> Result<ScriptArray, Error> {
        self.symbol(b'[')?;
        let len = self.constant()?;
        self.symbol(b']')?;
        if !(1..=1_000_000).contains(&len) {
            return Err(self.error("array size must be between 1 and 1000000"));
        }
        let len = len as usize;
        let total = self
            .array_cells
            .checked_add(len)
            .filter(|n| *n <= self.array_limit)
            .ok_or_else(|| self.error("array cell budget exceeded"))?;
        let end = self
            .globals
            .len()
            .checked_add(len)
            .filter(|n| u32::try_from(*n).is_ok())
            .ok_or_else(|| self.error("native script-cell index range exceeded"))?;
        let array = ScriptArray {
            offset: self.globals.len() as u32,
            len: len as u32,
        };
        self.globals
            .try_reserve_exact(len)
            .map_err(|_| self.error("array allocation capacity exceeded"))?;
        self.globals.resize(end, 0);
        self.array_cells = total;
        Ok(array)
    }

    pub(super) fn array_initializer(&mut self, array: ScriptArray) -> Result<(), Error> {
        self.symbol(b'(')?;
        let mut index = 0;
        loop {
            if index == array.len as usize {
                return Err(self.error("too many array initializers"));
            }
            let value = i64::from(self.constant()?);
            self.globals[array.offset as usize + index] = value;
            index += 1;
            match self.next()?.kind {
                Kind::Symbol(b',') => {}
                Kind::Symbol(b')') => {
                    self.globals
                        [array.offset as usize + index..(array.offset + array.len) as usize]
                        .fill(value);
                    return Ok(());
                }
                _ => return Err(self.error("expected comma or closing array initializer")),
            }
        }
    }

    pub(super) fn initial_array_write(&mut self, array: ScriptArray) -> Result<(), Error> {
        self.symbol(b'[')?;
        let index = self.constant()?;
        self.symbol(b']')?;
        self.symbol(b':')?;
        let value = i64::from(self.constant()?);
        let index = u32::try_from(index)
            .ok()
            .filter(|n| *n < array.len)
            .ok_or_else(|| self.error("array index outside declared bounds"))?;
        self.globals[(array.offset + index) as usize] = value;
        Ok(())
    }
}
