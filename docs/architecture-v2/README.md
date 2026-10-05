# KONTRA 2.0 architecture workbench

Status: independent note/family/expression ownership and segmented PCM rendering,
2026-10-05. Production playback has not been replaced or certified.

This work starts on `docs/plan-v2-architecture`, in the isolated T3 worktree
`t3code-deca12d9`, at source commit
`ac2adc981191347bbdacaee3a29c359464ec712e`. “2.0” names the architecture effort;
it does not change the current package version or release policy.

The proposed direction is one shared musical runtime with versioned format and
behavior adapters. The immediate job is to make ownership, scheduling, preparation,
and retirement independently testable in a clean-sheet product. Explicit user direction
on 2026-10-05 removes all 1.x compatibility, reuse and migration requirements.
MIDI 2.0 is required from the event-model design onward. Third-party instrument
compatibility remains a separate, evidence-backed capability.

## Working documents

1. [Current architecture and ownership map](CURRENT_STATE.md): inspected source,
   existing safeguards, coupling, and unresolved risks.
2. [Clean-sheet target architecture and delivery plan](PLAN.md): boundaries, lifecycle
   contracts, decisions, MIDI 2.0 scope and delivery gates.
3. [Task list](TASKS.md): ordered work with dependencies and acceptance criteria.
4. [Conformance allocation](CONFORMANCE_MAP.md): all 128 supplied scenario IDs
   assigned to implementation tasks, without claiming execution.
5. [First implementation evidence](IMPLEMENTATION.md): runnable checks, actual scope,
   reproduced legacy defects, and remaining gates.
6. [Family/expression implementation and measurements](OWNERSHIP_SLICE.md): current
   contracts, independent core checks and resident-render microbenchmark.
7. [Native gates and scheduling](SCHEDULING_SLICE.md): physical/effective keys,
   sustain/sostenuto, timestamped expression and retained work ownership.
8. [Independent native executable](NATIVE_ENTRY.md): owned prepared assets, indexed
   native layer selection and a runnable WAV render path.

9. [Rust Doctor and realtime DSP policy](RUST_DSP_POLICY.md): scan evidence, CI gate,
   Rust best practices and justified DSP decisions.

10. [Native envelopes and tail ownership](ENVELOPES.md): sample-time AHDSR, retained
    release tails and callback invariant review.

Continue through **V2-01 → V2-02 → V2-03/V2-04**: define independent workload
and protocol contracts, then build unified event/continuation scheduling on the
new family/expression ownership. Full lifecycle and pedal behavior remain open.
The prototype may change freely. The old-VM KSP probe is historical evidence, not
the new scripting implementation. Legacy defects do not block v2 delivery.

## Preserved source material

The four attachments are retained byte-for-byte under `references/`. Their
SHA-256 hashes are recorded in [SHA256SUMS](references/SHA256SUMS).

| File | Role |
| --- | --- |
| [pasted-text.txt](references/pasted-text.txt) | Universal runtime, module boundaries, expression ownership, and MPE design brief |
| [deep-research-report.md](references/deep-research-report.md) | Broad research, semantic/DSP sketches, interchange and export ideas |
| [UNIVERSAL_SAMPLER_ARCHITECTURE.md](references/UNIVERSAL_SAMPLER_ARCHITECTURE.md) | Detailed proposed contracts, evidence limits, and staged delivery |
| [CONFORMANCE_SCENARIOS.json](references/CONFORMANCE_SCENARIOS.json) | 128 human-authored scenario specifications: 100 native contracts, 15 documented profile rules, 13 reference probes |

The references are research inputs, not descriptions of this checkout. External
vendor claims were not independently revalidated during this repository mapping.
The deep research report contains citation tokens from its original session;
those tokens are not recoverable source links. The architecture document includes
a named source registry. Resolve and pin the relevant primary source before
promoting a vendor rule to a verified compatibility claim.

Where the inputs differ, this plan proposes the following reconciliation:

- Preserve a source representation in addition to semantic IR and prepared render
  data; optimized region indices cannot replace script-visible source identities.
- Introduce KSP during the first semantic milestone. The broad report's initial
  scripting deferral would postpone the main stress test of KONTRA's actual needs.
- Share typed runtime services between languages, without assuming arbitrary
  UVI/Lua programs can be translated into the KSP VM.
- Treat crate lists and Rust snippets as sketches. In particular, preserve signed
  external host IDs; do not adopt the broad report's `Option<u32>` sketch blindly.
- Keep dynamic module ABI, exporters, new formats, and advanced DSP behind their
  own evidence gates. They are not prerequisites for a correct note lifecycle.

Existing [Falcon format](../FALCON_FORMAT_GROUNDWORK.md) and
[Falcon runtime](../FALCON_RUNTIME_UI_GROUNDWORK.md) audits remain relevant inputs.
They establish groundwork, not a working UVI engine. Existing compatibility notes
remain authoritative for current advertised behavior; this folder is a proposal.

## Verification and maintenance

Verify the preserved inputs from the repository root:

```sh
(cd docs/architecture-v2/references && sha256sum -c SHA256SUMS)
python3 -m json.tool docs/architecture-v2/references/CONFORMANCE_SCENARIOS.json > /dev/null
git diff --check -- . ':!docs/architecture-v2/references'
```

The whitespace check excludes the references because the supplied originals contain
trailing whitespace and are intentionally preserved unchanged. For staged changes,
add `--cached` to that check. Keep references unchanged; record amendments in the working documents. Source
line links in the map are anchored to the baseline commit above. Keep them as historical evidence when the new implementation replaces that subsystem. Task completion requires executable evidence;
an existing test name, a parsed JSON file, or a checked planning item is not a
passing conformance result. Keep measured results separate from the original catalogue.
