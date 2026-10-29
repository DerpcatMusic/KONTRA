# KONTRA

> **One gold-standard engine. Every format is a translation into one IR.
> Faithful where it is audible, modern everywhere else. Then go beyond what
> the originals can do.**

Before designing or changing anything in v3, read
[docs/architecture-v3/VISION.md](docs/architecture-v3/VISION.md):
- why KONTRA exists (one universal sampler that plays Kontakt, UVI, SFZ and
  others correctly, then better);
- the pipeline (format → translator laws → neutral IR → one engine → plugin);
- what every design must keep possible (per-note expression, per-voice
  modulation, sample accuracy, efficiency);
- the five-question design test every PR answers.

How to build: [docs/architecture-v3/README.md](docs/architecture-v3/README.md)
(the ten rules) and [docs/ENGINE.md](docs/ENGINE.md) (one engine, KISS and
YAGNI). How to work on a task: [12-WORKFLOW.md](docs/architecture-v3/12-WORKFLOW.md)
and the `v3-task` skill.
