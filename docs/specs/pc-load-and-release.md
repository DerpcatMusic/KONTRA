# KONTRA plugin load and release reliability

## Acceptance criteria

- The public bundle jobs use the Moose Git commit selected by KONTRA’s lockfile and fail before building if that commit is absent. A bundle build that modifies the lockfile fails before publishing.
- Windows checks the x64 bundle layout, detects redistributable CRT imports, and loads the installed binaries and resolves their entry points. Linux reports direct ELF dependencies and enforces the existing glibc 2.35 ceiling. macOS validates that Audio Unit discovery and initialization succeed after installing the signed universal package.
- An open KONTRA editor refreshes its published model and observes folder completion while the DAW is idle. Checking folder readiness preserves its result, and closing the editor cancels its Linux portal request.
- Plugin tests, scoped Clippy, and release-script tests exercise the changes. Native checks run in the bundle workflows.

## Scope

The work covers the KONTRA shell and public workflows. Dependency pins and framework implementation remain owned by their respective lanes. Full DAW qualification requires native test hosts.

## Load and editor audit

### Windows x64

- The native Rust and VST3 shim build already use `+crt-static` through cargo-moose, including at the public Moose revision selected by the current KONTRA lockfile. A plugin built with a different tool or the wrong flags can import `VCRUNTIME140*.dll` or `MSVCP140*.dll` and be rejected by the scanner before the editor opens. Both Windows binaries in public KONTRA v0.6.20 were x64 PE images with no MSVC redistributable in their direct import tables. The bundle jobs now check both normal and delay imports and load each binary with the native loader's default search flags. The runner covers its own system DLLs. Older Windows versions and a 32-bit DAW can still reject the package.
- CLAP installs as one file. VST3 needs `KONTRA.vst3/Contents/x86_64-win/KONTRA.vst3` inside the bundle. The installer now rejects a package missing the inner binary before it touches an existing installation. Per-user paths are under `%LOCALAPPDATA%\Programs\Common`. DAWs that only scan system-wide locations need the appropriate CLAP or VST3 folder under `%CommonProgramFiles%` registered or populated.
- Wave 1B rejects WARP and other CPU adapters before requesting a wgpu device. The x64 MSVC GPU path defaults to static DXC. GPU startup failures that return to Rust can fall back to native CPU presentation. Launching the DAW with `MUI_FORCE_CPU=1` skips GPU initialization. A native access violation or hang during DX12 or Vulkan discovery, device creation, shader compilation, or present can still take down the DAW before fallback runs.
- Windows still uses rfd for folder selection with Win32 owner tracking. A host can bypass window modality and unload while the worker is running. Wave 1 window-image pinning keeps a successfully opened editor's code mapped and reduces the unload risk. rfd provides no cancel handle. Native dialog cancellation, owner lifetime, and host shutdown still need testing together.

### Linux x64

