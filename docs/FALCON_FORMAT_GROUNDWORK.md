# Falcon / UVI format groundwork

This is metadata research, not Falcon playback compatibility. The host importer still accepts Kontakt NKI/NKM; UFS and UVIP are not added to its playable catalog.

## Runnable artifact

```sh
python3 tools/inspect_uvi.py --self-test
python3 tools/inspect_uvi.py /path/to/bank.ufs /path/to/patch.uvip > private-inventory.jsonl
```

The inspector reads exactly 328 bytes per UFS. Plain-XML UVIP reads are capped at 2 MiB, 10,000 elements and depth 64, with node/depth limits enforced during tree construction. DTD/entity declarations are rejected by the parser callback regardless of XML encoding. JSON keeps numeric UFS fields under their offsets, the raw header, every XML tag/attribute and source/text hashes. Embedded script text is not dumped. Unknown module names and API versions remain metadata and explicitly have no runtime support. Unknown UFS layouts and UVIP roots fail with an error; no extension or filename establishes playability.

The self-check uses entirely authored bytes and XML. It verifies unknown attributes/modules/API versions survive, opaque UFS bytes round-trip, and unsupported/malformed inputs fail. No third-party scripts, samples, keys or fixture bytes ship with this change.

## Installed corpus observations (2026-10-02)

The scoped `/mnt/MAIN_STORAGE/Libraries` inventory found 25 UFS files under UVI, totaling 5,812,620,371 bytes, and no loose UVIP, UVIM or Lua files. All 25 share the following observed layout:

| Byte offset | Observation | Meaning established? |
| --- | --- | --- |
| 0 | ASCII `UFS2` | Identifies this observed layout; not a complete version contract |
| 4 | little-endian u32 `3` | Numeric field only; not named a format version |
| 8–31 | Variable or zero bytes | Opaque, preserved |
| 32 | little-endian u64 | 22 files have actual length + 12; three have actual length + 16,644; not validated as size/offset |
| 40 | little-endian u64 `328` | Observed field/layout discriminator |
| 48 | NUL-terminated readable name | All observed name lengths 11–28 bytes |
| after name | Nonzero variable bytes | Opaque; not assumed to be padding |
| 304–319 | Same 16 bytes across this corpus | Opaque; not interpreted or required by inspector |
| 320 | little-endian u64 `272` | Numeric field only |

Bounded first/near-header/last 32 KiB windows totaled 2,457,600 bytes. They exposed no readable preset, script, sample-path or XML references. This establishes an opaque boundary in the inspected windows, **not proof of encryption or absence of clear data elsewhere**. No directory index, sample codec or program loader was recovered.

Additional scoped likely user/music/host locations produced no loose UFS/UVIP/UVIM/DMAP or Falcon/UVI host-resource matches. This does not establish system-wide absence; no unbounded mounted-volume traversal was performed.

Private evidence is in ignored `artifacts/falcon-compatibility-private/`: `paths.json`, `headers.json`, `header328.json`, `bounded-content-scan.json`, `additional-paths.json`, `host-resource-paths.json`, `inspector-output.jsonl` and `validation-summary.json`. Keep generated metadata and downloaded third-party examples private.

## Concrete clear program evidence

The [official UVI examples gallery](https://lua.uvi.net/_examples_page.html) links two downloaded XML patches: [MappingArticulations.uvip](https://lua.uvi.net/MappingArticulations.uvip), 8,240 bytes, SHA-256 `98f403a58b92a4e04364094c08d4b114edc93549cf3937f4dd5bb745973bcb2d`; and [FXControls.uvip](https://lua.uvi.net/FXControls.uvip), 5,143 bytes, SHA-256 `b17f3895d0e78b4c16d6ab5691f39ac080a2b70a983705c228e1fe086478cf0e`. Copies stay private because redistribution terms were not established.

Both root at `UVI4`, contain `Program/Layers/Layer/Keygroups/Keygroup`, embed `ScriptProcessor/script` with observed `API_version="13"`, and contain DAHDSR and SignalConnection modules. The mapping example uses SampleMappingOscillator with an empty MappingPath; the other uses MinBlepGenerator and Drive. These facts ground topology/attribute inventory only. No mapping, sample audio or soundbank index is supplied by these examples, and no engine behavior was verified.

## Next parser gate and engine boundary

UVI describes [UFS as a monolithic container](https://support.uvi.net/hc/en-us/articles/201360562-What-is-a-UFS-file) containing patches, samples and other resources. These support docs do not supply a byte-level index or codec specification. Actual bank parsing needs a documented reader or authorized export that exposes member names and clear program/sample data; header success cannot satisfy that gate.

The [official mapping guide](https://lua.uvi.net/_sample_mapping_intro.html) defines an open `layers/layer/zone` DMAP/XML format, with file-relative sample paths, key/velocity/root/tuning/gain fields, layer order as dim1 and rr minus one as dim2. This is a concrete follow-on parser frontier once authored/real mapping fixtures are available. It is separate from the uppercase keygroup attributes in UVIP. Round-robin routing, purge/load completion and streamed sample offsets require engine work after parsing.

Existing `src/import.rs` supplies reusable Instrument/Group/Zone data and sample resolution, but its archive access is Kontakt NKR and scripts are KSP. Do not route UFS through NKR or map Falcon's graph into groups without preserving distinctions. Plain sampled zones can eventually reuse `src/audio.rs` decoding and engine streaming; oscillators, modulation connections, scoped effects, UVIScript and its host API need their own semantic lowering/runtime evidence.

The [Falcon manual](https://uvi.s3.us-east-1.amazonaws.com/UVIFC/falcon_2026_manual.pdf) defines Multi → Part → Program → Layer → Keygroup → Oscillator and save-with-samples operations. An authorized headless exporter could provide that hierarchy, parameter types/values, module identities, script source/API version and resolved sample/resource paths; no such CLI was found or executed in this stage. Full compatibility remains gated on those records, runtime/UI contracts and DSP/reference validation.
