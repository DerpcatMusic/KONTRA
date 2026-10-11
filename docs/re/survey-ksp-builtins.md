# Survey KSP builtin status and native evidence

NI KSP engine-parameter commands document `output_channel_name` as a zero-based
lookup of a configured output's name. Same-SHA Oct-7 FUN_14078d910 case 8 and
radare2 0x14078dbb7 confirm an unsigned index range 0..127, output stride
0x52a0, availability word +0x17d8 and name getter FUN_140883290. Unavailable
outputs return empty text. The initializer's generated Output N labels are
not proof of native configured-name behavior; live lowering remains open.
The host's routing::port_names supplies real names, but those names are not
part of KSP Environment, and changing live routing names is not a script
text-table update. Wiring a snapshot alone would still leave stale callbacks.

NI KSP load/save get_folder documents library/User Content, factory DATA,
patch and resource-container folders. Cache FUN_14078d910 case 9 reads host
path storage; radare2 0x14078e0ac confirms a path pointer and
0x14078e216 the shared string formatter. Callback lowering now uses the
host-supplied Environment.folders, with bounded UTF-8-safe text constants
or indexed tables. Unknown selectors yield empty text. Folder provisioning
is separate: load::script_folders currently infers library/patch directories;
it does not provision factory DATA or resource-container directories, or
the unencoded User Content fallback. Those remain open, as does RC support.

NI KSP general commands document set_rpn/set_nrpn as 14-bit address/value
messages for inter-script communication. Radare2 0x14079b8b0 includes the
14-bit validation and calls FUN_140955b20 at 0x14079b902. Runtime lowering
already emits SendParameter; its simplified routing/timing policy remains
marked Approximate in tests/rpn.rs. This inspection did not establish the
native listener scheduling or earlier/later-slot behavior needed to remove
that status.

The Conflux NKM MIDI Player writes global key/scale during its note/control
logic and subscribes to their asynchronous changes during init. The current
Environment/runtime have no rack-wide global key/scale service or subscriber
callback delivery. No native setter/subscriber implementation was identified
by the available cache/string cross-reference searches. Implementing only
stored integers would omit those observable notifications. These calls and
subscribe_async remain reported; no warning was hidden.

Docs:
- <https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/engine-parameter-commands>
- <https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/load-save-commands>
- <https://docs.native-instruments.com/ni-tech-manuals/ksp-manual/en/general-commands>

Binary SHA256: 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8.
