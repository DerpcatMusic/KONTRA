# AO bounded-load follow-up

The census on optimized product checkpoint `9993db69` observed one timeout among
346 completed AO presets: `Presets/08 Ambient/Coline MW.uvip`, 91.126 s at
`load program 0`. This outer checkpoint does not identify an inner spinning stage.

The exact frozen scanner (SHA-256
`19e2f2c76771ceeb1f5db47956d404a290e16ef61dabb6e34909176b362b6751`)
completed a read-only Coline retry: load 8.533 s, audible, 66/66 bindings,
Original missing-images, zero Lua faults. Shared `KONTRA_AUDIT_LOAD` tags measured
bank open 638 ms, program decrypt 57 ms, XML translation 93 ms, script resources
7 ms, and `uvi_lua_init` 7,419 ms. No permanent spin reproduced. The historical
timeout's precise cause remains unknown; the dominant stage in this retry was Lua
initialization, not sample preload.

## Systemic fix

`Shared::arm` previously renewed the load allowance for the script body,
restoration, and onInit. Graph construction and the Rust deferred-work loop did
not check it, and exhausted initialization could still return a partial host.
A failing-first infinite-onInit check demonstrated that admission bug.

Graph construction and all script initialization phases now share the initial
`Config::load` deadline (20 s by default). Graph traversal and deferred batches
check it cooperatively. Expiration rejects admission with the fixed reason
`uvi_lua_init unsupported: initialization time budget exceeded`; live callbacks
still receive independent callback budgets. The existing script-thread loader
propagates the failure, without detaching a runaway worker or adding a timeout
thread. Post-init UI snapshots and container I/O are separate work; this is not
a hard wall-clock timeout on every possible I/O operation.

## Targeted verification

On the changed production translate → script attachment → streamed-preparation
path, these AO presets all loaded within the scanner's 90 s budget:

| Preset | Debug admission time | Shared Lua-init span |
| --- | ---: | ---: |
| Coline MW | 51.920 s | 23.241 s |
| Diamond Crackling | 32.125 s | 23.663 s |
| Antartide | 33.047 s | 23.382 s |

The Lua-init parent span includes post-init UI snapshots, beyond the host's
initialization deadline. These debug witnesses establish bounded admission,
not a speed comparison against optimized census rows (Diamond 56.6 s and
Antartide 36.2 s in that census). All three use the same guarded host path.

`sampler-uvi` area no-run passed; the deadline non-renewal test, infinite-onInit
and repeating-spawn rejection cases, and 11 host-parameter checks passed. The
three installed witnesses passed separately under an external 100 s test-process
timeout and the internal 90 s assertion. Logs are in
`~/.cache/kontakto-w10/{coline-before/,coline-*,diamond-after.log,antartide-after.log}`.
No full suite, full 660 sweep, new plugin release or install was run for this fix.
