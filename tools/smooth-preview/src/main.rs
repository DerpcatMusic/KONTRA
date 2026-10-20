//! SMOOTH: a spectrally selective dynamics editor, drawn by MUI.
//!
//! The editor is one intrinsic tree. `skin` holds the look -- metrics,
//! palette, and the parts every view is built from; `view` composes those
//! parts into the column, rail and right column; `graph` is the response
//! plot. Nothing is placed at a coordinate: canvases are the only place with
//! geometry, and even they are given a box by the layout.
//!
//! ```text
//! cargo run -p smooth-preview --release -- [out.png] [scale] [theme]
//! ```
//!
//! `scale` renders the same tree at 2x or 4x for a pixel comparison against
//! the reference. `theme` picks one of `smooth_ui::theme::THEMES`; with no
//! argument, `SMOOTH_THEME` is consulted and `voice` -- the reference's own
//! look -- is the default.

use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};
use smooth_ui::Model;
use smooth_ui::{H, W};
use smooth_ui::{skin, view};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let out = args.first().map_or("smooth.png".to_owned(), Clone::clone);
    let scale = args.get(1).map_or(Ok(1.0), |s| s.parse::<f64>())?;
    if let Some(id) = args.get(2) {
        if smooth_ui::theme::set(id).is_none() {
            return Err(format!(
                "unknown theme {id:?}; try one of {:?}",
                smooth_ui::theme::THEMES
                    .iter()
                    .map(|theme| theme.id)
                    .collect::<Vec<_>>()
            )
            .into());
        }
    } else {
        smooth_ui::theme::from_env();
    }

    let mut ui = Ui::default().font(font());
    if let Some(fallback) = fallback_font() {
        ui = ui.fallback_font(fallback);
    }
    ui.set_scale(Some(scale));

    let mut model = Model::default();
    let mut dial = skin::model_dial();
    for _ in 0..2 {
        let root = view::editor(&mut ui, &mut model, &mut dial);
        ui.frame(root, Some(Size::new(W, H)), Input::default(), 1.0 / 60.0)?;
    }

    let width = (W * scale).round() as u16;
    let height = (H * scale).round() as u16;
    let mut ctx = RenderContext::new(width, height);
    let mut resources = Resources::default();
    mui::vello::paint(
        &mut mui::vello::Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut mui::vello::Cache::default(),
        },
        ui.scene().expect("a resolved scene"),
        mui::vello::kurbo::Affine::scale(scale),
    )?;
    ctx.flush();
    let mut pixmap = Pixmap::new(width, height);
    ctx.render(&mut pixmap, &mut resources);

    // The window is opaque in the reference: what falls outside its rounded
    // corners is the desktop, so it is flattened to the page colour here.
    let page_hex = smooth_ui::theme::t().page;
    let page = [
        (page_hex >> 16) as u8,
        (page_hex >> 8) as u8,
        page_hex as u8,
    ];
    let rgba: Vec<u8> = pixmap
        .take_unpremultiplied()
        .iter()
        .flat_map(|p| {
            let a = p.a as f32 / 255.0;
            let over = |v: u8, b: u8| (v as f32 * a + b as f32 * (1.0 - a)).round() as u8;
            [
                over(p.r, page[0]),
                over(p.g, page[1]),
                over(p.b, page[2]),
                255,
            ]
        })
        .collect();

    let mut encoder = png::Encoder::new(
        std::io::BufWriter::new(std::fs::File::create(&out)?),
        width as u32,
        height as u32,
    );
    encoder.set_color(png::ColorType::Rgba);
    encoder.write_header()?.write_image_data(&rgba)?;
    println!("Wrote {out} at {width}x{height}");
    Ok(())
}

/// SMOOTH's face: a rounded geometric sans, with Noto for what it lacks.
fn font() -> Font {
    let home = std::env::var("HOME").unwrap_or_default();
    let candidates = [
        format!(
            "{home}/.local/share/fonts/GoogleSansFlex-VariableFont_GRAD,ROND,opsz,slnt,wdth,wght.ttf"
        ),
        "assets/NotoSans.ttf".to_owned(),
    ];
    let bytes = candidates
        .iter()
        .find_map(|path| std::fs::read(path).ok())
        .unwrap_or_default();
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    Font::new(leaked).expect("a UI font")
}

fn fallback_font() -> Option<Font> {
    let bytes = std::fs::read("assets/NotoSans.ttf").ok()?;
    let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
    Font::new(leaked).ok()
}
