//! Read local library artwork once, on the import worker. No copies on disk.
use moose::mui::mui::scene::Image;
use std::{
    collections::HashMap,
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::Arc,
};

pub fn scan(root: &Path, files: &[PathBuf]) -> HashMap<String, Arc<Image>> {
    let names: std::collections::BTreeSet<_> = files
        .iter()
        .filter_map(|p| {
            p.strip_prefix(root)
                .ok()?
                .components()
                .next()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
        })
        .collect();
    names
        .into_iter()
        .filter_map(|name| {
            let folder = root.join(&name);
            let mut candidates = vec![folder.join("wallpaper.png")];
            if let Ok(entries) = std::fs::read_dir(&folder) {
                let mut containers: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| {
                        p.extension().is_some_and(|e| {
                            e.eq_ignore_ascii_case("nicnt") || e.eq_ignore_ascii_case("nkr")
                        })
                    })
                    .collect();
                containers.sort_by_key(|p| {
                    (
                        !p.extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("nicnt")),
                        p.clone(),
                    )
                });
                candidates.extend(containers);
            }
            for path in candidates {
                let mut bytes = Vec::new();
                if File::open(path)
                    .and_then(|f| f.take(8 * 1024 * 1024).read_to_end(&mut bytes))
                    .is_err()
                {
                    continue;
                }
                // NICNT product wallpaper is a complete PNG following the product metadata.
                for (start, _) in bytes
                    .windows(8)
                    .enumerate()
                    .filter(|(_, b)| *b == b"\x89PNG\r\n\x1a\n")
                {
                    if let Some(image) = decode(&bytes[start..]) {
                        if image.width >= 180 && image.height >= 60 {
                            return Some((name, Arc::new(image)));
                        }
                    }
                }
            }
            None
        })
        .collect()
}
/// Resolve the selected preset's named wallpaper, never an arbitrary PNG in its NKR.
pub fn performance(
    instrument: &crate::import::Instrument,
    computed: Option<&str>,
) -> Result<Option<Arc<Image>>, String> {
    let Some(name) = computed
        .map(str::to_owned)
        .or_else(|| wallpaper(&instrument.scripts))
    else {
        return Ok(None);
    };
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err("Invalid instrument wallpaper name".into());
    }
    let filename = if name.to_lowercase().ends_with(".png") {
        name
    } else {
        format!("{name}.png")
    };
    // Fall back to literal names when the init interpreter cannot run this script.
    for folder in instrument.path.ancestors().skip(1).take(4) {
        for relative in ["Resources/pictures", "resources/pictures", "pictures"] {
            if let Ok(entries) = std::fs::read_dir(folder.join(relative)) {
                for file in entries.flatten().filter(|e| {
                    e.file_name()
                        .to_string_lossy()
                        .eq_ignore_ascii_case(&filename)
                }) {
                    let bytes = File::open(file.path())
                        .and_then(|f| {
                            let mut bytes = Vec::new();
                            f.take(32 * 1024 * 1024).read_to_end(&mut bytes)?;
                            Ok(bytes)
                        })
                        .map_err(|e| e.to_string())?;
                    return decode(&bytes)
                        .map(|i| Some(Arc::new(i)))
                        .ok_or_else(|| "Invalid instrument wallpaper PNG".into());
                }
            }
        }
        let mut containers: Vec<_> = std::fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nkr")))
            .collect();
        containers.sort();
        for path in containers {
            let Ok(mut file) = File::open(&path) else {
                continue;
            };
            let Ok(archive) = ni_file::nkr::Archive::read(&mut file) else {
                continue;
            };
            let resource = format!("Resources/pictures/{filename}");
            let Some(entry) = archive.find(&resource) else {
                continue;
            };
            if entry.size > 32 * 1024 * 1024 {
                return Err("Instrument wallpaper exceeds 32 MiB".into());
            }
            let key = crate::import::library_key(&instrument.path).map_err(|e| e.to_string())?;
            let bytes = archive
                .read_entry_with_key(&mut file, &resource, key.as_ref())
                .map_err(|e| e.to_string())?;
            return decode(&bytes)
                .map(|i| Some(Arc::new(i)))
                .ok_or_else(|| "Invalid instrument wallpaper PNG".into());
        }
    }
    Err(format!("Instrument wallpaper {filename} was not found"))
}
fn wallpaper(scripts: &[String]) -> Option<String> {
    scripts
        .iter()
        .flat_map(|s| s.lines())
        .filter_map(|line| {
            let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            let value = compact
                .strip_prefix("set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,\"")?;
            // Preserve spaces in the original filename.
            if !value.ends_with("\")") {
                return None;
            }
            let start = line.find('"')? + 1;
            let end = line.rfind('"')?;
            Some(line[start..end].to_owned())
        })
        .last()
        .filter(|name| !name.is_empty())
}
fn decode(bytes: &[u8]) -> Option<Image> {
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let size = reader.output_buffer_size()?;
    if size > 32 * 1024 * 1024 {
        return None;
    }
    let mut data = vec![0; size];
    let info = reader.next_frame(&mut data).ok()?;
    let data = &data[..info.buffer_size()];
    let rgba = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .chunks_exact(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|v| [*v, *v, *v, 255]).collect(),
        png::ColorType::GrayscaleAlpha => data
            .chunks_exact(2)
            .flat_map(|c| [c[0], c[0], c[0], c[1]])
            .collect(),
        _ => return None,
    };
    Image::rgba(info.width, info.height, rgba)
}
#[cfg(test)]
mod tests {
    #[test]
    fn named_wallpaper() {
        assert_eq!(
            super::wallpaper(&[
                "set_control_par_str($INST_WALLPAPER_ID, $CONTROL_PAR_PICTURE, \"A B\")".into()
            ]),
            Some("A B".into())
        );
        assert_eq!(
            super::wallpaper(&[
                "set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,@dynamic)".into()
            ]),
            None
        );
    }
    #[test]
    fn png_decode_checks_input_and_keeps_alpha() {
        let mut bytes = Vec::new();
        {
            let mut e = png::Encoder::new(&mut bytes, 1, 1);
            e.set_color(png::ColorType::Rgba);
            let mut w = e.write_header().unwrap();
            w.write_image_data(&[12, 34, 56, 78]).unwrap();
        }
        assert_eq!(
            super::decode(&bytes).unwrap().rgba.as_ref(),
            &[12, 34, 56, 78]
        );
        assert!(super::decode(&bytes[..12]).is_none());
        assert!(super::decode(b"not an image").is_none());
    }
}
