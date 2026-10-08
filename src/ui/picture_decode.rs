//! Bounded authored-image preparation. PNG strips are read a scanline at a time.
use moose::mui::mui::scene::Image;
use sampler_ui_ir as ir;
use std::io::Cursor;
const PIXELS: usize = 8 << 20;

/// One source rectangle, shrunk to the requested device pixels. No atlas copy.
pub(super) fn png(bytes:&[u8], meta:ir::ImageMeta, frame:usize, target:[u32;2], window:Option<[u32;4]>, canceled:impl Fn()->bool)->Option<Image> {
    let mut decoder=png::Decoder::new_with_limits(Cursor::new(bytes),png::Limits{bytes:32<<20});
    decoder.set_transformations(png::Transformations::EXPAND|png::Transformations::STRIP_16);
    let mut reader=decoder.read_info().ok()?;
    let (width,height)=(reader.info().width,reader.info().height);
    if width==0 || height==0 || width>65536 || height>1048576 {return None;}
    let n=meta.frames.max(1);let frame=(frame as u32).min(n-1);
    let (fw,fh)=if meta.axis==ir::Orientation::Vertical {(width,height/n)}else{(width/n,height)};
    if fw==0 || fh==0 {return None;}
    let [mut x,mut y,w,h]=window.unwrap_or([0,0,fw,fh]);
    if x.checked_add(w)?>fw || y.checked_add(h)?>fh || w==0 || h==0 {return None;}
    if meta.axis==ir::Orientation::Vertical {y+=frame*fh;}else{x+=frame*fw;}
    // Never upscale source pixels. A later paint scales the prepared surface.
    let (tw,th)=(target[0].max(1).min(w),target[1].max(1).min(h));
    let len=(tw as usize).checked_mul(th as usize)?.checked_mul(4)?;
    if len>PIXELS*4 {return None;}
    let (kind,depth)=reader.output_color_type();
    if depth!=png::BitDepth::Eight {return None;}
    let channels=match kind {png::ColorType::Rgba=>4,png::ColorType::Rgb=>3,png::ColorType::GrayscaleAlpha=>2,png::ColorType::Grayscale=>1,_=>return None};
    let mut rgba=vec![0;len];
    let put=|dest:&mut [u8],row:&[u8],ox:u32,oy:u32| {
        let at=(x+ox*w/tw) as usize*channels;let c=&row[at..at+channels];
        let c=match channels {4=>[c[0],c[1],c[2],c[3]],3=>[c[0],c[1],c[2],255],2=>[c[0],c[0],c[0],c[1]],_=>[c[0],c[0],c[0],255]};
        let at=(oy as usize*tw as usize+ox as usize)*4;dest[at..at+4].copy_from_slice(&c);
    };
    if reader.info().interlaced {
        // Adam7 needs reconstruction. Bound its entire transient atlas separately.
        let len=reader.output_buffer_size()?;if len>8<<20 {return None;}
        let mut atlas=vec![0;len];reader.next_frame(&mut atlas).ok()?;
        for oy in 0..th {if canceled(){return None;}let sy=y+oy*h/th;let row=&atlas[sy as usize*width as usize*channels..(sy+1) as usize*width as usize*channels];for ox in 0..tw {put(&mut rgba,row,ox,oy);}}
    } else {
        let mut oy=0;
        for sy in 0..y+h {
            if canceled(){return None;}
            let row=reader.next_row().ok()??;
            while oy<th && y+oy*h/th==sy {for ox in 0..tw {put(&mut rgba,row.data(),ox,oy);}oy+=1;}
        }
        if oy!=th {return None;}
    }
    Image::rgba(tw,th,rgba)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn extracts_only_requested_strip_frame_and_preserves_alpha() {
        let mut bytes=Vec::new();
        {let mut e=png::Encoder::new(&mut bytes,3,6);e.set_color(png::ColorType::Rgba);e.set_depth(png::BitDepth::Eight);let mut w=e.write_header().unwrap();w.write_image_data(&[vec![10,20,30,40].repeat(9),vec![50,60,70,80].repeat(9)].concat()).unwrap();}
        let image=png(&bytes,ir::ImageMeta{frames:2,..Default::default()},1,[2,2],None,||false).unwrap();
        assert_eq!((image.width,image.height,image.rgba.len()),(2,2,16));
        assert!(image.rgba.chunks_exact(4).all(|p|p==[50,60,70,80]));
        assert!(png(&bytes,Default::default(),0,[3,6],None,||true).is_none());
    }
}

pub(super) fn decode(bytes:&[u8],meta:ir::ImageMeta,frame:usize,target:[u32;2],window:Option<[u32;4]>,canceled:impl Fn()->bool)->Option<Image> {
    if bytes.starts_with(b"\x89PNG") {return png(bytes,meta,frame,target,window,canceled);}
    if canceled() {return None;}
    if bytes.starts_with(&[0xff,0xd8]) {
        use zune_jpeg::zune_core::{bytestream::ZCursor,colorspace::ColorSpace,options::DecoderOptions};
        let options=DecoderOptions::default().jpeg_set_out_colorspace(ColorSpace::RGBA).set_max_width(8192).set_max_height(8192);
        let mut decoder=zune_jpeg::JpegDecoder::new_with_options(ZCursor::new(bytes),options);
        decoder.decode_headers().ok()?;let info=decoder.info()?;
        if info.width as usize*info.height as usize>PIXELS {return None;}
        let image=Image::rgba(info.width.into(),info.height.into(),decoder.decode().ok()?)?;
        let n=meta.frames.max(1);let frame=frame.min(n as usize-1) as u32;
        let (w,h)=if meta.axis==ir::Orientation::Vertical {(image.width,image.height/n)}else{(image.width/n,image.height)};
        let [mut x,mut y,w,h]=window.unwrap_or([0,0,w,h]);
        if meta.axis==ir::Orientation::Vertical {y+=frame*(image.height/n);}else{x+=frame*(image.width/n);}
        let image=super::pictures::crop(&image,x,y,w,h)?;
        return Some((*image).clone());
    }
    None
}
