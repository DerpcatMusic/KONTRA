//! Fixed-capacity text: script strings never allocate on the audio thread.

use std::fmt;

/// Bytes a text value holds; longer results are cut and counted.
pub const TEXT_CAPACITY: usize = 320;

#[derive(Clone, Copy)]
pub struct Text {
    len: u16,
    bytes: [u8; TEXT_CAPACITY],
}

impl Text {
    pub const EMPTY: Self = Self {
        len: 0,
        bytes: [0; TEXT_CAPACITY],
    };

    /// Text from `s`, cut at a character boundary if too long. Returns whether it was cut.
    pub fn from_str_cut(s: &str) -> (Self, bool) {
        let mut t = Self::EMPTY;
        let cut = !t.push(s);
        (t, cut)
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        let bytes = self.bytes.get(..usize::from(self.len)).unwrap_or_default();
        // Only whole characters are ever pushed, so this never fails.
        std::str::from_utf8(bytes).unwrap_or_default()
    }

    /// Appends as much of `s` as fits, at a character boundary. False if cut.
    pub fn push(&mut self, s: &str) -> bool {
        let room = TEXT_CAPACITY - usize::from(self.len);
        let mut take = s.len().min(room);
        while !s.is_char_boundary(take) {
            take -= 1;
        }
        let start = usize::from(self.len);
        if let (Some(dst), Some(src)) = (
            self.bytes.get_mut(start..start + take),
            s.as_bytes().get(..take),
        ) {
            dst.copy_from_slice(src);
        }
        self.len += take as u16;
        take == s.len()
    }

    /// Appends a formatted value without allocating. False if cut.
    pub fn push_fmt(&mut self, args: fmt::Arguments<'_>) -> bool {
        struct Sink<'a>(&'a mut Text, bool);
        impl fmt::Write for Sink<'_> {
            fn write_str(&mut self, s: &str) -> fmt::Result {
                self.1 &= self.0.push(s);
                Ok(())
            }
        }
        let mut sink = Sink(self, true);
        let _ = fmt::write(&mut sink, args);
        sink.1
    }
}

impl PartialEq for Text {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl fmt::Debug for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_cuts_at_a_character_boundary() {
        let long = "é".repeat(TEXT_CAPACITY);
        let (t, cut) = Text::from_str_cut(&long);
        assert!(cut);
        assert_eq!(t.as_str().len(), TEXT_CAPACITY);
        assert!(t.as_str().chars().all(|c| c == 'é'));
    }

    #[test]
    fn formatted_values_append_without_allocating() {
        let mut t = Text::from_str_cut("gain ").0;
        assert!(t.push_fmt(format_args!("{}", -12)));
        assert_eq!(t.as_str(), "gain -12");
    }
}
