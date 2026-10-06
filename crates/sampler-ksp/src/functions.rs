use super::{CallbackKind, Error, Instruction, Kind, Parser, Program};

impl<'a> Parser<'a> {
    fn function_name(&mut self) -> Result<&'a str, Error> {
        let token = self.next()?;
        let Kind::Word(name) = token.kind else {
            return Err(self.error("expected function name"));
        };
        if name.starts_with(['$', '%', '@', '~', '?', '!']) {
            return Err(self.error("function name cannot be a variable"));
        }
        let checkpoint = self.offset;
        if self.next()?.kind == Kind::Symbol(b'(') {
            self.symbol(b')')?;
        } else {
            self.offset = checkpoint;
        }
        Ok(name)
    }

    pub(super) fn function(&mut self) -> Result<(), Error> {
        let name = self.function_name()?;
        if self.functions.contains_key(name) {
            return Err(self.error("duplicate function"));
        }
        let (start, emitted) = (self.offset, self.emitted);
        let mut end = start;
        let kinds = [
            CallbackKind::Note,
            CallbackKind::Release,
            CallbackKind::Control,
            CallbackKind::Controller,
        ];
        // Context-dependent operations lower once per supported caller context.
        // Previously declared templates are expanded iteratively; no recursion or
        // audio-thread call stack is introduced. Total template storage is bounded.
        let bodies = kinds.map(|kind| {
            self.offset = start;
            self.emitted = 0;
            self.code.clear();
            let code = self.body(kind, true)?;
            let program =
                Program::new(code.clone()).map_err(|_| self.error("invalid function operands"))?;
            if !kind.accepts(&program) {
                return Err(self.error("function requires a different event context"));
            }
            end = self.offset;
            Ok(code)
        });
        self.code.clear();
        self.emitted = emitted;
        self.offset = end;
        if bodies.iter().all(Result::is_err) {
            return Err(*bodies[0].as_ref().unwrap_err());
        }
        let width = bodies
            .iter()
            .filter_map(|body| body.as_ref().ok())
            .map(Vec::len)
            .max()
            .unwrap();
        self.function_instructions = self
            .function_instructions
            .checked_add(width)
            .filter(|&count| count <= self.limit)
            .ok_or_else(|| self.error("function instruction budget exceeded"))?;
        self.functions.insert(name, bodies);
        Ok(())
    }

    pub(super) fn call_function(&mut self, kind: CallbackKind) -> Result<(), Error> {
        let name = self.function_name()?;
        let bodies = self.functions.get(name).ok_or_else(|| {
            self.error("function must be declared before use; recursion is unsupported")
        })?;
        let context = match kind {
            CallbackKind::Note => 0,
            CallbackKind::Release => 1,
            CallbackKind::Control => 2,
            CallbackKind::Controller => 3,
        };
        let body = bodies[context].as_ref().map_err(|error| *error)?;
        let emitted = self
            .emitted
            .checked_add(body.len())
            .filter(|&count| count <= self.limit)
            .ok_or_else(|| self.error("instruction budget exceeded"))?;
        let base = self.code.len();
        base.checked_add(body.len())
            .ok_or_else(|| self.error("instruction budget exceeded"))?;
        // ponytail: inlining grows with call sites; shared bytecode calls are needed
        // if real libraries exceed the admitted instruction-storage budget.
        self.code.extend(body.iter().map(|&op| match op {
            // Template targets include its exclusive end: returning falls through
            // into the caller, whereas an explicit exit retains native End.
            Instruction::Jump { target } => Instruction::Jump {
                target: base + target,
            },
            Instruction::JumpIfZero { local, target } => Instruction::JumpIfZero {
                local,
                target: base + target,
            },
            other => other,
        }));
        self.emitted = emitted;
        Ok(())
    }
}
