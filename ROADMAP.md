# KONTRA roadmap

## Current work — in parallel

| Sampler | Stage | Implementation |
| --- | --- | --- |
| Kontakt | In development | [Feature status](https://derpcatmusic.github.io/KONTRA/features.html?sampler=Kontakt) · [Source](crates/sampler-kontakt) |
| UVI / Falcon | In development | [Feature status](https://derpcatmusic.github.io/KONTRA/features.html?sampler=UVI) · [Source](crates/sampler-uvi) |

Playback, scripting, effects, modulation and authored instrument interfaces are active implementation areas for both samplers.

## Next samplers

| Sampler | Stage |
| --- | --- |
| Opus / EastWest | Planned |
| HALion | Planned |
| HISE | Planned |
| Other samplers | Planned |

No fixed dates or order within the next-samplers group are announced.

## Live implementation table

The [compatibility table](https://derpcatmusic.github.io/KONTRA/features.html) is rebuilt on every commit to main from the repository's code probes, implementation ledgers and verification records. It can be sorted and filtered by sampler, category and status. Each published table identifies its source revision and links to code at that revision.

Statuses describe individual features, not universal library compatibility. Commit messages alone do not establish implementation or verification. Updating the code, probes or tracking records changes the generated table; see [the site build](website/README.md) and [tracking sources](tools/coverage).

Detailed engineering references remain in [architecture-v2](docs/architecture-v2), [Kontakt/KSP parity](docs/architecture-v2/KSP_PARITY.md), and the [Falcon runtime groundwork](docs/FALCON_RUNTIME_UI_GROUNDWORK.md). Their historical checkpoints do not set the current sampler priorities above.
