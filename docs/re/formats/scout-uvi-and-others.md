# Scout: public decoders and write-ups for UVI and other sampler formats

Date: 2026-10-27. Read-only scout. Nothing cloned into KONTRA, nothing built,
nothing committed. Sources were GitHub REST (`gh api`), GitHub search (`gh
search repos`/`code`), and web search/fetch. Each claim below is marked
**verified** (I read the file or API result) or **search snippet** (I saw it
only in a search summary, not fetched).

Limits of this pass:

- GitHub search was rate-limited for part of the run. Repo metadata came from
  direct REST reads. Repo searches for `hise ch1`, `sampletank pak`,
  `vstsound`, `eastwest opus`, and `ample ascf` returned nothing and were
  re-run after the reset.
- Web search was used for SINE, HALion, Toontrack, Ample, Spitfire, SampleTank,
  EastWest Opus, UFS, and UVIScript. Those results are summaries, not fetched
  documents.
- Nothing was cloned. `/data/dev/workspace/Research/scout/` was created empty
  for future use.

## 1. Bottom line

- **No public UVI UFS2 decoder, spec, or encryption write-up exists.** A code
  search for `ufs2` returned only unrelated filesystem tools (testdisk,
  sleuthkit). A repo search for `uvi ufs` returned nothing. KONTRA's
  `crates/sampler-uvi` decoder is the only one found. Self-reliance is the
  right call here.
- **UVI program files (`.uvip`) are plain XML** (`<UVI4><Program ...>`). This
  is verified on a real file. KONTRA already parses this XML.
- **The strongest public reference for "many sampler formats" is
  ConvertWithMoss** (LGPL-3.0, Java, active). It has readers for roughly 50
  formats, including EXS24, Kontakt NKI/NKM, SF2, SFZ, DecentSampler, and
  several Akai/E-mu/Kurzweil families. It does **not** read UVI, Ample,
  EastWest, Spitfire, SINE, Toontrack, HALion, or SampleTank.
- **HISE's `.ch1` monolith and HLAC codec are open (GPLv3)** and are the
  primary source for HISE's own format. KONTRA is Apache-2.0 and GPL code
  cannot be copied into it, so we re-implement from observed behaviour, not copy code.
- **Closed formats with no public information found:** EastWest Opus
  (`.ewi`/`.ewp`), Orchestral Tools SINE, Toontrack `.ezd`/TPC II, Ample
  `.ascf` (KONTRA has the only RE, and its census says the container is still
  unknown), HALion `.hsb`/`.vstsound`, IK SampleTank `.pak`, Spitfire LABS
  (plugin only), and MachFive (not a sampler; the search hits are a
  marketing company).

## 2. Ranked table per format

Rank 1 = most useful to KONTRA next. "KONTRA today" is from the worktree at
the time of the scout; `crates/format/sfz`, `crates/sampler-ikmpak`, and
`crates/sampler-uvi` exist.

