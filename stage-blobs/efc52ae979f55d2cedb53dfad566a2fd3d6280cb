// Authored NIS wire fixtures, independent of the source decoder and old importer.
pub fn layer(domain: &[u8; 4], id: u32, properties: &[u8], inner: &[u8]) -> Vec<u8> {
    let mut out = ((20 + properties.len() + inner.len()) as u64)
        .to_le_bytes()
        .to_vec();
    out.extend(domain.iter().rev());
    out.extend(id.to_le_bytes());
    out.extend(1u32.to_le_bytes());
    out.extend(inner);
    out.extend(properties);
    out
}
pub fn item(layers: &[u8], children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = vec![0; 8];
    out.extend(1u32.to_le_bytes());
    out.extend(b"hsin");
    out.extend(0xaabbccddu32.to_le_bytes());
    out.extend(0x11223344u32.to_le_bytes());
    out.extend([0x5a; 16]);
    out.extend(layers);
    out.extend(1u32.to_le_bytes());
    out.extend((children.len() as u32).to_le_bytes());
    for child in children {
        out.extend([0xaa; 12]); // Deliberately opaque/noncanonical descriptors.
        out.extend(child);
    }
    out.extend([0xfe, 0xed]);
    let length = out.len() as u64;
    out[..8].copy_from_slice(&length.to_le_bytes());
    out
}
pub fn encryption(payload: &[u8], compressed: bool, protected: bool) -> Vec<u8> {
    let mut subtree = 1u32.to_le_bytes().to_vec();
    subtree.push(u8::from(compressed));
    if compressed {
        let mut packed = Vec::new();
        for run in payload.chunks(32) {
            packed.push((run.len() - 1) as u8);
            packed.extend(run);
        }
        subtree.extend((payload.len() as u32).to_le_bytes());
        subtree.extend((packed.len() as u32).to_le_bytes());
        subtree.extend(packed);
    } else {
        subtree.extend(payload);
    }
    let base = layer(b"NISD", 1, &[9], &[]);
    let subtree = layer(b"NISD", 0x73, &subtree, &base);
    let mut flags = 1u32.to_le_bytes().to_vec();
    flags.push(u8::from(protected));
    item(&layer(b"NISD", 0x74, &flags, &subtree), &[])
}
