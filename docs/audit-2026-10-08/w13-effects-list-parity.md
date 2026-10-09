# W13: native group insert inventory

One row from the 84-axis table: **Full group/instrument insert/send/main/bus FX list**. Clean 608a20a1's broad Present label missed native Kontakt group inserts: the translator stores them on shared zone voice chains, while the Effects list read only `Group.chain`.

Port from v1 0cb7a8a0:src/ui/chain.rs, `effects`: show the complete group insert inventory under one heading before the instrument and bus lists. Adapt the native ownership lookup to the existing v2 graph implementation. The graph and Effects list now share that lookup; repeated zone and group references show each chain once, and another group's voice chains stay excluded.

Failing-first `ui::v2_tests::v1_effects_list_includes_native_zone_chains_once_per_group` passed its existing group/instrument/bus assertions, then failed because the native Ladder entry count was zero. It passed after the port (1/1). Its rendered synthetic fixture covers two distinct voice chains, repeated references, a summed group chain, another group's effects, and a chain referenced by both zones and group. The existing native Ladder and voice/group graph ownership regressions passed (2/2); root `cargo test --profile ci --no-run` passed. All cargo ran through `kontakto-heavy`.

The fixture checks UI scene text, not presented host pixels, live bypass changes, or native DSP parity. The frozen 0.3.326 artifact does not include this follow-up.

Suggested release note: “The Sound editor now lists native Kontakt group inserts.”

NEXT: restore live key/velocity modulation source readouts; pause for W6's CLAP-window handoff.
