# kontra-law

Parameter laws: curves that map a normalized knob position `x` in `0..=1` to a
parameter value (Hz, dB, ms, ...). A law is applied once when a value arrives, never
inside the sample loop, and `Law::eval` never allocates or panics.

## Shapes

| `Law` variant | Schema `kind` | Fields | Output |
|---|---|---|---|
| `Linear` | `linear` | `lo`, `hi` | straight line from `lo` to `hi` |
| `Exp` | `exp` | `lo`, `hi` (both above zero) | geometric interpolation (frequency tapers) |
| `Db` | `db` | `lo`, `hi` (dB) | linear gain; the dB value is linear in `x` |
| `Table` | `table` | `values` (at least two) | evenly spaced over `0..=1`, linearly interpolated |
| `Breakpoints` | `breakpoints` | `points` = `[[x, y, curve], ...]` | `x` strictly increasing; `curve` bends the segment that starts at the point, `0` is straight |
| `Formula` | (not in TOML yet) | `FormulaId` | a named closed form; none are registered |

`eval` clamps `x` to `0..=1`. Breakpoint curves are clamped to ±16. Table and linear
endpoints are exact.

## Law file schema

One law per TOML file. `tolerance_db` is the error, in dB, that a measured response may
show against this law and still count as a match. Unknown keys are rejected.

```toml
tolerance_db = 0.1

[law]
kind = "exp"
lo = 20.0
hi = 20000.0
```

```toml
tolerance_db = 0.5

[law]
kind = "breakpoints"
points = [[0.0, 0.0, 0.0], [0.5, 0.2, 2.0], [1.0, 1.0, 0.0]]
```

## Embedding

`laws!` (macro_rules, not a proc macro) embeds an explicit list of files as `pub const`
[`LawSource`](src/file.rs) values and generates one test that parses every listed file.
Paths are relative to the file that invokes the macro. There is no directory globbing.

```rust
kontra_law::laws!(
    SOLID_BUS_COMP => "laws/kontakt/solid_bus_comp.toml",
    FILTER_CUTOFF => "laws/kontakt/filter_cutoff.toml",
);
```

Parse at prepare time with `SOLID_BUS_COMP.parse()`, which returns a `LawSpec`
(`law` and `tolerance_db`).
