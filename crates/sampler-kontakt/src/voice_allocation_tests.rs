// Synthetic BVoiceLimit v0x60 fields confirmed by reader 0x140d049b0;
// see docs/re/KONTAKT_ALLOCATION_M1.md. No library payloads are stored here.
#[test]
fn m1_exclusive_voice_group_translation_reaches_semantic_domain_without_diagnostic() {
    fn inline(exclusion: i32) -> Vec<u8> {
        let mut bytes = vec![0, 0x60, 0];
        bytes.extend(0u32.to_le_bytes()); // empty UTF-16 name
        bytes.extend(1i16.to_le_bytes()); // Oldest
        bytes.push(0); // preference off
        bytes.extend(16i32.to_le_bytes());
        bytes.extend(10i32.to_le_bytes());
        bytes.extend(exclusion.to_le_bytes());
        bytes
    }
    let mut data = inline(-1);
    let mut mask = [0; 16];
    mask[0] = 0b0000_0101; // physical voice groups 0 and 2
    data.extend(mask);
    data.extend(inline(0));
    data.extend(inline(0));
    let mut out = translation();
    out.voice_groups(&data).unwrap();
    assert_eq!(out.voice_groups[0], Some(0));
    assert_eq!(out.voice_groups[1], None);
    assert_eq!(out.voice_groups[2], Some(1));
    assert_eq!(out.ir.voice_limit.unwrap().exclusive_set, None);
    assert_eq!(out.ir.voice_limits[0].exclusive_set, Some(0));
    assert_eq!(out.ir.voice_limits[1].exclusive_set, Some(0));
    assert!(
        out.ir.unsupported.is_empty(),
        "exclusivity must now have a runtime consumer"
    );
    // Unknown positive values still have a diagnostic and no guessed consumer.
    let mut data = inline(-1);
    let mut mask = [0; 16];
    mask[0] = 1;
    data.extend(mask);
    data.extend(inline(16));
    let mut out = translation();
    out.voice_groups(&data).unwrap();
    assert_eq!(out.ir.voice_limits[0].exclusive_set, None);
    assert_eq!(out.ir.unsupported.len(), 1);
}
