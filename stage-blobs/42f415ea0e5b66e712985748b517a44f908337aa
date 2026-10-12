# Missing insert-bypass default

Retained authored Workstation 4.0.9 loaded-program getter observations return
Boolean `false` for omitted `BypassInsertFX` on Program, Layer and Keygroup,
both before initialization and during initialization. SamplePlayer returns nil
in those observations. No new native execution was performed for this change.

The correction belongs to `host::source_parameters`, shared by Lua and
saved-state validation. It supplies `false` only when the attribute is absent;
explicit XML values and types retain precedence. SamplePlayer and synthetic
Part/Synth receive no new property. No initial command or saved delta is created.
The renderer already treats omission as false, so this changes host property
availability without changing renderer admission or its bypass algorithm.

Both cached original and V2 Clarinet inventories already serialize this
attribute on every Program, Layer and Keygroup: 183 nodes in each bank. This
is an authored missing-property correction, not evidence of a newly fixed
failure in either bank or of complete native bypass fidelity.

The original graph fingerprint is unchanged. Existing states retain their
identity and values; new omitted-property overrides cannot be restored by older
implementations lacking this property. Prepared functional coverage checks
getter types, explicit values, unsupported targets, wrong-type command rejection,
default-free saves and Boolean delta validation/preload/recapture. These checks
are uncompiled and unrun under the CPU restriction. Combined integration,
actual bank callbacks and PCM remain unverified; installed binaries are unchanged.
