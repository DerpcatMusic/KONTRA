//! FastLZ level 1 and 2, checked: every read and back reference is bounded and
//! the output size is exact. Wire rules reviewed against Ariya Hidayat's
//! MIT-licensed reference (b1342dabcf5257ab303743c9332fe75e9147a011).

/// Expands `input` into exactly `expected` bytes.
pub(crate) fn expand(input: &[u8], expected: usize) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    out.try_reserve_exact(expected)
        .map_err(|_| "preset too large")?;
    let mut bytes = input.iter().copied();
    let first = next(&mut bytes)?;
    let level = first >> 5;
    if level > 1 {
        return Err("unknown FastLZ level");
    }
    let mut control = first & 31;
    loop {
        if control < 32 {
            for _ in 0..=control {
                out.push(next(&mut bytes)?);
            }
        } else {
            let mut count = usize::from(control >> 5) + 2;
            if control >> 5 == 7 {
                loop {
                    let extra = next(&mut bytes)?;
                    count += usize::from(extra);
                    if level == 0 || extra != 255 {
                        break;
                    }
                }
            }
            let low = next(&mut bytes)?;
            let mut distance = (usize::from(control & 31) << 8) + usize::from(low) + 1;
            if level == 1 && control & 31 == 31 && low == 255 {
                distance =
                    8192 + (usize::from(next(&mut bytes)?) << 8) + usize::from(next(&mut bytes)?);
            }
            let from = out
                .len()
                .checked_sub(distance)
                .ok_or("FastLZ reference before start")?;
            // Forward order: a run may overlap what it is writing.
            for i in from..from + count {
                let byte = *out.get(i).ok_or("FastLZ reference before start")?;
                out.push(byte);
            }
        }
        if out.len() > expected {
            return Err("FastLZ output longer than declared");
        }
        match bytes.next() {
            Some(c) => control = c,
            None => break,
        }
    }
    if out.len() == expected {
        Ok(out)
    } else {
        Err("FastLZ output shorter than declared")
    }
}

fn next(bytes: &mut impl Iterator<Item = u8>) -> Result<u8, &'static str> {
    bytes.next().ok_or("truncated FastLZ stream")
}

#[cfg(test)]
mod tests {
    #[test]
    fn literals_and_an_overlapping_run_expand_exactly() {
        // "abc", then copy 6 bytes from distance 3: "abcabcabc".
        let input = [2, b'a', b'b', b'c', (4 << 5), 2];
        assert_eq!(super::expand(&input, 9).unwrap(), b"abcabcabc");
        assert!(super::expand(&input, 10).is_err());
        assert!(
            super::expand(&[0, b'a', 0x20, 5], 4).is_err(),
            "reference before start"
        );
    }
}
