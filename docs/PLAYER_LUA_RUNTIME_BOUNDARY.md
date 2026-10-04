# Player Lua runtime boundary

The concurrent main refactor's Kontakt NativeUI and isolated UVI use different Lua runtimes. On 2026-10-04, main requested `mlua =0.12.1` with `lua54,vendored,send`; isolated UVI requested `lua51,vendored,error-send`. A combined Cargo graph would unify those features, and [mlua's build guard](https://github.com/mlua-rs/mlua/blob/v0.12.1/mlua-sys/build/main.rs) rejects two VM variants. `send` also changes closure/userdata bounds ([mlua documentation](https://docs.rs/mlua/0.12.1/mlua/#send-and-sync-support)). Current UVI state is worker-local `Rc`/`RefCell`; main's new NativeUI consumes KSP controls. Neither runtime can be substituted without checking its real consumers.

## Measured Linux coexistence

An authored Linux x86_64 probe independently linked two small Rust `cdylib`s to already-built static Lua 5.1/5.4 archives. This avoids Cargo feature unification between the modules. It used no vendor content, production backend implementation or new framework.

Each module exposed one C function-table entry. `nm -D --defined-only` showed only that entry; `readelf --syms` showed 117/144 Lua API/library-entry symbols bound `LOCAL`. Neither module dynamically depended on Lua. Isolation came from the actual local symbol binding, not an assumption that `RTLD_LOCAL` alone isolates libraries; the [Linux loader documentation](https://man7.org/linux/man-pages/man3/dlopen.3.html) explains global lookup and promotion limits.

Four fresh-process cases passed: both library load orders, each with `RTLD_LOCAL` and `RTLD_GLOBAL`. Both live VMs reported their own `_VERSION`, called a module-owned callback with distinct owner state, collected temporary userdata, recovered after a protected authored error, and finalized retained userdata during `lua_close`. After one module closed and disappeared from `/proc/self/maps`, the other still ran callbacks and GC. Both modules unmapped after their owners were destroyed. This establishes coexistence for these artifacts; a successful `dlclose` generally does not itself guarantee unmapping.

## Ownership required at this boundary

A future implementation must resolve each function table through its library handle and keep that module/allocator responsible for creating and destroying its opaque handles. The probe exchanged only C-callable function pointers, fixed-width scalars/arrays, caller-owned report storage and opaque handle tokens. `Lua`, `Table`, Rust references, strings, vectors, trait objects and shared smart pointers did not cross between modules. [C representation](https://doc.rust-lang.org/reference/type-layout.html#the-c-representation) establishes layout, not a complete compatibility contract; version/size negotiation, buffer bounds, errors, thread affinity and ownership still need explicit agreement.

The module must remain loaded until all VMs, userdata finalizers, workers, callbacks, queued work and audio endpoints that can invoke its code have retired. `lua_close` ran while callback owner storage was still valid, then that storage was freed, then the library closed. There were no background threads in this probe. Production UVI needs its existing endpoint retirement and worker stop/join proof before a module lease can be released. No foreign pointer can be freed through a different allocator or passed to the other VM.

## Limits and alternatives

This is not an integrated player/backend ABI. No current UVI Worker, Slot, packet transport, source loader, shared PCM service, persisted state, stamped UI command or core mixer was moved across a dynamic boundary. Routing, physical ports, precise MIDI values, asynchronous retirement, real-time deadlines, load failures and host teardown remain unproved here. Shared sampler services must retain one agreed owner; copying their global budgets/caches into independent modules is not established by this test. Windows/macOS linking and unload behaviour were not tested.

A coordinated single-runtime policy is an alternative, but changing Lua version or enabling `send` requires actual NativeUI/UVI semantic and ownership checks. Process separation is another boundary with additional transport/lifecycle work. Renaming the same mlua dependency does not prevent Cargo feature unification. No alternative was implemented by this probe.

## Reproduction scope

The authored sources, exact compiler arguments, archive hashes, symbol dumps and four-case results are retained in the private `dual-lua-cdylib-proof` evidence bundle. They remain outside production tools: the hand-declared Lua C APIs are a bounded architecture experiment, not a maintained host interface.

To reproduce, locate separately built `liblua5.1.a` and `liblua5.4.a` in the relevant target trees; independently link the authored probe with `rustc --crate-type cdylib`, its matching Lua API variant, and that archive. Inspect each resulting library with `nm -D --defined-only`, `readelf --wide --syms` and `readelf -d`. Run the C-table client in fresh processes for both load orders and visibility modes. Verify distinct versions/context callbacks, protected error recovery, GC/close counts, actual unmapping and survivor execution. Record compiler/platform and archive hashes; do not generalize a cached-artifact result to production or another platform.
