//! Format-neutral Luau runtime shared by every script front end.
//!
//! Owns the VM lifecycle (state creation, library set, memory limit, compiler
//! policy), the work budget that bounds one callback, and GC step budgeting.
//! Format crates (`sampler-uvi` today, the KSP front end next) supply the API
//! surface and the engine mapping; they never create a VM themselves.
//! Design: `docs/architecture-v2/UNIVERSAL_SCRIPT.md`.

mod budget;
mod cache;
mod gc;
mod vm;

pub use budget::{Budget, BudgetExhausted};
pub use cache::{bytecode as cached_bytecode, default_dir as cache_dir};
pub use gc::bounded_gc_step;
pub use vm::{Jit, check_syntax, compile_bytecode, compiler, libs, new_vm, new_vm_with};

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;
    use std::time::Instant;

    #[test]
    fn vm_has_only_the_granted_libraries_and_enforces_memory() {
        let lua = new_vm(1 << 20).unwrap();
        let globals = lua.globals();
        assert!(globals.get::<mlua::Table>("string").is_ok());
        assert!(globals.get::<mlua::Value>("io").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("os").unwrap().is_nil());
        assert!(globals.get::<mlua::Value>("package").unwrap().is_nil());
        let grow = lua.load("local t = {} for i = 1, 1e7 do t[i] = i end").exec();
        assert!(grow.is_err(), "1 MiB limit must stop large allocation");
    }

    #[test]
    fn budget_stops_an_infinite_loop_at_its_checkpoint_limit() {
        let lua = new_vm(64 << 20).unwrap();
        let budget = Rc::new(Budget::new(500, None));
        let hook = Rc::clone(&budget);
        lua.set_interrupt(move |_| {
            hook.checkpoint()?;
            Ok(mlua::VmState::Continue)
        });
        let err = lua.load("while true do end").exec().unwrap_err().to_string();
        assert!(err.contains("work budget exceeded"), "{err}");
        assert!(budget.exhausted.get());
        assert_eq!(budget.checkpoints.get(), 501);
        budget.refill(10, Some(Instant::now()));
        assert!(!budget.exhausted.get());
        assert_eq!(budget.remaining.get(), 10);
    }

    #[test]
    fn syntax_check_accepts_luau_and_rejects_garbage() {
        assert!(check_syntax("local x: number = 1 return x").is_ok());
        assert!(check_syntax("local = = =").is_err());
    }

    #[test]
    fn bounded_gc_reports_progress_without_a_full_collect() {
        let lua = new_vm(64 << 20).unwrap();
        lua.load("local t = {} for i = 1, 10000 do t[i] = {i} end").exec().unwrap();
        let mut steps = 0;
        while !bounded_gc_step(&lua, 1).unwrap() {
            steps += 1;
            assert!(steps < 100_000, "collector must finish");
        }
    }

    #[test]
    fn cached_bytecode_runs_like_source() {
        let source = "local t = {} for i = 1, 4 do t[i] = i * 2 end return t[4]";
        let bytes = compile_bytecode(source).unwrap();
        let lua = new_vm(64 << 20).unwrap();
        let value: i64 = lua.load(&bytes[..]).eval().unwrap();
        assert_eq!(value, 8);
    }
}
