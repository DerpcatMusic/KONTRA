# Changelog

Human-reviewed changes belong here before release. Dates and stable release headings
are added when a release is actually published; commit history is not a changelog.

## 0.2.0 — unreleased

### Added

- One package version and build identity across CLI, standalone, plugin metadata,
  About, diagnostics and package manifests. Identity includes the actual full Git
  revision, optional export source revision, UTC timestamp, target, profile and features.
- Deliberate SemVer release preparation and reproducible nightly prereleases, with
  focused local checks and documented contributor workflow.
- Structured support diagnostics and an application log view/export path.
- Script condition inheritance between successfully initialized slots, native
  pedal/release conditions, and additional supported script syntax and zone queries.

### Fixed

- Physical note ownership, delayed callback cancellation, MIDI stop ordering and
  selective sound-off across articulation channels sharing one engine channel.
- Sustain release bookkeeping, generated-note lifecycle and MPE expression/tuning
  routing in the covered playback paths.
- Zone ID mapping after import filtering and source-parser allocations in validated
  preset/container paths.
- Browser scaling/layout and selected DSP effect processing paths.

### Compatibility

Kontakt preset import, scripts and playback remain partial. Successful import or a
passing synthetic test does not establish sonic parity for every library. Existing
compatibility notes and unsupported-operation diagnostics remain applicable; these
changes do not announce complete format, script or sound equivalence.
