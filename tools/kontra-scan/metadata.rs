//! Reuses the params auditor's chunk traversal; exports counts, never source or saved values.
use ni_file::kontakt::{
    Chunk, StructuredObject,
    objects::{BParScript, Bank},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub fn sigil(bytes: &[u8]) -> &'static str {
    match bytes.first() {
        Some(b'$') => "$",
        Some(b'~') => "~",
        Some(b'%') => "%",
        Some(b'?') => "?",
        Some(b'@') => "@",
        Some(b'!') => "!",
        None => "empty",
        _ => "other",
    }
}
// Bounded framing check: params() deliberately suppresses malformed saved tables.
fn table(data: &[u8]) -> (&'static str, BTreeMap<String, usize>) {
    fn word(d: &mut &[u8]) -> Option<usize> {
        let (a, b) = d.split_at_checked(4)?;
        *d = b;
        Some(u32::from_le_bytes(a.try_into().ok()?) as usize)
    }
    fn sized<'a>(d: &mut &'a [u8], optional: bool) -> Option<&'a [u8]> {
        let n = word(d)?;
        if optional && n == u32::MAX as usize {
            return Some(&[]);
        }
        let (a, b) = d.split_at_checked(n)?;
        *d = b;
        Some(a)
    }
    let mut d = data;
    let mut tags = BTreeMap::new();
    let prefix = (|| {
        sized(&mut d, true)?;
        d = d.get(3..)?;
        sized(&mut d, false)?;
        sized(&mut d, true)?;
        sized(&mut d, true)?;
        Some(())
    })();
    if prefix.is_none() {
        return ("unknown", tags);
    }
    if d.is_empty() {
        return ("absent", tags);
    }
    let framed = (|| {
        let n = word(&mut d)?;
        if n > d.len() / 4 {
            return None;
        }
        for _ in 0..n {
            *tags.entry(sigil(sized(&mut d, false)?).into()).or_default() += 1;
        }
        Some(())
    })();
    (
        if framed.is_some() {
            "decoded"
        } else {
            "malformed"
        },
        tags,
    )
}
pub fn inspect(chunks: &[Chunk]) -> Value {
    let mut out = Vec::new();
    let mut next = 0;
    for c in chunks {
        walk(c, "file", 0, &mut next, &mut out);
    }
    json!({"slots":out})
}

fn children(chunks: &[Chunk], owner: &str, program: u32, next: &mut u32, out: &mut Vec<Value>) {
    let mut slot = 0;
    for c in chunks {
        if c.id == 6 {
            let raw = BParScript::try_from(c);
            let (integrity, raw_tags) = raw
                .as_ref()
                .map(|r| table(&r.0.public_data))
                .unwrap_or(("unknown", BTreeMap::new()));
            match raw.and_then(|s|s.params()) {
                Ok(s) => {
                    let source_state=match s.text.as_deref(){None=>"absent",Some("")=>"zero_bytes",Some(t)if t.trim().is_empty()=>"whitespace_only",_=>"nonempty"};
                    let empty=source_state!="nonempty";
                    let linked=s.textfile_name.as_ref().is_some_and(|n|!n.is_empty());
                    let category=if s.bypass{"bypassed"}else if !empty{"inline_nonempty"}else if s.textfile_name.as_ref().is_some_and(|n|!n.trim().is_empty()){"linked_only"}else{"empty"};
                    let disposition=if s.bypass{"bypassed"}else if !empty{"embedded"}else if linked{"linked_unresolved"}else{"empty"};
                    let symbols=s.text.as_deref().map(super::symbols).unwrap_or_default();
                    out.push(json!({"owner":owner,"program_index":program,"slot":slot,"wire_slot":slot,"runtime_slot":null,"raw_category":category,"effective_source_kind":null,"bypassed":s.bypass,
                        "empty":empty,"source_state":source_state,"compile_disposition":disposition,"linked_file_present":linked,"symbols":symbols,"saved_sigils":raw_tags,"raw_saved_entries_by_sigil":raw_tags,"saved_entries_total":raw_tags.values().sum::<usize>(),"saved_table_integrity":integrity}));
                }
                Err(_) => out.push(json!({"owner":owner,"program_index":program,"slot":slot,"wire_slot":slot,"runtime_slot":null,"raw_category":"decode_failed","error":"script record parse"})),
            }
            slot += 1;
        } else {
            walk(c, owner, program, next, out);
        }
    }
}

fn walk(c: &Chunk, owner: &str, program: u32, next: &mut u32, out: &mut Vec<Value>) {
    match c.id {
        0x28 | 0x29 => {
            if let Ok(o) = StructuredObject::try_from(c) {
                let (owner, program) = if c.id == 0x28 {
                    let n = *next;
                    *next += 1;
                    (
                        if owner == "file" {
                            "standalone-program"
                        } else {
                            "embedded-program"
                        },
                        n,
                    )
                } else {
                    ("bank-container", program)
                };
                children(&o.children, owner, program, next, out);
            }
        }
        3 => {
            if let Ok(bank) = Bank::try_from(c) {
                children(&bank.0.children, "bank-global", program, next, out);
                if let Ok(list) = bank.slot_list() {
                    for (_, slot) in list.slots {
                        if let Ok(list) = slot.program_list() {
                            for p in list.programs {
                                let n = *next;
                                *next += 1;
                                children(&p.0.children, "embedded-program", n, next, out);
                            }
                        }
                    }
                }
            }
        }
        6 => children(std::slice::from_ref(c), owner, program, next, out),
        _ => (),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn scanner_counts_fixed_sigils_and_saved_framing() {
        let mut d = Vec::new();
        d.extend(u32::MAX.to_le_bytes());
        d.extend([0; 3]);
        d.extend(0u32.to_le_bytes());
        d.extend(u32::MAX.to_le_bytes());
        d.extend(u32::MAX.to_le_bytes());
        assert_eq!(super::table(&d).0, "absent");
        d.extend(3u32.to_le_bytes());
        for e in [b"!private payload".as_slice(), b"$private 1", b" private"] {
            d.extend((e.len() as u32).to_le_bytes());
            d.extend(e);
        }
        let (integrity, tags) = super::table(&d);
        assert_eq!(integrity, "decoded");
        assert_eq!(tags.get("!"), Some(&1));
        assert_eq!(tags.get("other"), Some(&1));
        assert!(!format!("{tags:?}").contains("private"));
        d.pop();
        assert_eq!(super::table(&d).0, "malformed");
    }
}