| Rank | Format | Public decoder / spec found | Use for KONTRA | KONTRA today | Confidence |
| --- | --- | --- | --- | --- | --- |
| 1 | **SFZ** | sfizz (BSD-2, C++, archived 2025-03-17); ConvertWithMoss `format/sfz`; sfzformat.com spec (**search snippet**) | Reference reader and opcode behaviour; round-trip tests | `crates/format/sfz` exists | High |
| 2 | **EXS24 (Logic)** | ConvertWithMoss `format/exs/EXS24Block.java` (LGPL, block table, `GAMETSPP` tagged records); renoise-exs24 (MIT, Lua, 2025); ROMPlayer (MIT, Swift, stale 2019, claims EXS24) | Block layout and zone/sample mapping | No EXS reader found (`rg` for `exs24` in `crates/` returned no reader) | Medium-high |
| 3 | **DecentSampler** (`.dspreset`/`.dslibrary`) | Public spec at decentsamples.com (cited in ConvertWithMoss README, **not fetched**); ConvertWithMoss `format/decentsampler/`; examples repo (MIT, 60+ `.dspreset` files, **verified** listing); schwung-dspreset (MIT, C, 2026-09) | Easy win: XML plus WAV/FLAC. Use examples as test fixtures | Not present | High for the format, low for readers |
| 4 | **HISE monolith `.ch1` + HLAC** | HISE (GPLv3 plus commercial; C++; pushed 2026-10-11; 1,440 stars). Files: `hi_streaming/hi_streaming/MonolithAudioFormat.cpp`, `hi_lac/hlac/*.cpp|h`, `tools/hlac_tool/` | Only open source for `.ch1` naming, split parts, multimic suffix, and HLAC cycle/header layout | Not present | High (author source). **Clean-room only** |
| 5 | **Akai family** (S900/S1000/S3000 images, AKP/AKM, MPC) | ConvertWithMoss `format/akai/*` (many read-only); S1000/S3000 disk image readers | Future RE aid; large coverage | Not present | High for ConvertWithMoss's read paths; unverified against real discs |
| 6 | **Kontakt NKI/NKM** (comparison oracle) | ConvertWithMoss `format/ni/kontakt`, `format/ni/maschine`, `format/ni/nicontainer`; README table: Kontakt 1 to 7, NCW decode, **encrypted files not supported** (**verified** in README) | Cross-check NKI parsing; LABS may be NKI-based | KONTRA has `sampler-kontakt` and the native oracle (`docs/re/NATIVE_ORACLE.md`) | High (KONTRA is ahead) |
| 7 | **SoundFont 2** (`.sf2`) | ConvertWithMoss `file/sf2/Sf2File.java` and friends. The public Creative SF2 2.01 spec exists (**from memory, not fetched in this run**) | Reference reader for presets/zones/loops | No SF2 reader found | High |
| 8 | **UVI `.uvip`** (program XML) | oeholmen/UVIScript `Misc/Simple Synth.uvip` (**verified**: plain XML); SimonORorke/UVIScript `Programs/Tibetan Horns.uvip` (MIT, 2023; contents **not verified**) | Free UVI-written test oracles for the XML schema (`Connections`, `Mappers`, `SignalConnection`) | Parses `<UVI4>` XML (`crates/sampler-uvi/src/lib.rs`, `engine_parameters.rs`, tests) | High for "plain XML" |
| 9 | **UVI UFS2 bank** (`.ufs`) | **None found.** UVI support pages describe a container only (**search snippet**) | Nothing public. KONTRA's decoder is the reference | `ufs.rs`, `ufs_cache.rs`, `crypto.rs`, `access.rs`; **21 of 26 user banks fail** (`docs/re/FORMAT_COVERAGE.md:72`) | Owned by KONTRA |
| 10 | **UVIScript (Lua)** | UVI docs at lua.uvi.net (UI page **verified**; its constraints: widgets created in main script only, no `wait` in callbacks, use `spawn`); oeholmen scripts (no license) | Test fixtures and API surface; docs define the sandbox | `script.rs`, `script/*`, `script_prelude.lua` | High (official docs) |
| 11 | **IK Modo Drum `.pak`** | Nothing public for SampleTank. Modo payloads are plain RIFF/WAVE | Nothing to port: `docs/re/formats/ikmpak.md` already covers the three user packs | `crates/sampler-ikmpak` (read-only, complete per the doc) | KONTRA is ahead |
| 12 | **Superior Drummer `.s20`/`.obw`** (user has 18 `.s20`, 13 `.obw`, verified by census) | ConvertWithMoss has no SD reader. None found | Nothing public | `docs/re/formats/sd-obw-s20.md` | KONTRA owns it |
| 13 | **Ample `.ascf`** (user has 482 files in Ample libraries, verified by census) | **None found.** The Ample EULA prohibits reverse engineering (**search snippet**, Chinese-language EULA) | Keep RE in-house; respect the EULA question before going further | `crates/sampler-ample` exists; `FORMAT_COVERAGE.md` says "needs RE before any reader" | Low for outside help |
| 14 | **EastWest Opus `.ewi` / Play `.ewp`** | None. A forum reply says the format is "locked down" (**search snippet**). The Opus manuals forbid copying (**search snippet**) | Render the instruments to WAV (the forum's own workaround) | Not present | Low |
| 15 | **Spitfire LABS** | None on the file format. Libraries reportedly came from Kontakt (**search snippet**, Gearnews) | Test NKI path first, if the user has LABS files | Not present | Low |
| 16 | **Orchestral Tools SINE `.sine`** | None. Only VI-Control threads and a mention of SPC compression (**search snippet**) | Nothing | Not present | Low |
| 17 | **Toontrack EZdrummer** (`.ezdp3`, samples via TPC II) | Forum says `.ezdp3` is plain text listing `usersounds` paths (**search snippet**). TPC II compression is named but undocumented (**search snippet**). I did not find the user's EZdrummer folder in this pass | Project XML is cheap to read; sample decode is blocked | Not present | Low-medium |
| 18 | **HALion `.hsb` / `.vstsound`** | None. Only generic filetype pages (**search snippet**) | Nothing | Not present | Low |
| 19 | **Renoise XRNI** (not asked, but a clean reference) | Documented in ConvertWithMoss `README-FORMATS.md` (ZIP plus `Instrument.xml` plus FLAC) | Easy and open | Not present | High (format), not needed by user |

Also noted, not ranked: EXS2MPC (no license, 2026-07), tonverk-elmulti-converter
(ISC, 2026-10, EXS24 and SFZ to Tonverk), linuxsampler (GPL-2, last push 2023-08,
Gigasampler-focused), sfzen (GPL-3, SFZ manipulation, 2026-09), fleximeter/sfz
(Apache-2.0, Python SFZ parser, 2025-08).

## 3. Project details

Fields: URL, license, language, last activity (`pushedAt`, verified via REST
unless noted), formats covered, what it decodes that KONTRA lacks, and
trustworthiness.

### git-moss/ConvertWithMoss

- URL: https://github.com/git-moss/ConvertWithMoss
- License: LGPL-3.0 (verified). Language: Java (verified). Last push 2026-10-11, 459 stars (verified).
- Formats (from `documentation/README-FORMATS.md`, verified): 1010music bento and blackbox/tangerine/bitbox; Ableton Sampler; Akai AKP/AKM, MESA, MPC Modern, MPC60, MPC500/1000/2500, MPC2000/2000XL/3000, S900/S950 and S1000/S3000 disk images (many read-only); Arturia Synclavier V; Audiomodern Soundbox; Bitwig; Casio FZ; CWITEC TX16Wx; DecentSampler; discoDSP Bliss; DLS (read-only); Elektron Tonverk; E-mu Emulator I to X; Ensoniq EPS/ASR (read-only); Ensoniq Mirage; Expert Sleepers disting EX; Fairlight CMI; FL Studio DirectWave; Groove Synthesis 3rd Wave; ISLA S2400; ISO/IMG; Korg KSC/KMP/KSF and wavestate/modwave; Kurzweil K2000/K2500/K2600 and PC3/Forte; **Logic EXS24**; **Native Instruments Kontakt** (NKI/NKM, Kontakt 1 to 7, NCW decode, encrypted not supported); Maschine; Polyend Tracker; Reason NN-XT; Renoise; Roland MC-707/MC-101, MV-8000, S-10/S-50/S-770, SP-404MK2, ZEN-Core; Sequential Prophet X; SFZ; SoundFont 2; Omnisphere 3; Synclavier Regen; Synthstrom Deluge; TAL Sampler; Teenage Engineering OP-XY; Waldorf Quantum/Iridium; Yamaha YSFC.
- Useful paths (verified): `documentation/design/*.md` (one design note per format, e.g. `DIRECTWAVE_DWP_FORMAT.md`, `E4B_FORMAT.md`, `SOUNDBOX_FORMAT.md`, `SYNCLAVIER_V_FORMAT.md`, `CASIO_FZ_FORMAT.md`); `src/main/java/de/mossgrabers/convertwithmoss/format/exs/EXS24Block.java`; `.../file/sf2/Sf2File.java`; `.../format/akai/s1000/`; `.../format/ni/kontakt`.
- KONTRA lacks (per the table above): Akai AKP/S1000/MPC images, E-mu Emulator family, Kurzweil, Casio FZ, Korg KMP, Roland S-series, Renoise XRNI, DecentSampler, Bitwig, Ableton, Tonverk, EXS24, SF2 and SFZ readers (KONTRA has SFZ at `crates/format/sfz`; the others were not found in `crates/`).
- Trustworthiness: high for format knowledge. The maintainer is active and writes a design note per format. Reading support for some hardware formats is listed as read-only. I did not run its test corpus.

### christophhart/HISE

- URL: https://github.com/christophhart/HISE
- License: GPL-3.0 with a commercial licence on request (`license.txt` and the per-file headers; verified). Language: C++ (verified). Last push 2026-10-11, 1,440 stars (verified).
- Format: `.ch1` monoliths. `MonolithAudioFormat.cpp` (verified) names files with the `ch` prefix, split parts `a`–`z` (`getSplitPartFromChar`), and multimic channel suffixes (`extension << channelIndex+1`). `hi_lac/hlac/` is a lossless codec (`HlacDecoder.h`, `HlacEncoder.cpp`, `BitCompressors.cpp`, `HlacAudioFormatReader.cpp`). `tools/hlac_tool/Source/HlacTests.cpp` is a test harness. `hi_core/hi_sampler/sampler/SfzImporter.cpp` imports SFZ.
- Also (search snippet): the `.hrX` distribution format wraps HLAC in FLAC and splits it into 1 GB parts; a request to merge all `.ch1` files into one monolith was answered as "a lot of work" (the streaming engine expects one file per sample map), not implemented.
- KONTRA lacks: any HLAC decoder or monolith reader.
- Trustworthiness: primary source (the author of HISE). The header-level facts are verified; the codec internals were not read in this pass.
- License note: **GPLv3 is not compatible with KONTRA's Apache-2.0 for code copying.** Re-implement from the observed bit layout (with test vectors generated from HISE's `hlac_tool`, which is run by the user, not by us), and do not copy `HlacDecoder.cpp`.