- The public workflow builds on Ubuntu 22.04 and enforces GLIBC 2.35. Both v0.6.20 binaries require GLIBC 2.35 and loaded with their expected entry-point exports. Their direct ELF dependencies were `libstdc++.so.6`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6` and the x86-64 ELF interpreter. Wave 1 worktree debug artifacts also directly linked `libxkbcommon.so.0`. A system without the required glibc, libstdc++ symbol versions or linked libraries can reject the binary before any CPU fallback or editor logic runs.
- The observed images did not directly import Vulkan, X11, Wayland, ALSA or JACK. X11, XCB and Vulkan still need their appropriate loader libraries at runtime when selected. Wayland needs a reachable compositor and compatible native embedding or floating-window support from the host. A loader probe on the build machine cannot qualify those paths or guarantee an older distro has the right symbol versions. The bundle workflow now installs Wayland headers explicitly and records the actual `DT_NEEDED` entries.
- Wave 1A and 1B remove late `XInitThreads()` from plugin startup, check X11 reachability for CLAP, use level-triggered XCB with a full event drain, and keep scale and Wayland teardown scoped to the owning editor/thread. A DAW with an unsupported embedding API or an unavailable display can still fail to show the editor. Lavapipe and llvmpipe are treated as CPU adapters in production, so MUI can use its own software renderer. A driver that crashes inside discovery still requires the CPU override or a stack-level mitigation.
- Folder selection needs a session D-Bus and an XDG desktop portal with a FileChooser backend supporting folder requests. X11 supplies the child XID. KONTRA currently sends no exported Wayland parent handle, so the picker may open unparented on Wayland. The editor reports portal errors, and users can drag a folder into the editor when the picker is unavailable. The shell now cancels the portal job on editor close and preserves completion results for the background tick.

### macOS

- AUv2 shim ABI v14 (`0x4D41750E`) must match the Rust component. A tool/runtime mismatch can generate an installable bundle whose factory initialization fails. The scripts now derive the full Git commit from the standalone lock, check that Cargo accepts that lock, and keep the post-build `git diff --exit-code -- Cargo.lock`. A source-less monorepo lock has no suitable revision and the build now stops before installing the bundler.
- The universal installer requires both slices from the same source commit with matching bundle identities. The composition scripts verify both architectures, sign all three bundles and the package, wait for Apple acceptance, staple and validate the installer ticket, and check installed signatures. Signing credentials, the notarization service, matching slices, and the supported macOS deployment target remain release dependencies. The release now runs `auval` for `aumu/Knt3/Mtri` after installation. On the ARM runner that exercises native ARM AU discovery and initialization. Intel DAW instantiation still needs an Intel host. The compatibility ZIP contains signed universal bundles. The PKG carries the stapled installer ticket.
- rfd's macOS picker is dispatched to the main thread. `mui-native-dialog` provides the Linux portal API only. Substituting it for rfd on macOS or Windows requires a native service implementation.

### KONTRA shell and formats

- CLAP, VST3 and AU have Moose feature and export wiring. The public matrix packages CLAP/VST3 on all three platforms and AUv2 on macOS. The `aax` feature delegates to Moose. An AAX release requires the Avid SDK, an appropriate signed bundle and Pro Tools testing. There is no AAX artifact in this pipeline.
- The shell uses `MuiEditor` with per-instance scale, gestures, native cancellation and parent hooks. The first snapshot refresh now runs in Moose's background task pool so a busy rack cannot stall editor creation. The wheel policy falls back to its default and an explicit resize rejection restores the previous displayed percentage. Format adapters still own host-thread dispatch and delivery of resize requests.
- Moose's application `changed` hook runs when MUI ticks. Wave 1B can settle at 0 Hz, so a KONTRA editor could stop seeing background results, meter updates and pictures while idle. The shell now requests a deadline at 250 ms when idle and 50 ms during scans or loads. This keeps changes visible with an idle DAW. A public Moose wake hook for external model changes would let KONTRA return to fully push-driven idle.

## Public rollout and remaining framework work

1. Land Matari PR #121. Its branch contains Wave 1A and 1B, with the local `moose-gui`, `moose-egui`, `mui-truce`, winit and wgpu 29 paths retired. Pinning, high-DPI fitting, CPU adapter rejection, per-editor scale and push-driven window pacing are upstream improvements.
2. Confirm Copybara's MUI export and then its dependent Moose export reached public `main`. The existing public KONTRA lock was observed at Moose `2dfe232eda11156f932341b52eb16165cf64ed00` and MUI `c91adff71c77b87dfd8cac234ac523aec80fdaf7`, before PR #121.
3. Confirm the separate KONTRA mirror to `DerpcatMusic/KONTRA` has run after both exports and committed a standalone lock with the new public revisions. That workflow strips the monorepo patch block, invokes `tools/mirror-locks.sh` from the standalone checkout, and runs locked Cargo metadata. It can also run independently after a KONTRA merge, so check the Git commits in the artifact-producing workflow even if the mirror previously succeeded.
4. **Wave 2A** owns CPU/GPU presentation and teardown, replacing softbuffer, Win32 `GetDC` presentation and the macOS `CGImage` transport. The planned per-driver crash sentinel and explicit software controls address failures that happen before automatic fallback.
5. **Wave 2B** owns Linux's lower glibc ceiling (2.31), a direct ELF dependency allowlist, symbol visibility and two-instance X11/Wayland and broken-GPU tests. Add native stress coverage and build-time shader validation there.
6. **Wave 3A** owns replacing wgpu 30 with the thin native HAL and qualifying GPU/CPU pixel parity. Driver faults and host-specific behavior still need native regression tests.

Sources in this checkout include `docs/STACK-OWNERSHIP-PLAN.md`, `frameworks/mui/docs/platform-notes.md`, the `moose-mui` editor and `mui-baseview` native worker, `cargo-moose/src/util/build.rs`, and the top-level `.github/workflows/mirror.yml`.
