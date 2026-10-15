# KONTRA public site

Static HTML/CSS/JavaScript, published to GitHub Pages by `.github/workflows/pages.yml` on **every push to main**. The public pages need no browser API token. Publishing the local-file inspector also requires the Rust browser target and Emscripten 6.0.12.

Build and preview from the checked-out revision (choose an output directory on the dev drive):

```sh
python3 website/build_site.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
# Activate emsdk 6.0.12 and install the wasm32-unknown-emscripten Rust target first.
python3 website/build_inspector.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
python3 -m http.server 4173 --directory /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

The build copies public assets and renders `features.html` with the complete compatibility table and embedded data. JavaScript adds search, sampler/category/status filters, semantic status sorting and 25-row pagination. Without JavaScript, the complete table and source links remain available. Filtered views can be linked with `?sampler=Kontakt` or `?sampler=UVI`, plus `category`, `status` and `q`. The same data is published as `compatibility.json`. The old `/tracker/` URL redirects to the table.

## Compatibility data

`build_site.py` uses `tools/coverage/tracker.py` to read committed engine probes, modulation targets, the KSP parity ledger, format catalogs, UVI feature records and verification records. Public builds set `KONTRA_COVERAGE_SOURCE_ONLY=1`: private local catalogs cannot refresh or replace committed snapshots. The source commit and commit date identify each publication; row source links are pinned to that exact commit.

- **Implemented:** implemented and verified according to the tracker.
- **Partial:** incomplete, approximate, unverified, stale probes or a known mismatch. The row retains its notes and verification state.
- **Not implemented:** recorded as unsupported or absent.

A commit alone does not prove feature completion. Code probes and current source are re-evaluated each build; coverage and verification records still need updating when implementation changes. The table tracks features, not whole-library compatibility, and does not represent a test run against every library. It describes main, which can be ahead of the latest downloadable release.

`ROADMAP.md` and `roadmap.html` record the current sampler priorities: Kontakt and UVI/Falcon in parallel; Opus/EastWest, HALion, HISE and other sampler frontends next. Keep those two files synchronized. Historical architecture checkpoints do not override these priorities.

## Validation

```sh
python3 website/check.py
python3 website/test_build_site.py
node website/test-compatibility.cjs
node website/test-releases.cjs
python3 website/check.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

Only public HTML, CSS, JavaScript, generated data and assets are deployed. Design context, checks and build scripts stay out of the artifact. Font and icon provenance/license files remain alongside assets.

The homepage queries GitHub's latest published release for its version/date and highlights the detected desktop platform. Direct downloads and release-history links work without JavaScript. API failure leaves the release-page link available. Update download asset names if release packaging changes.

## Browser library inspector

`inspector.html` accepts an instrument selection or a library folder. Files are mounted through Emscripten's read-only WORKERFS in a fresh worker for each inspection; payloads are read on demand rather than copying a whole UFS bank into memory. Only selected files are available. Files/reports are not uploaded or persisted. Stop terminates the worker; Clear files also releases the page's file references and report state.

`tools/corpus-health/src/browser.rs` calls the same `inspect.rs` implementation as the native inspector. `build_site.py` publishes the existing `tools/library-inspector.html` report viewer, including its diagnostics, script lowering, converted structure, UI records and timing views. Browser reports additionally show explicit pixel-layout rectangles from the exported UI model. They do not render artwork, stock sizing or grid placement; native CPU witnesses remain a separate renderer. Audio, live musical callbacks and native equivalence remain explicitly untested. Individual successful stages are never labeled whole-library compatibility.

UFS input first reads the program directory; choose a bank program for inspection. NKM inputs use a one-based program choice in the page and the inspector's zero-based argument internally. NKSN inputs can resolve their base from selected files or accept an explicit selected NKI. Resource/module behavior follows the shared inspector; making files available does not implement an unsupported resource path.

`build_inspector.py` compiles the shared Rust implementation to `wasm32-unknown-emscripten`, with native JIT disabled only for the browser target. It writes the JavaScript factory, WebAssembly and exact source revision. The worker loads the engine on demand using that revision in asset URLs. The Pages workflow rebuilds and tests it on every main commit and validates relevant PRs, so inspector and report-viewer changes reach the site together. Existing runtime notices and source bundles accompany the distribution.

Run the actual WebAssembly fixture checks with:

```sh
node website/test-inspector.mjs /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
```

Use [Emscripten's installation instructions](https://emscripten.org/docs/getting_started/downloads.html) for the SDK. The pinned SDK tag resolves to `35ff8a6d150541276abbc6bae512ca90bcfbe220`. The normal site build can be previewed before the engine build; Inspect then reports that the engine is unavailable.
