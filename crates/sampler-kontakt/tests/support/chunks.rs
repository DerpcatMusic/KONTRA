// Authored wire bytes, no legacy decoder/writer or commercial library fixture.
pub fn sized(data: &[u8]) -> Vec<u8> {
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend(data);
    out
}
pub fn object(version: u16, private: &[u8], public: &[u8], children: &[u8]) -> Vec<u8> {
    let mut out = vec![1];
    out.extend(version.to_le_bytes());
    for data in [private, public, children] {
        out.extend(sized(data));
    }
    out
}
pub fn chunk(id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = id.to_le_bytes().to_vec();
    out.extend(sized(data));
    out
}
