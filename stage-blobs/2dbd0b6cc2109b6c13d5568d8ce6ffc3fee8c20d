# Retained auxiliary-resource frontier

Static review of retained Piccolo and Contrabassoon directory evidence found
only the currently handled member modes 0, 1 and 2. Piccolo's retained final
metadata-record offset and physical size leave 277 bytes after its record prefix.
That matches the current layout's 277 meaningful field bytes without its further
12 opaque footer bytes. This is a layout-match inference; the declared record
length was not re-read. Its expected container size also exceeds physical size by 12.
The selected program's data range is present. Existing parser tail handling
accepts this case with a warning. Making expected size equality mandatory would
reject this already decoded metadata without establishing a real missing payload.

The retained directories also contain 76 Piccolo and 66 Contrabassoon protected
`.vhfpreset` members. Each bank also has one protected ZIP-named record and five
protected TTF-named records; only Piccolo's ZIP path is retained as
`artwork/fonts/Archive.zip`. Standalone font loading exists. No
`.vhfpreset` schema or generic font-ZIP dispatch has been
established in the current source. The single-entry verified ZIP parser used
for program decoding must not be assumed to decode arbitrary font archives.
These 142 members are auxiliary-format evidence, not 142 additional complete
programs or confirmed runtime failures. Filename extensions do not establish
the corresponding payload schemas.

Piccolo's retained artwork/font `Archive.zip` record is 137,472 bytes, within a
256 KiB inspection bound. Its private access record exists. The installed
`bb60218` CLI source implements raw bank-member decryption for non-UVIP selections,
but first opens and decodes the complete directory; it has no cached-record-offset
extraction entrypoint. This review does not establish archive extraction success.
Individual `.vhfpreset` record identities and sizes are not retained in the
reviewed receipts. No extraction or rediscovery ran under the CPU restriction.

Exact inner schemas, archive entries, native dispatch and actual authored host
requests remain unverified. This review adds no decoder, admission or playable
coverage. It used historical directory evidence and current static source only;
installed binaries, private access records and settings remain unchanged.
