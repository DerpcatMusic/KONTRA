//! Bounded authored-image preparation. PNG strips are read a scanline at a time.
use crate::support::MutexExt;
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::io::Cursor;
const PIXELS: usize = 8 << 20;
static DECODE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn source_rect(meta: ir::ImageMeta, frame: usize, width: u32, height: u32,
    window: Option<[u32;4]>) -> Option<[u32;4]> {
    let n=meta.frames;
    if n==0 {return None;}
    let vertical=meta.axis==ir::Orientation::Vertical;
    if (if vertical {height}else{width})%n!=0 {return None;}
    let (fw,fh)=if vertical {(width,height/n)}else{(width/n,height)};
    let [mut x,mut y,w,h]=window.unwrap_or([0,0,fw,fh]);
    if w==0 || h==0 || x.checked_add(w)?>fw || y.checked_add(h)?>fh {return None;}
    let frame=frame.min(n as usize-1) as u32;
    if vertical {y=y.checked_add(frame.checked_mul(fh)?)?;} else {x=x.checked_add(frame.checked_mul(fw)?)?;}
    Some([x,y,w,h])
}

/// Non-streamable codecs have a bounded atlas; retain only the requested view.
fn selected(image: Image, meta: ir::ImageMeta, frame: usize, target:[u32;2],
    window:Option<[u32;4]>, canceled:impl Fn()->bool) -> Option<Image> {
    let [x,y,w,h]=source_rect(meta,frame,image.width,image.height,window)?;
    let (tw,th)=(target[0].max(1).min(w),target[1].max(1).min(h));
    if [x,y,tw,th]==[0,0,image.width,image.height] {
        if canceled() {return None;}
        return Some(image);
    }
    let len=(tw as usize).checked_mul(th as usize)?.checked_mul(4)?;
    if len>PIXELS*4 {return None;}
    let mut rgba=vec![0;len];
    for oy in 0..th {
        if canceled() {return None;}
        let sy=y+((u64::from(oy)*u64::from(h))/u64::from(th)) as u32;
        for ox in 0..tw {
            let sx=x+((u64::from(ox)*u64::from(w))/u64::from(tw)) as u32;
            let from=(sy as usize*image.width as usize+sx as usize)*4;
            let at=(oy as usize*tw as usize+ox as usize)*4;
            rgba[at..at+4].copy_from_slice(&image.rgba[from..from+4]);
        }
    }
    Image::rgba(tw,th,rgba)
}

