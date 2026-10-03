# Linux editor startup diagnostics

The native editor writes bounded startup markers to its session journal and
stderr. They distinguish window creation, showing, the first frame callback,
and the first tick's decision. They do not change the rendering backend or
surface configuration. Ordinary frames do not repeat these markers.

| Last completed boundary | What the log establishes | Next evidence |
| --- | --- | --- |
| `native window ready; waiting for first frame` | The editor handler was constructed. Baseview's event-loop initialization and showing can still be pending. | Look for creation completion; if absent, inspect the native window thread rather than GPU selection. |
| `native window creation complete; requesting show` | Native creation returned; synchronous showing was requested. | Look for show completion and inspect the child window's Map State. |
| `native window show complete` | The show request returned successfully. | If no callback arrives, inspect child and ancestor visibility and native event-loop delivery. |
| `first on_frame entered; borrowing handler` | Baseview called the adapter, before borrowing its handler or locking the model. | Look for `handler already borrowed` or the first-tick marker. |
| `first tick entering` | The handler reached its frame function, with the recorded physical size and scale. | The next marker identifies an unavailable handle, hidden window, zero size, pending retry, or GPU initialization. |
| `GPU init requested` | The renderer entered GPU initialization. | Subsequent markers distinguish instance/surface creation from adapter/device/renderer creation. |

Callbacks and show completion run on different threads, so nearby markers can
interleave. A successful show request does not establish that every ancestor is
mapped. A journal excerpt ending immediately after startup does not establish a
long-running stall.

For a diagnostic artifact, include these changes in one normal Linux build and
retain its `build-info.json` and artifact digest. Do not rebuild once for each
backend hypothesis. In a fresh session, reproduce with the graphics environment
unchanged, leave the editor open for at least ten seconds, close it once, and
retain the complete session journal and stderr. Record whether the incident was
standalone, CLAP, or VST3 and the host version. Review logs before sharing them;
library paths and unrelated private data are not needed for this diagnosis.

Use `xwininfo -id <outer-window-id> -tree` to find the current MUI child, then
`xwininfo -id <child-window-id>` and the corresponding parent/ancestor IDs to
record geometry and Map State. Window IDs change between sessions. A tree listing
proves a child exists; the separate attributes establish whether it is viewable.
Checking only the outer frame is insufficient.

`KONTRA_NATIVE_UI_TIMING=1` captures the next primary-button drag. It is not a
startup callback counter, and an editor that never receives a drag does not emit
a timing report.

The locked MUI revision uses wgpu's default surface configuration. In wgpu
30.0.1, `CompositeAlphaMode::Auto` selects supported `Opaque`, then `Inherit`;
it does not select `PreMultiplied`. An explicit selection in that same order
does not change this behavior. Surface capabilities belong to the actual
surface/adapter pair, so a GPU model alone does not establish the offered modes.
See [wgpu alpha modes](https://docs.rs/wgpu/30.0.1/wgpu/enum.CompositeAlphaMode.html)
and [surface capabilities](https://docs.rs/wgpu/30.0.1/wgpu/struct.SurfaceCapabilities.html).
An alpha-mode change cannot explain a stop before the first GPU-init marker.
