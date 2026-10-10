//! Retained CPU pixels and optional native presentation without a GPU device.
use crate::{Cache, Canvas, Cpu, kurbo::Affine};
use mui_scene::{Painted, ResolvedScene};
use vello_common::pixmap::Pixmap;

/// A single-threaded CPU renderer. Unchanged scenes reuse their pixels.
/// SIMD selection remains Vello's runtime choice; no machine-specific ISA is required.
pub struct Renderer {
    ctx: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    cache: Cache,
    pixels: Pixmap,
    retained: Vec<Painted>,
    transform: Option<Affine>,
}

fn dimensions(size: (u32, u32)) -> Result<(u16, u16), String> {
    // Bound the RGBA target to 64 MiB. Native presentation and renderer scratch
    // are additional memory; this is not a total process-memory budget.
    if size.0 == 0 || size.1 == 0 || u64::from(size.0) * u64::from(size.1) > 16 * 1024 * 1024 {
        return Err("CPU target must contain 1..=16777216 pixels".into());
    }
    Ok((
        size.0
            .try_into()
            .map_err(|_| "CPU target width exceeds u16")?,
        size.1
            .try_into()
            .map_err(|_| "CPU target height exceeds u16")?,
    ))
}

fn context((width, height): (u16, u16)) -> vello_cpu::RenderContext {
    vello_cpu::RenderContext::new_with(
        width,
        height,
        vello_cpu::RenderSettings {
            // Do not compete with the DAW's audio workers, even with cpu-threads enabled.
            num_threads: 0,
            ..Default::default()
        },
    )
}

impl Renderer {
    pub fn new(size: (u32, u32)) -> Result<Self, String> {
        let size = dimensions(size)?;
        Ok(Self {
            ctx: context(size),
            resources: vello_cpu::Resources::default(),
            cache: Cache::default(),
            pixels: Pixmap::new(size.0, size.1),
            retained: Vec::new(),
            transform: None,
        })
    }

    pub fn size(&self) -> (u32, u32) {
        (
            u32::from(self.pixels.width()),
            u32::from(self.pixels.height()),
        )
    }

    pub fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        if self.size() != size {
            let size = dimensions(size)?;
            self.ctx = context(size);
            self.pixels = Pixmap::new(size.0, size.1);
            self.transform = None;
        }
        Ok(())
    }

    /// Premultiplied sRGB RGBA8, top-to-bottom. Valid after a successful render.
    pub fn pixels(&self) -> &[u8] {
        self.pixels.data_as_u8_slice()
    }

    /// Returns whether pixels changed. GPU materials must first be resolved
    /// with `WeldBackend::Reference`; unsupported scenes fail without presenting.
    pub fn render(&mut self, scene: &ResolvedScene, transform: Affine) -> Result<bool, String> {
        self.render_inner(scene, transform, None::<fn(&mut dyn Canvas)>)
    }

    /// Overlays repaint on every call and are included in the presented pixels.
    pub fn render_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: impl FnOnce(&mut dyn Canvas),
    ) -> Result<bool, String> {
        self.render_inner(scene, transform, Some(overlay))
    }

    fn render_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        overlay: Option<impl FnOnce(&mut dyn Canvas)>,
    ) -> Result<bool, String> {
        if !transform.as_coeffs().iter().all(|v| v.is_finite()) {
            return Err("nonfinite CPU scene transform".into());
        }
        if overlay.is_none() && self.transform == Some(transform) && self.retained == scene.paint {
            return Ok(false);
        }
        self.transform = None;
        self.ctx.reset();
        let mut canvas = Cpu {
            ctx: &mut self.ctx,
            resources: &mut self.resources,
            cache: &mut self.cache,
        };
        crate::paint(&mut canvas, scene, transform).map_err(|e| e.to_string())?;
        let has_overlay = overlay.is_some();
        if let Some(overlay) = overlay {
            overlay(&mut canvas);
        }
        self.ctx.flush();
        // ponytail: changed frames repaint fully; add damage clipping after
        // measuring active CPU fallback workloads, preserving blend/backdrop order.
        self.ctx.render(&mut self.pixels, &mut self.resources);
        self.retained.clone_from(&scene.paint);
        self.transform = (!has_overlay).then_some(transform);
        Ok(true)
    }
}

/// An opaque native window presenting CPU pixels. No wgpu device or surface is used.
#[cfg(feature = "software-window")]
pub struct Window<W> {
    surface: softbuffer::Surface<W, W>,
    renderer: Renderer,
    presented: bool,
    opaque_bits: u32,
}

