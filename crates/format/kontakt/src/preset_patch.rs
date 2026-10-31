//! Measurement harness: a copy of a preset with its Hot Solo record rewritten.
//!
//! The instrument's Amps & FX page is KSP-drawn, so the oracle's click tool
//! cannot operate it; a saved record is the only handle on Hot Solo's knobs.
//! These tests rewrite copies of a preset (never the original: `src` is opened
//! read-only and the output is a new path), which is how a knob sweep for
//! `docs/re/engine/k-hotsolo.md` is produced: one copy per knob setting, each
//! rendered through `tools/oracle-mcp`.
//!
//! The record's slot is found by walking the decrypted chunk stream
//! (0x28 program → 0x3a insert rack → 0x25 slot → 0x54 effect) so the byte
//! ranges are the file's own, and the copy is re-encoded with the library
//! keystream it came with.
//!
//! ```text
//! PATCH_SRC=Electric\ Mint.nki PATCH_OUT=out.nki \
//!   PATCH_VALUES=0.333,0.470,0.420,0.962,0.603,0.603,0.752,0.643 \
//!   cargo test -p kontra-format-kontakt --lib patch_hot_solo -- --ignored --nocapture
//! ```
//! `PATCH_SRC` is the source `.nki` (the library's own instrument), `PATCH_OUT`
//! the copy to write. `PATCH_BYPASS=1` also sets the slot's bypass byte;
//! `PATCH_LIST` takes one `output.nki values [bypass]` line per copy, all read
//! from the same `PATCH_SRC`.

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::Path;

    use ni_file::nis::{ItemContainer, ItemType, SubtreeItem};

    /// The eight floats' order: prenormal, pre-overdrive, master, presence,
    /// depth, treble, mid, bass (reader 0x140d01b50).
    const VALUES: usize = 8;

    fn find<'a>(item: &'a ItemContainer, kind: &ItemType) -> Option<&'a ItemContainer> {
        if &item.data.header.item_type() == kind {
            return Some(item);
        }
        item.children.iter().find_map(|c| find(c, kind))
    }

    fn find_mut<'a>(item: &'a mut ItemContainer, kind: &ItemType) -> Option<&'a mut ItemContainer> {
        if &item.data.header.item_type() == kind {
            return Some(item);
        }
        item.children.iter_mut().find_map(|c| find_mut(c, kind))
    }

    /// The chunk stream inside the decrypted preset subtree, and where it
    /// starts in `inner_data`.
    fn stream(inner_data: &[u8]) -> (usize, usize) {
        let inner = ItemContainer::read(Cursor::new(inner_data)).unwrap();
        let preset = find(&inner, &ItemType::PresetChunkItem).unwrap();
        let data = &preset.data.data;
        let size = u64::from_le_bytes(data[12..20].try_into().unwrap()) as usize;
        let at = inner_data
            .windows(16)
            .position(|w| w == &data[20..36])
            .expect("chunk stream start");
        (at, size)
    }

    /// A chunk and its structured-object parts, as byte ranges in the stream.
    struct Walk {
        id: u16,
        private: Option<(usize, usize)>,
        public: Option<(usize, usize)>,
        children: Vec<Walk>,
    }

    /// One chunk at `at`: its walk record and the offset after it.
    fn chunk_at(buf: &[u8], at: usize, end: usize, depth: usize) -> Option<(Walk, usize)> {
        if at + 6 > end {
            return None;
        }
        let id = u16::from_le_bytes(buf[at..at + 2].try_into().ok()?);
        let len = u32::from_le_bytes(buf[at + 2..at + 6].try_into().ok()?) as usize;
        let body = at + 6;
        let after = body + len;
        if after > end {
            return None;
        }
        let mut node = Walk {
            id,
            private: None,
            public: None,
            children: Vec::new(),
        };
        if id == 0x3a && len > 3 {
            // BParamArrayBParFX8: structured byte, u16 version, then per slot a
            // present flag and the slot chunk.
            let version = u16::from_le_bytes(buf[body + 1..body + 3].try_into().ok()?);
            let mut inner = body + 3;
            if version == 0x13 {
                inner += 4;
            }
            for _ in 0..8 {
                if inner >= after {
                    break;
                }
                let flag = buf[inner];
                inner += 1;
                if flag == 1 {
                    let (slot, next) = chunk_at(buf, inner, after, depth + 1)?;
                    node.children.push(slot);
                    inner = next;
                }
            }
        } else if len > 4 && buf[body] == 0 {
            // Unstructured: u16 version, then the public data to the end.
            node.public = Some((body + 3, len - 3));
        } else if len > 9 && buf[body] == 1 && depth < 8 {
            // StructuredObject: private, public, then its children.
            let private_len = u32::from_le_bytes(buf[body + 3..body + 7].try_into().ok()?) as usize;
            let public_at = body + 7 + private_len;
            if public_at + 4 <= after {
                let public_len =
                    u32::from_le_bytes(buf[public_at..public_at + 4].try_into().ok()?) as usize;
                let children_at = public_at + 4 + public_len;
                if children_at + 4 <= after {
                    let children_len =
                        u32::from_le_bytes(buf[children_at..children_at + 4].try_into().ok()?)
                            as usize;
                    if children_at + 4 + children_len <= after {
                        node.private = Some((body + 7, private_len));
                        node.public = Some((public_at + 4, public_len));
                        node.children = walk(
                            buf,
                            children_at + 4,
                            children_at + 4 + children_len,
                            depth + 1,
                        );
                    }
                }
            }
        }
        Some((node, after))
    }

    fn walk(buf: &[u8], mut at: usize, end: usize, depth: usize) -> Vec<Walk> {
        let mut out = Vec::new();
        while let Some((node, after)) = chunk_at(buf, at, end, depth) {
            out.push(node);
            at = after;
        }
        out
    }

    /// The Hot Solo slot's (private, public) ranges: 0x28 program, 0x3a rack,
    /// 0x25 slot, 0x54 effect.
    fn hot_solo(buf: &[u8], start: usize, size: usize) -> ((usize, usize), (usize, usize)) {
        let top = walk(buf, start, start + size, 0);
        let program = top.iter().find(|c| c.id == 0x28).expect("program chunk");
        for rack in program.children.iter().filter(|c| c.id == 0x3a) {
            for slot in &rack.children {
                if slot.id != 0x25 {
                    continue;
                }
                if let Some(effect) = slot.children.iter().find(|c| c.id == 0x54) {
                    return (slot.private.unwrap(), effect.public.unwrap());
                }
            }
        }
        panic!("no Hot Solo slot");
    }

    /// The preset's subtree with its library key.
    fn open(
        src: &Path,
    ) -> (
        ItemContainer,
        std::sync::Arc<dyn ni_file::nis::LibraryKey>,
        Vec<u8>,
    ) {
        let tree = ItemContainer::read(std::fs::File::open(src).unwrap()).unwrap();
        let key = crate::access::library_key(src).unwrap();
        let enc = find(&tree, &ItemType::EncryptionItem).unwrap();
        let subtree = SubtreeItem::read_with_key(
            Cursor::new(&enc.data.inner.as_ref().unwrap().data),
            Some(&*key),
        )
        .unwrap();
        (tree, key, subtree.inner_data)
    }

    fn patch(src: &Path, out: &Path, values: [f32; VALUES], bypass: Option<bool>) {
        let (mut tree, key, mut inner_data) = open(src);
        let (start, size) = stream(&inner_data);
        let (private, public) = hot_solo(&inner_data, start, size);
        assert_eq!(public.1, 34, "Hot Solo v0x10 public data");
        for (n, v) in values.iter().enumerate() {
            let at = public.0 + 4 * n;
            inner_data[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        if let Some(bypass) = bypass {
            // BParFXParams: effect type, reserved, flag, bypass, output, dry.
            inner_data[private.0 + 9] = u8::from(bypass);
        }
        let subtree = SubtreeItem { inner_data };
        let mut body = Vec::new();
        subtree
            .write_with_key(&mut body, true, Some(&*key))
            .unwrap();
        find_mut(&mut tree, &ItemType::EncryptionItem)
            .unwrap()
            .data
            .inner
            .as_mut()
            .unwrap()
            .data = body;
        tree.write(&mut std::fs::File::create(out).unwrap())
            .unwrap();
        // The copy reads back with the new values through the same key.
        let copy = ItemContainer::read(std::fs::File::open(out).unwrap()).unwrap();
        let enc = find(&copy, &ItemType::EncryptionItem).unwrap();
        let again = SubtreeItem::read_with_key(
            Cursor::new(&enc.data.inner.as_ref().unwrap().data),
            Some(&*key),
        )
        .unwrap()
        .inner_data;
        let (start, size) = stream(&again);
        let (_, public) = hot_solo(&again, start, size);
        assert_eq!(
            &again[public.0..public.0 + 32],
            &values.map(f32::to_le_bytes).concat()[..]
        );
        println!("{}: {values:?} bypass={bypass:?}", out.display());
    }

    fn values(list: &str) -> [f32; VALUES] {
        let parsed: Vec<f32> = list.split(',').map(|v| v.trim().parse().unwrap()).collect();
        parsed.try_into().unwrap()
    }

    /// The preset to copy: `PATCH_SRC`, the path of the source `.nki`. Kept in
    /// the environment, as the output path and the values are, so the sweep
    /// runs on any machine that has the library.
    fn source() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var_os("PATCH_SRC").expect("PATCH_SRC: path of the source .nki"),
        )
    }

    #[test]
    #[ignore = "writes a preset copy; needs the Electric Mint library"]
    fn patch_hot_solo() {
        let out = Path::new(&std::env::var("PATCH_OUT").unwrap()).to_path_buf();
        let bypass = std::env::var("PATCH_BYPASS").ok().map(|b| b == "1");
        patch(
            &source(),
            &out,
            values(&std::env::var("PATCH_VALUES").unwrap()),
            bypass,
        );
    }

    #[test]
    #[ignore = "writes preset copies; needs the Electric Mint library"]
    fn patch_many() {
        for line in std::fs::read_to_string(std::env::var("PATCH_LIST").unwrap())
            .unwrap()
            .lines()
            .filter(|l| !l.trim().is_empty())
        {
            let mut parts = line.split(' ');
            let out = parts.next().unwrap();
            let values = values(parts.next().unwrap());
            let bypass = parts.next().map(|b| b.trim() == "1");
            patch(&source(), Path::new(out), values, bypass);
        }
    }
}
