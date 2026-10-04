# KONTRA renderer cache ownership

Pinned source: Matari-Audio/MUI `86c9dece53bb2e2538332d1169bbbe10cbd3496e`, `crates/mui-vello`. The standalone manifest replaces workspace inheritance with those same dependency versions.

`Cache::tick` sweeps image entries whose original application pixel buffers have been released, including frames that draw no bitmap. Previously sweeping occurred only while drawing another image, so switching to a vector-only view could indefinitely retain premultiplied CPU pixmaps or copied GPU upload blobs. Retained GPU encodes also sweep dead owners on partial damage without aging still-owned images. Fonts retain the existing bounded age policy.

This does not reset the GPU atlas: partial damage can omit images that remain visible, and the existing persistent-atlas fix must be preserved. Atlas allocation capacity and renderer resource pools are separate from released CPU image payloads.