/// One source rectangle, shrunk to the requested device pixels. No atlas copy.
pub(super) fn png(
    bytes: &[u8],
    meta: ir::ImageMeta,
    frame: usize,
    target: [u32; 2],
    window: Option<[u32; 4]>,
    canceled: impl Fn() -> bool,
) -> Option<Image> {
    let mut decoder =
        png::Decoder::new_with_limits(Cursor::new(bytes), png::Limits { bytes: 32 << 20 });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let (width, height) = (reader.info().width, reader.info().height);
    if width == 0 || height == 0 || width > 65536 || height > 1048576 {
        return None;
    }
    let [x,y,w,h]=source_rect(meta,frame,width,height,window)?;
    // Never upscale source pixels. A later paint scales the prepared surface.
    let (tw, th) = (target[0].max(1).min(w), target[1].max(1).min(h));
    let len = (tw as usize).checked_mul(th as usize)?.checked_mul(4)?;
    if len > PIXELS * 4 {
        return None;
    }
    let (kind, depth) = reader.output_color_type();
    if depth != png::BitDepth::Eight {
        return None;
    }
    let channels = match kind {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => return None,
    };
    let mut rgba = vec![0; len];
    let put = |dest: &mut [u8], row: &[u8], ox: u32, oy: u32| {
        let at = (x + (u64::from(ox)*u64::from(w)/u64::from(tw)) as u32) as usize * channels;
        let c = &row[at..at + channels];
        let c = match channels {
            4 => [c[0], c[1], c[2], c[3]],
            3 => [c[0], c[1], c[2], 255],
            2 => [c[0], c[0], c[0], c[1]],
            _ => [c[0], c[0], c[0], 255],
        };
        let at = (oy as usize * tw as usize + ox as usize) * 4;
        dest[at..at + 4].copy_from_slice(&c);
    };
    if reader.info().interlaced {
        // Adam7 needs reconstruction. Bound its entire transient atlas separately.
        let len = reader.output_buffer_size()?;
        if len > 8 << 20 {
            return None;
        }
        let mut atlas = vec![0; len];
        reader.next_frame(&mut atlas).ok()?;
        for oy in 0..th {
            if canceled() {
                return None;
            }
            let sy = y + (u64::from(oy)*u64::from(h)/u64::from(th)) as u32;
            let row = &atlas[sy as usize * width as usize * channels
                ..(sy + 1) as usize * width as usize * channels];
            for ox in 0..tw {
                put(&mut rgba, row, ox, oy);
            }
        }
    } else {
        let mut oy = 0;
        for sy in 0..y + h {
            if canceled() {
                return None;
            }
            let row = reader.next_row().ok()??;
            while oy < th && y + (u64::from(oy)*u64::from(h)/u64::from(th)) as u32 == sy {
                for ox in 0..tw {
                    put(&mut rgba, row.data(), ox, oy);
                }
                oy += 1;
            }
        }
        if oy != th {
            return None;
        }
    }
    Image::rgba(tw, th, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn editor_memory_webp_rgb_expansion_stays_below_nine_bytes_per_pixel() {
        let side = 512;
        let pixels = side as usize * side as usize;
        let mut encoded = Vec::new();
        image_webp::WebPEncoder::new(&mut encoded)
            .encode(&[19,29,39].repeat(pixels),side,side,image_webp::ColorType::Rgb).unwrap();
        assert!(!image_webp::WebPDecoder::new(Cursor::new(&encoded)).unwrap().has_alpha());
        let mut image = None;
        let peak = crate::plugin::tests::peak_allocated(|| {
            image = decode(&encoded,Default::default(),0,[side,side],None,||false);
        });
        let image = image.unwrap();
        assert!(image.rgba.chunks_exact(4).all(|p|p==[19,29,39,255]));
        assert!(peak <= pixels*9+16384, "RGB expansion used {peak} bytes for {pixels} pixels");
    }
    #[test]
    fn poisoned_codec_permit_keeps_valid_original_art_decodable() {
        const CHILD: &str = "KONTRA_CODEC_POISON_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "ui::picture_decode::tests::poisoned_codec_permit_keeps_valid_original_art_decodable", "--nocapture"])
                .env(CHILD, "1").env("KONTRA_DISABLE_NETWORK", "1").output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            return;
        }
        assert!(std::panic::catch_unwind(|| {
            let _guard = DECODE.lock().unwrap();
            panic!("synthetic codec permit fault");
        }).is_err());
        assert!(DECODE.is_poisoned());
        let pixels = [9, 99, 199, 127].repeat(4);
        let mut bytes = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut bytes, 2, 2);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder.write_header().unwrap().write_image_data(&pixels).unwrap();
        }
        for _ in 0..2 {
            let image = decode(&bytes, Default::default(), 0, [2, 2], None, || false).unwrap();
            assert_eq!((image.width, image.height), (2, 2));
            assert_eq!(image.rgba.as_ref(), pixels.as_slice());
        }
        assert!(!DECODE.is_poisoned());
    }
    #[test]
    fn extracts_only_requested_strip_frame_and_preserves_alpha() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 3, 6);
            e.set_color(png::ColorType::Rgba);
            e.set_depth(png::BitDepth::Eight);
            let mut w = e.write_header().unwrap();
            w.write_image_data(
                &[
                    vec![10, 20, 30, 40].repeat(9),
                    vec![50, 60, 70, 80].repeat(9),
                ]
                .concat(),
            )
            .unwrap();
        }
        let image = png(
            &bytes,
            ir::ImageMeta {
                frames: 2,
                ..Default::default()
            },
            1,
            [2, 2],
            None,
            || false,
        )
        .unwrap();
        assert_eq!((image.width, image.height, image.rgba.len()), (2, 2, 16));
        assert!(image.rgba.chunks_exact(4).all(|p| p == [50, 60, 70, 80]));
        assert!(png(&bytes, Default::default(), 0, [3, 6], None, || true).is_none());
    }

    #[test]
    fn every_codec_selects_the_same_frame_window_at_device_size() {
        let pixels: Vec<u8> = (0..24).flat_map(|n| [n, 10, 20, 30]).collect();
        let image = Image::rgba(6, 4, pixels).unwrap();
        for (axis, expected) in [
            (ir::Orientation::Horizontal, [10, 16]),
            (ir::Orientation::Vertical, [13, 14]),
        ] {
            let meta = ir::ImageMeta { frames: 2, axis, ..Default::default() };
            let window = match axis {
                ir::Orientation::Horizontal => [1, 1, 2, 2],
                ir::Orientation::Vertical => [1, 0, 2, 2],
            };
            let target = match axis {
                ir::Orientation::Horizontal => [1, 2],
                ir::Orientation::Vertical => [2, 1],
            };
            let prepared = selected(image.clone(), meta, usize::MAX, target, Some(window), || false).unwrap();
            assert_eq!([prepared.width, prepared.height], target);
            assert_eq!(prepared.rgba.as_ref(), [expected[0], 10, 20, 30, expected[1], 10, 20, 30]);
            assert!(selected(image.clone(), meta, 0, target, Some([u32::MAX, 0, 2, 1]), || false).is_none());
            assert!(selected(image.clone(), meta, 0, target, None, || true).is_none());
        }
        assert!(source_rect(ir::ImageMeta {frames: 3, ..Default::default()}, 0, 6, 4, None).is_none());
        assert!(source_rect(ir::ImageMeta {frames: 0, ..Default::default()}, 0, 6, 4, None).is_none());
        let whole = selected(image.clone(), Default::default(), 0, [6,4], None, || false).unwrap();
        assert!(std::sync::Arc::ptr_eq(&whole.rgba, &image.rgba));
    }
}

