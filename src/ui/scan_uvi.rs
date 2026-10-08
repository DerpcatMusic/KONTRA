//! Shared scanner: the v1 production Original panel, no library resources saved.
use super::{theme, uvi_instrument};
use crate::{
    library::ViewMode,
    scan_metrics as metrics,
    uvi::{host::UiSnapshot, ui_assets::UiAssets, worker::Stamp},
};
use moose::mui::mui::prelude::*;
pub(crate) fn paint(
    snapshot: &UiSnapshot,
    assets: &mut UiAssets,
    stamp: Stamp,
) -> anyhow::Result<serde_json::Value> {
    use moose::mui::mui::vello::{
        self,
        vello_cpu::{Pixmap, RenderContext, Resources},
    };
    let pictures = assets.refresh(std::slice::from_ref(snapshot));
    let fonts = assets.fonts();
    let mut ui = theme::ui();
    let mut state = uvi_instrument::State::default();
    let width = snapshot.root.width.ceil().clamp(1., 4096.) as u16;
    let height = snapshot.root.height.ceil().clamp(1., 4096.) as u16;
    for _ in 0..2 {
        let root = uvi_instrument::view(
            &mut ui,
            &mut state,
            0,
            stamp,
            stamp,
            (0, 0),
            snapshot,
            &pictures,
            &fonts,
            ViewMode::Original,
            |_, _| Some(1),
        );
        ui.frame(
            root,
            Some(Size::new(width.into(), height.into())),
            Input::default(),
            1. / 60.,
        )
        .map_err(|_| anyhow::anyhow!("layout failed"))?;
    }
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    vello::paint(
        &mut vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut vello::Cache::default(),
        },
        ui.scene().ok_or_else(|| anyhow::anyhow!("no scene"))?,
        vello::kurbo::Affine::IDENTITY,
    )
    .map_err(|_| anyhow::anyhow!("paint failed"))?;
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut resources);
    let rgba: Vec<_> = pix
        .take_unpremultiplied()
        .iter()
        .flat_map(|p| [p.r, p.g, p.b, p.a])
        .collect();
    Ok(metrics::pixels(&rgba))
}