### sfztools/sfizz

- URL: https://github.com/sfztools/sfizz
- License: BSD-2-Clause (verified). Language: C++ (verified). **Archived**, last push 2025-03-17, 529 stars (verified).
- Format: SFZ parser and synth, JACK client.
- KONTRA lacks: nothing in particular if `crates/format/sfz` is complete. Use sfizz as a behaviour oracle (opcode defaults, `off_by`, `amp_velcurve_N`).
- Trustworthiness: high for SFZ semantics. Archived means no fixes after 2025.

### DecentSampler (format spec, examples, player)

- Spec URL (cited in ConvertWithMoss README, not fetched): https://www.decentsamples.com/wp-content/uploads/2020/06/format-documentation.html
- Examples: DecentSamples/DecentSampler-Sample-Library-Examples. MIT (verified), pushed 2026-10-05, 14 stars. Verified listing: about 60 `.dspreset` files covering buttons, convolution, LFOs, MPE, XY pad, animation, and oscillators (e.g. `example-017-oscillators/Scripts/dx7_to_dspreset.py`).
- Player: legsmechanical/schwung-dspreset. MIT, C, pushed 2026-09-22, **0 stars** (verified). Trust: low to medium.
- ConvertWithMoss implements `format/decentsampler/DecentSamplerCreator.java` and a detector (verified paths).
- KONTRA lacks: a DecentSampler reader. Its XML is simple.
- Trustworthiness: the spec and examples come from the vendor. Good fixtures.

