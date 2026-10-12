# Sampling profile ownership

SamplingProfile is an immutable prepared-bank source choice. Compatibility is
Rust Default and the default for authored `.kontra-instrument` JSON that omits
the field. New authored JSON stores the resolved profile explicitly; an explicit
Kontakt field round-trips. Unknown legacy JSON fields do not opt into Kontakt.
Actual NI parsing selects Kontakt independently of opaque, malformed, or missing
private MIDI/controller options. Native program amplification and controller
policy retain their existing owners and admission rules.

The prior coupling was a proxy: Builder::controllers recognizes only a typed
private MIDI prefix, then Context.native_sampler used its ControllerPolicy.
Those options describe sustain, standard volume/pan/note modes and CC7 maximum;
they do not identify sampler interpolation. The bounded sampler evidence remains
in COUNTED_LOOPS.md: native quality dispatch 0x140968ca0 reads group+d8, saved
source modes 0/3 select runtime modes 0/4, and both select Sampler via registry
0x140d208f0. This change does not claim unproved kernels for other modes/qualities.

NI cache stamps now begin `KONTRA instrument sampling-profile 1 ` and cache
metadata requires the explicit field. Previous cache epochs miss and reimport;
there is no decoder-policy migration at render time. Sampling preparation, DFD
quarantine, physical support/crop preparation and prepared-quality admission use
the same profile as Context. Residency fills additionally require equal profiles.
Changing profile closes the prior instrument lifetime before adopting new source
arithmetic, even if source mode, quality and sample headers are unchanged.

Supplied samples use Compatibility through Bank::from_samples. Callers that
intend the bounded native source path use Bank::from_samples_with_profile; a
Native controller fixture alone no longer selects a kernel. Preparation is the
same worker-owned physical-source pass as file-backed banks. No callback profile
parsing, allocation, additional source registry or controller-state cloning was
introduced.

Staged checks (execution belongs to the central build owner):

* `saved_sampling_profile_migration_and_cold_ni_cache_select_independent_pcm`
  writes real legacy/explicit JSON and an NKI with invalid MIDI option prefix,
  verifies old cache miss/reimport, then compares complete fractional stereo PCM
  against independently evaluated polynomial samples at unity rate, fixed and
  irregular pieces, with zero callback heap operations.
* `native_interpolation_residency_fill_preserves_pcm_but_new_source_identity_closes_old_owners`
  includes a profile-only replacement with exact old Host END256 before new
  onset, a fresh-profile PCM reference, and retained retired banks for disposal
  outside the callback guard. Existing selected-kernel/HQ/prefetch fixtures now
  request their intended profile explicitly.

## Source identity and saved quality admission

Kontakt bank preparation and off-thread zone remapping now share one source
predicate: ordinary Sampler0/DFD3 requires no decoded wavetable and quality0/1/2;
mode9 requires a decoded wavetable, followed by the existing supported-state,
complete-cycle and residency checks. Other modes, missing identities and
mode/payload mismatches are quarantined per zone before playback-map preparation.
Groups and exact source-mode/quality metadata remain retained, with a bounded
UnsupportedSource diagnostic naming the mode, table presence, quality and zone.
Compatibility retains its existing authored source behavior. Header resolution
still precedes this boundary, but excluded zones add no PCM decode spans to the
existing loader plan.

The NI decoder preserves arbitrary source mode words for known unstructured
source versions. The importer only decodes wavetable parameters from v0x106
mode9; a v0x104 mode9 therefore retains mode9 without a playable table owner.
Neither absent nor unsupported source metadata may select ordinary Hermite in
Kontakt playback. Existing remap restrictions remain narrower for DFD3.

Saved quality closure (Kontakt pinned binary): Group reader0x140d0cf80 routes
the final signed field through0x140cf0360. Branch0x140cf0369 accepts only0/1/2
(stores at0x140cf0394/388/37c); every other value reaches the exception at
0x140cf03a0 and throw0x140cf03b6. XML0x140920cd2 likewise maps only
normal/high/ultra. Global override3 at0x140968ea6 means inherit Group+d8,
not saved quality3. Effective ordinary Kontakt settings therefore reject quality
edits outside0/1/2 before publication; Compatibility remains unchanged.

`saved_unsupported_kontakt_sources_retain_identity_cache_and_only_supported_sibling_pcm`
stages a real saved-NKI mutation containing mode5, missing mode9 table, opaque
v0x105 identity and saved quality3, then verifies raw/cache retention, per-zone
quarantine and complete supported-sibling PCM through loaded, bare and supplied
banks. It also covers prepared invalid quality and remap payload mismatches.
The existing residency identity proof uses supported DFD3 as its replacement,
rather than an absent Kontakt identity. This is an admission/retention proof,
not a claim of unsupported source algorithm parity. Execution belongs to the
central build owner.
