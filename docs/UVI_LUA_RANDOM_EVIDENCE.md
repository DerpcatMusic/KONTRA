# UVI Lua random-number parity evidence

Native random-event and PCM parity remain unproved. Linux fixed-seed replay hashes are regression evidence within that Linux runtime; they do not establish equality with the official Windows stream. This gate does not change existing musical API admission or playback.

The repository selects vendored Lua 5.1 through mlua. The vendored `lmathlib.c` calls `rand()` and `srand()` from its platform C runtime. [Lua 5.1.5 source](https://www.lua.org/source/5.1/lmathlib.c.html) specifies the adapter arithmetic; [UVI's Lua reference](https://lua.uvi.net/_lua_reference.html) lists `math.random` and `math.randomseed`, but specifies no cross-platform stream or activation-seeding policy. [Microsoft's CRT reference](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/rand?view=msvc-170) documents its 32767 output maximum.

## Measured implementation differences

The unchanged official UVI Workstation 4.0.9 x64 reader has SHA-256 `78729e96b752aea746280275072ad24cb4399a053739c49a161ff1fcfbf85721`. Its Lua registration table points `math.random` to `0x1416cf150` and `math.randomseed` to `0x1416cf2a0`. Protected on-disk code is not a usable instruction image. The existing private loaded-text capture resolves those bindings.

Bounded instruction execution runs the unchanged loaded Lua bindings, integer conversion, CRT seed assignment/update, fraction scaling, floor and result write. Only OS CRT-context lookup is replaced by a single authored context. No bank program, full native VM creation, native activation lifecycle or PCM runs inside this helper.

- Native CRT update: `state = state * 214013 + 2531011`, wrapping at 32 bits; raw output is `(state >> 16) & 32767`. The Lua adapter uses `(raw % 32767) / 32767`.
- Linux's compiled environment has `RAND_MAX = 2147483647`; the SDK's actual Lua output matches that libc sequence. For explicit seed 1, native first output is `41 / 32767` (about 0.0012512589), while Linux returns about 0.8401877172. Twelve outputs differ for each of five authored seed probes.
- Native fractional output has 32767 reachable values. Raw outputs 0 and 32767 both map to zero. This describes adapter support and raw preimages, not a measured uniform distribution of the generator.
- With the authored `math.random(65536)` probe, native arithmetic permits only 32767 distinct integer results and its greatest result is 65534. Large-interval distribution therefore differs independently of seed.
- Native seed conversion agrees with Linux for the tested seeds 1, 2, -1, 4294967297 and 1.9: integer truncation and low 32 bits give the same initial seed. These probes do not establish behavior for nonfinite, out-of-i64 or nonnumeric inputs.
- The native binding consumes a random value before checking interval/argument-count errors. Invalid authored intervals and arity advance the state; this matches the vendored Lua source order.
- Isolated Linux probes prove that reseeding a second VM, including on another thread, changes the first VM's next output. The native CRT uses an OS-context getter instead of a Lua-state field; fresh-context initialization and reuse are measured below; actual VM/thread/fiber scheduling and activation seeding remain unmeasured. Replacing either stream with an assumed per-VM seed would be a behavior change.

## Native CRT context and initialization evidence

Additional bounded instruction fixtures execute the original CRT initializer (`0x141a01074`), the original context getter (`0x141a012c4`) on both lazy-allocation and reuse paths, and the original Lua bindings. Only OS FLS/TLS lookup/set, allocation, locale initialization and last-error plumbing are authored substitutes. The fixtures select their OS contexts; they do not observe Workstation's actual execution-thread or fiber assignment.

- The initializer writes seed 1 to the CRT context's state field at offset `0x28`. A fresh context's first Lua result, without a `randomseed` call, is `41 / 32767`.
- The getter initializes a missing context once and returns an existing context without reseeding it. Two authored OS contexts cause exactly two allocations and two original initializer invocations; returning to the first context continues its sequence.
- Two authored Lua states receiving the same CRT context share the stream. The second state's first draw is `18467 / 32767` after the first state's `41 / 32767`; reseeding the second state changes the first state's next draw.
- An unchanged non-Lua two-`rand` call block (`0x14114474e..0x14114476c`) receiving that same context consumes the next two raw values, 18467 and 6334. The following Lua draw is `26500 / 32767`. This proves sharing with that native caller block; its containing engine method, source identity and actual scheduling are not measured.

The original UVI Lua-constructor math-registration block (`0x1413946ea..0x14139471a`) and its original math opener (`0x1416cf2c0`) were also executed with authored Lua stack/registry/closure plumbing. The registration table resolves the same `random` and `randomseed` bindings above. Installing the math library leaves an existing CRT seed of 12345 unchanged and invokes neither RNG binding nor CRT `rand`/`srand` in those measured blocks. Preceding and following constructor hooks, full VM creation and instrument activation were not executed.

These results establish a default for a **fresh CRT OS context**, not for each instrument, session or Lua VM. An instrument may run on an existing context whose stream was seeded or advanced by another caller. Sharing applies to callers of this reader's measured CRT implementation with the same context; sharing across other native modules or different CRT instances is not established. A per-session seed-1 replacement would change the demonstrated reuse behavior and remains unsupported by this evidence.

## Owned program relevance and limits

Read-only text inventories of the approved Alto Flute 2 and Clarinet A V2 programs plus deduplicated bank modules each contain two `math.random` call occurrences, both in two-argument form, and no `math.randomseed` occurrences. Inventory counts alone do not prove dynamic call frequency or exclude aliases, host-side initialization, other modules or native seeding. Bank source and private access values remain in memory; receipts retain public API counts and hashes only.

No RNG override, default seed, activation seed, mutex, queue change or vendor-binary modification is proposed. Keep native random-event/PCM fidelity claims closed until unchanged Workstation probes establish initialization policy, context sharing/interleaving and matching event mappings. Seeded same-runtime replays remain useful when isolated from competing CRT users and their diagnostic seed is disclosed.

## Private receipts

The study artifacts are `owned-random-inventory-safe.json`, `native-random-safe.json`, `linux-random-safe.json`, `native-ownership-safe.json`, `native-context-getter-safe.json`, `native-math-open-safe.json`, and reproducible authored helper sources in `kontakto-uvi-random-audit-private`. They are separate from the unused named-collection allocation candidate. No full Cargo/plugin build was performed.