### renoise-exs24, ROMPlayer, EXS2MPC, tonverk-elmulti-converter (EXS24 cluster)

- matt-allan/renoise-exs24: MIT, Lua, pushed 2025-07-11, 8 stars. "A Renoise tool for loading EXS24 instruments" (verified). Read the Lua parser in this repo if an EXS24 parser is needed, after checking MIT terms.
- AudioKit/ROMPlayer: MIT, Swift, pushed 2019-12-29, 549 stars. Description says "EXS24, Sound Font, Wave Player" (verified in description). Its tree lists SFZ sounds; I did not find an EXS parser file in the paths I searched. Stale.
- intelliriffer/EXS2MPC: **no license**, Swift or other, pushed 2026-07-18, 7 stars. EXS to MPC converter. Do not vendor.
- asatamax/tonverk-elmulti-converter: ISC, pushed 2026-10-05, 17 stars. EXS24 and SFZ to Tonverk (description).
- Note: the WebSearch summary said Logic EXS reverse engineering was "too cumbersome" (older article). The current ConvertWithMoss README lists "Logic EXS24" as supported, so the repo is the more recent statement. Direction (read, write, or both) was not checked.

### oeholmen/UVIScript (UVI program files and scripts)

- URL: https://github.com/oeholmen/UVIScript
- License: **none** (`license: null`, verified). Lua, pushed 2026-05-17, 31 stars.
- Verified content: `Misc/Simple Synth.uvip` is plain XML starting with `<UVI4><Program Name="Program" ... Polyphony="16" ... Streaming="1">`, and includes `<Connections><SignalConnection ... Source="$Program/LFO 2" Destination="Gain" ...>` and `<Mappers><ControlSignalMapper Name="Mapper 0" Min="0" Max="48" ... DisplayName="Semitones">0,000000 0,020833 ...</ControlSignalMapper>`. Also `compiled/synths/programs/*.uvip` (Korg Minilogue, Tweak Synth family) and many `Misc/*.lua`.
- Use: oracle files for the program XML schema. **Do not vendor the scripts** (no license).
- KONTRA lacks: nothing for XML parsing (already parsed). It can improve the test corpus.
- Trustworthiness: the `.uvip` XML is verified to be plain text. The Lua scripts are community code of unknown quality.

