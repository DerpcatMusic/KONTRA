# mui-vello patch provenance

Vendored from MUI `dcf0796082feec053af1418e3a38a302ee61da0a`,
`crates/mui-vello`, with its MIT license. Cargo workspace dependencies are
made explicit; the application Cargo.lock retains the same MUI revision.
The sibling vello dependency uses KONTRA's existing patched renderer.

## Partial vector repaint

`src/effects/retained.rs::GpuRenderer::encode` passes the damage-local
transform to `one`, as it already does for the surrounding canvas.
`vector_paint` explicitly installs this transform, so the previous
full-window transform omitted the damage origin and clipped the vector
out of a partial tile. Normal text and bitmap paints already used the
canvas transform. Full frame encoding keeps the same transform.

The failing-first `gpu_damage` example renders nonzero-origin partial
updates and compares every pixel against fresh full frames at scales
1, 1.5 and 2, using a real GPU and including ordinary-fill controls.
Run through kontakto-heavy:

```sh
cargo run --profile ci -p mui-vello --features gpu-effects --example gpu_damage -- RECEIPT_DIR
```

This fixes disappearing vector artwork; it does not claim to explain
Conflux's entire duplicated chrome or black rectangle. Those need the
separate presented-window witness. The v1 MUI `aa1f8f3` predated this
explicit vector transform argument and did not have this regression.
