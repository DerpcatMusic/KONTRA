# KONTRA public site

Static HTML/CSS/JavaScript, published to GitHub Pages by `.github/workflows/pages.yml` on **every push to main**. No package installation or browser API token is required.

Build and preview from the checked-out revision (choose an output directory on the dev drive):

```sh
python3 website/build_site.py /mnt/Windows11/DEV_WORKSPACE/RunScratch/kontra-site
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