#[cfg(feature = "software-window")]
impl<W: raw_window_handle::HasDisplayHandle + raw_window_handle::HasWindowHandle + Clone>
    Window<W>
{
    pub fn new(window: W, size: (u32, u32)) -> Result<Self, String> {
        // Softbuffer's X11 backend also accepts depth-32 ARGB visuals (which
        // baseview prefers). Their high byte is alpha, not padding; zero would
        // make an otherwise correct CPU frame invisible under a compositor.
        // Depth-24 X11 ignores it. Other backends use softbuffer's 0x00RRGGBB.
        let opaque_bits = match window.window_handle().map_err(|e| e.to_string())?.as_raw() {
            raw_window_handle::RawWindowHandle::Xlib(_)
            | raw_window_handle::RawWindowHandle::Xcb(_) => 0xff00_0000,
            _ => 0,
        };
        let renderer = Renderer::new(size)?;
        let context = softbuffer::Context::new(window.clone()).map_err(|e| e.to_string())?;
        let surface = softbuffer::Surface::new(&context, window).map_err(|e| e.to_string())?;
        Ok(Self {
            surface,
            renderer,
            presented: false,
            opaque_bits,
        })
    }

    /// Re-presents retained pixels after an expose or a failed native present.
    pub fn invalidate(&mut self) {
        self.presented = false;
    }

    /// Zero-sized windows retain their pixels until restored.
    pub fn resize(&mut self, size: (u32, u32)) -> Result<(), String> {
        if size.0 != 0 && size.1 != 0 {
            self.renderer.resize(size)?;
        }
        self.invalidate();
        Ok(())
    }

    pub fn present(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
    ) -> Result<bool, String> {
        self.present_inner(scene, transform, size, None::<fn(&mut dyn Canvas)>)
    }

    pub fn present_with_overlay(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
        overlay: impl FnOnce(&mut dyn Canvas),
    ) -> Result<bool, String> {
        self.present_inner(scene, transform, size, Some(overlay))
    }

    fn present_inner(
        &mut self,
        scene: &ResolvedScene,
        transform: Affine,
        size: (u32, u32),
        overlay: Option<impl FnOnce(&mut dyn Canvas)>,
    ) -> Result<bool, String> {
        use std::num::NonZeroU32;
        let (Some(width), Some(height)) = (NonZeroU32::new(size.0), NonZeroU32::new(size.1)) else {
            return Ok(false);
        };
        self.renderer.resize(size)?;
        if self.renderer.render_inner(scene, transform, overlay)? {
            self.presented = false;
        }
        if self.presented {
            return Ok(false);
        }
        self.surface
            .resize(width, height)
            .map_err(|e| e.to_string())?;
        let mut buffer = self.surface.buffer_mut().map_err(|e| e.to_string())?;
        for (out, rgba) in buffer
            .iter_mut()
            .zip(self.renderer.pixels().as_chunks::<4>().0)
        {
            // Opaque window: premultiplied RGB composites transparency over black.
            *out = self.opaque_bits
                | (u32::from(rgba[0]) << 16)
                | (u32::from(rgba[1]) << 8)
                | u32::from(rgba[2]);
        }
        buffer.present().map_err(|e| e.to_string())?;
        self.presented = true;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    #[test]
    fn gpu_materials_are_rebuilt_for_cpu_and_can_return_to_gpu() {
        let root = || {
            row![
                block(20., 20.).fill(Role::Primary),
                block(20., 20.).fill(Role::Primary),
            ]
            .weld(Weld::all().reach(4.).blend(4.))
        };
        let mut ui = mui::Ui::default().gpu_welding();
        let mut renderer = Renderer::new((80, 40)).unwrap();
        for available in [true, false, true] {
            ui.set_gpu_welding_available(available);
            let frame = ui
                .frame(
                    root(),
                    Some(Size::new(80., 40.)),
                    mui::prelude::PointerInput::default(),
                    0.,
                )
                .unwrap();
            assert_eq!(frame.scene.external_welds().count() > 0, available);
            assert_eq!(
                renderer.render(frame.scene, Affine::IDENTITY).is_ok(),
                !available
            );
        }
    }

    #[test]
    fn cpu_overlay_is_visible_and_removing_it_restores_retained_scene() {
        use crate::kurbo::{Rect, Shape};
        let scene = mui_scene::resolve(&SceneSpec::new(
            block(8., 8.).radius(0.).fill(Color::srgb(1., 0., 0.)),
        ))
        .unwrap();
        let mut renderer = Renderer::new((8, 8)).unwrap();
        renderer.render(&scene, Affine::IDENTITY).unwrap();
        for green in [true, false] {
            let color = if green {
                Color::srgb(0., 1., 0.)
            } else {
                Color::srgb(0., 0., 1.)
            };
            assert!(
                renderer
                    .render_with_overlay(&scene, Affine::IDENTITY, |canvas| {
                        canvas.set_transform(Affine::IDENTITY);
                        canvas.set_paint(color.to_srgb().into());
                        canvas.fill_path(&Rect::new(0., 0., 4., 8.).to_path(0.1));
                    })
                    .unwrap()
            );
            assert_eq!(
                &renderer.pixels()[..4],
                if green {
                    &[0, 255, 0, 255]
                } else {
                    &[0, 0, 255, 255]
                }
            );
            assert_eq!(&renderer.pixels()[7 * 4..8 * 4], &[255, 0, 0, 255]);
        }
        assert!(renderer.render(&scene, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[255, 0, 0, 255]);
        assert!(!renderer.render(&scene, Affine::IDENTITY).unwrap());
    }

    #[test]
    fn retained_cpu_pixels_handle_changes_resize_and_invalid_input() {
        let scene = |color| {
            mui_scene::resolve(&SceneSpec::new(block(8., 8.).radius(0.).fill(color))).unwrap()
        };
        let red = scene(Color::srgb(1., 0., 0.));
        let green = scene(Color::srgb(0., 1., 0.));
        let mut renderer = Renderer::new((8, 8)).unwrap();
        assert!(renderer.render(&red, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[255, 0, 0, 255]);
        assert!(!renderer.render(&red, Affine::IDENTITY).unwrap());
        assert!(renderer.render(&green, Affine::IDENTITY).unwrap());
        assert_eq!(&renderer.pixels()[..4], &[0, 255, 0, 255]);
        assert!(renderer.render(&green, Affine::scale(f64::NAN)).is_err());
        assert!(renderer.resize((u32::MAX, u32::MAX)).is_err());
        assert_eq!(renderer.size(), (8, 8));
        renderer.resize((16, 16)).unwrap();
        assert!(renderer.render(&green, Affine::scale(2.)).unwrap());
        assert_eq!(
            &renderer.pixels()[renderer.pixels().len() - 4..],
            &[0, 255, 0, 255]
        );
        assert!(Renderer::new((0, 8)).is_err());
    }
}