### SimonORorke/UVIScript

- URL: https://github.com/SimonORorke/UVIScript. MIT (verified), pushed 2023-12-20, 5 stars.
- `Programs/Tibetan Horns.uvip` exists (verified). Its contents were not read in this pass (my fetch failed). Treat as unverified.

### lua.uvi.net (official UVIScript docs)

- URL: https://lua.uvi.net/_u_i_page.html (UI guide; **verified**).
- Verified facts: widgets must be created during main script execution, not in `onNote`/`onRelease`. Widget callbacks are not coroutines (`wait`, `waitBeat`, `waitForRelease` need `spawn`). Invalid widget names stop the load at that line. Documented widget and callback names: `Knob`, `ParamKnob`, `Table`, `WaveView`, `FileSelector`, `onNote`, `onRelease`, `onSave`, `onLoad`, `saveData`, `loadData`, `setKeyColour`.
- Search snippet only: sandboxed Lua 5.1 with no `os`/`io`, no third-party libraries; a "UVIScript AGENTS.md" page warns against LuaJIT and Lua 5.4 syntax.
- Use: the official source for sandbox behaviour (`script.rs`, `script/*` tests). Check these facts against the reference pages before relying on the Lua 5.1 claim, because I only read the UI page.

### ufs / encryption: what is public

- No UFS2 spec, decoder, or encryption write-up exists on GitHub or the web in this pass. Search results were filetype pages, UVI support pages, and forum threads.
- The only forum threads about "emulation files" next to banks (for example R2R emulation threads) were not used. They point at licensing bypass and are not format documentation. KONTRA should not use them.
- UVI's own support pages (search snippets) say a UFS is a container for samples, scripts, and images, and that Falcon has a "Verify soundbank integrity" option. That matches the KONTRA container work in `ufs.rs`.

## 4. What we can learn / port: concrete gaps

Gaps are listed with the KONTRA file they map to and the external file that
would help. "Port" means re-implement from observed facts with our own test
vectors. No code is copied from GPL, LGPL, or unlicensed projects.

### UVI (owned: Augmented Orchestra, VWinds, UVI banks in `/data/main/Libraries/UVI`)

1. **UFS bank coverage gap (21 of 26 banks fail).** `docs/re/FORMAT_COVERAGE.md:72`, `:127`. There is no public reference, so the work is in-house. Use `.uvip` XML examples (`oeholmen/UVIScript`) to confirm how script-event global Gain/Pan sources are written, since `FORMAT_COVERAGE.md:127` names that as the blocker.
2. **Program XML schema oracle.** `crates/sampler-uvi/src/lib.rs` (`<UVI4>` handling), `engine_parameters.rs`. Check `Connections` and `Mappers` parsing against `Misc/Simple Synth.uvip` by reading it locally. Do not commit that file (unlicensed repo). Prefer a program we author in UVI Workstation as the committed fixture.
3. **UVIScript sandbox list.** `script.rs`, `script_prelude.lua`. Confirm `os`/`io` are absent and that Lua is 5.1 by reading the lua.uvi.net reference pages (not fetched). Test that KONTRA's Lua environment rejects `wait` in `onNote`, matching the documented constraint.
4. **Encryption.** `crypto.rs` already covers the offset-seeded transform (512-byte block restart) and key derivation. No external source to compare with. Keep the existing tests; no action from outside.

### Modo Drum `.pak` and Superior Drummer (owned)

5. `.pak`: `docs/re/formats/ikmpak.md` is complete for the three user packs (verified). Nothing external to learn.
6. `.s20`/`.obw`: `docs/re/formats/sd-obw-s20.md` exists. No external decoder found in ConvertWithMoss (no Superior Drummer reader in its format list). Nothing to port.

### Ample `.ascf` (owned: 482 files)

7. No public reader or spec. `FORMAT_COVERAGE.md` names the container and codec as unknown. The Ample EULA (search snippet, table of contents only) appears to include a reverse-engineering restriction; the full text was not read. **Before more RE work on Ample, check the EULA with the user.** Nothing to port from outside.