pub(super) fn decode(
    bytes: &[u8],
    meta: ir::ImageMeta,
    frame: usize,
    target: [u32; 2],
    window: Option<[u32; 4]>,
    canceled: impl Fn() -> bool,
) -> Option<Image> {
    let _permit = DECODE.lock_unpoisoned();
    if bytes.starts_with(b"\x89PNG") {
        return png(bytes, meta, frame, target, window, canceled);
    }
    if canceled() {
        return None;
    }
    if bytes.starts_with(&[0xff, 0xd8]) {
        use zune_jpeg::zune_core::{
            bytestream::ZCursor, colorspace::ColorSpace, options::DecoderOptions,
        };
        let options = DecoderOptions::default()
            .jpeg_set_out_colorspace(ColorSpace::RGBA)
            .set_max_width(8192)
            .set_max_height(8192);
        let mut decoder = zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes), options);
        decoder.decode_headers().ok()?;
        let info = decoder.info()?;
        if info.width as usize * info.height as usize > PIXELS {
            return None;
        }
        let image = Image::rgba(
            info.width.into(),
            info.height.into(),
            decoder.decode().ok()?,
        )?;
        return selected(image,meta,frame,target,window,canceled);
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes)).ok()?;
        let (w, h) = decoder.dimensions();
        if w as usize * h as usize > PIXELS {
            return None;
        }
        let len = decoder.output_buffer_size()?;
        if len > PIXELS * 4 {
            return None;
        }
        let mut data = vec![0; len];
        decoder.read_image(&mut data).ok()?;
        if !decoder.has_alpha() {
            let pixels = data.len() / 3;
            data.resize(pixels * 4, 0);
            for n in (0..pixels).rev() {
                let c = [data[n * 3], data[n * 3 + 1], data[n * 3 + 2], 255];
                data[n * 4..n * 4 + 4].copy_from_slice(&c);
            }
        }
        return selected(Image::rgba(w,h,data)?,meta,frame,target,window,canceled);
    }
    if bytes.len() <= 1 << 20
        && std::str::from_utf8(bytes)
            .ok()
            .is_some_and(|s| s.contains("<svg"))
    {
        let text = std::str::from_utf8(bytes).ok()?;
        if text.matches('<').count() > 16384 || text.matches("<filter").count() > 8 {
            return None;
        }
        let mut options = resvg::usvg::Options::default();
        // SVG cannot bypass the resource authority to read arbitrary local files.
        options.image_href_resolver.resolve_string = Box::new(|_, _| None);
        options.image_href_resolver.resolve_data = Box::new(|_, _, _| None);
        let tree = resvg::usvg::Tree::from_data(bytes, &options).ok()?;
        let size = tree.size().to_int_size();
        let (w, h) = (size.width(), size.height());
        if w as usize * h as usize > 2 << 20 {
            return None;
        }
        let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)?;
        resvg::render(
            &tree,
            resvg::tiny_skia::Transform::identity(),
            &mut pixmap.as_mut(),
        );
        let mut pixels = pixmap.take();
        for p in pixels.chunks_exact_mut(4) {
            if p[3] > 0 {
                for i in 0..3 {
                    p[i] = ((p[i] as u32 * 255 + p[3] as u32 / 2) / p[3] as u32).min(255) as u8;
                }
            }
        }
        return selected(Image::rgba(w,h,pixels)?,meta,frame,target,window,canceled);
    }
    None
}
