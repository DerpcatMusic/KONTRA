# EffectRack absent Gain parameter compatibility

Original Clarinet and Clarinet V2 Mix Pan callbacks select an EffectRack for their second Gain write. The genuine MUI Up gesture produces raw Pan `0.02`; the current host rejects `Gain_1_1` on node 210 (original) or 224 (V2), failing at frame 96,512 after 377 completed blocks. Both receivers were independently inspected as EffectRack. The first write selects a retained, active GainMatrix. This is not an omitted GainMatrix-default defect.

The candidate ignores only finite numeric writes to an absent retained canonical `Gain_1_1` through `Gain_12_12` property on EffectRack. It creates no property, getter, saved-state delta, renderer command or DSP state. Existing retained properties, other node kinds, malformed names, nonnumeric/nonfinite values and all other unknown names retain their current checks. Insert collections retain their existing order.

## Native evidence and limits

The unchanged original common property-registry initializer executes after genuine original CRT cache, lock, heap, on-exit and synchronization initializers. Original registry access and key lookup return native-created DisplayType (ID 1) and Bypass (ID 3) metadata. No property names, IDs, guard values or encoded tables were substituted.

Static tracing of the complete EffectRack constructor closure identifies those two inherited descriptors and four null own-descriptor list arguments. Executing the complete original Rack registration routine with these exact arguments and declared pre-existing base wrapper/vector storage, followed by the complete original descriptor-name lookup, returns zero for all 144 canonical Gain names. The metadata is native-created; the pre-existing wrapper storage is an authored ABI fixture. Eight original bound Inserts index cases preserve vector positions and reject out-of-bounds indices.

The original numeric setter's missing-descriptor branches return without mutation. This branch was established by instruction tracing, rather than a complete native Lua callback execution. The full EffectRack factory remains blocked at the retained noncanonical security-cookie failure; security checks and cookies were left intact. These results do not establish full native host initialization or native audio parity.

The [official Element API](https://lua.uvi.net/class_element.html) and [element inventory](https://lua.uvi.net/_elements.html) document parameter operations and effect families. They do not establish an InsertFX Name sorting rule.

## Verification status

Completed: native original registry/accessor/key lookup, bounded original Rack registration/name lookup (150 cases), original bound Inserts index (8 cases), original/V2 baseline real MUI failure reproduction, and independent static ownership review of the candidate. A direct rustc production Pan harness compile of the earlier candidate completed successfully before the CPU stop; its host leaf hash was not captured atomically. The final candidate adds a bounded type lookup protecting synthetic contexts, plus prepared tests; it has no compilation or test verdict.

Prepared but not executed under the user's CPU constraint: regression tests for all 144 no-op names, unchanged commands/parameters/saved state, strict getter behavior, retained GainMatrix/Rack writes, Bypass, malformed names, values, known-but-unretained DisplayType, other kinds and synthetic Part/Synth contexts. Focused test compilation/execution, original/V2 genuine Pan callback success and first-leg GainMatrix/PCM effect checks remain pending. Removing the callback error alone must not be reported as proof of working Pan or whole-preset parity.