### EXS24 (not owned; user-facing gap)

8. Port the block layout from `EXS24Block.java` (type IDs 0x00 to 0x08, `GAMETSPP` tagged records) into a clean-room reader. Test against any real `.exs` the user has; none was found in `/data/main/Libraries` in this pass.

### SFZ (`crates/format/sfz`)

9. Cross-check opcode defaults and `amp_velcurve_N` against sfizz (BSD-2). Port behaviour, not code.

### DecentSampler (not present)

10. Build a reader from the vendor spec. Use the examples repo (MIT) as the fixture set: about 60 `.dspreset` files.

### SF2 (not present)

11. Reader from the public SoundFont 2.01 spec (not fetched in this pass; check the spec text before implementing). ConvertWithMoss `Sf2File.java` is a cross-check, not a source to copy.

### HISE `.ch1` (not present; if KONTRA must load HISE content)

12. Monolith naming (`ch` + split letter + multimic index) and HLAC layout from `MonolithAudioFormat.cpp` and `hi_lac/hlac/`. License: GPLv3. Clean-room only; generate test vectors with HISE's `tools/hlac_tool` on the user's machine, then implement from the vectors.

### Kontakt NKI (comparison)

13. ConvertWithMoss `format/ni/kontakt` notes "Encrypted files are not supported". Confirm KONTRA's NKI behaviour on encrypted files against `docs/re/NATIVE_ORACLE.md`. KONTRA is ahead on Kontakt; use ConvertWithMoss only as a cross-check.

### Formats with no public path (record, don't spend time)

- EastWest Opus `.ewi`/`.ewp`: render to WAV in a DAW (the forum's workaround). The license forbids copying.
- Orchestral Tools SINE: nothing public. Compression (SPC) is undocumented.
- Toontrack EZdrummer: `.ezdp3` project XML/text is cheap to read later. Sample decode needs TPC II, which is undocumented. I did not find the user's EZdrummer folder in this pass, so this is unverified for the user's installs.
- HALion `.hsb`/`.vstsound`: nothing public.
- IK SampleTank `.pak`: nothing public. The user's Modo `.pak` files are a different, already-decoded format.
- Spitfire LABS: if the user has LABS libraries, test the NKI path first. Not verified.

## 5. License notes

| Project | License | Can KONTRA (Apache-2.0) use it? |
| --- | --- | --- |
| ConvertWithMoss | LGPL-3.0 | Read it; re-implement facts. Do not copy source files into KONTRA without a license review. |
| HISE (incl. HLAC, monolith) | GPL-3.0 (+ commercial) | **No code copy.** Clean-room from observed behaviour and vectors. |
| sfizz | BSD-2-Clause | Yes, with the BSD notice. It is archived, so prefer behaviour porting. |
| DecentSamples examples | MIT | Yes, as test fixtures with the MIT notice. |
| schwung-dspreset | MIT | Read; copy only with notice. |
| ROMPlayer | MIT | Read; copy only with notice. Stale. |
| renoise-exs24 | MIT | Read; copy only with notice. |
| tonverk-elmulti-converter | ISC | Read; copy only with notice. |
| oeholmen/UVIScript | **none** (all rights reserved by default) | **Do not vendor** the Lua or `.uvip` files. Use only as a local oracle, not committed. |
| SimonORorke/UVIScript | MIT | Yes with notice. Contents of `Tibetan Horns.uvip` not checked. |
| EXS2MPC | **none** | **Do not vendor.** |
| lua.uvi.net docs | UVI copyright | Use for facts; cite the page. Do not copy text. |
| Ample EULA | Proprietary, restricts RE (search snippet) | Flag before RE work. |
| EastWest Opus manuals | Proprietary, no copying (search snippet) | Do not copy. |

## 6. Next steps (for the coordinator; none done here)

1. Decide whether the UFS coverage gap (21 of 26 banks) is the priority. There is no external help, so it is in-house work.
2. Ask the user whether they have Spitfire LABS or EZdrummer files, to test the NKI and EZdrummer paths. The EZdrummer folder was not found in this pass.
3. Pull the DecentSampler spec and SF2 spec text (not fetched here) before scheduling those readers.
4. Re-run GitHub search after its rate limit resets for `vstsound`, `hsb`, `sine`, and `tpc` terms. Those searches returned nothing in this pass, and the search quota was consumed by the earlier repo queries.
