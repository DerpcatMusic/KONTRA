//! Typed views of Kontakt's length-prefixed saved-variable text records.
//! Declaration context is required: `$menu 2` saves a menu index, while `$x 2`
//! saves an integer. No restoration or declaration inference happens here.
use crate::{Bytes, Error, ErrorKind, Limits};
use sampler_ksp::model::WidgetKind;
use std::{marker::PhantomData, str::FromStr};

/// How a saved numeric array relates to its declared dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayTail {
    /// Ordinary arrays omit repeated trailing cells; fill with the last value.
    RepeatLast,
    /// UI tables and XY pads serialize every cell.
    Exact,
}

/// A validated numeric list. Iteration parses values without allocating.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedNumbers<'a, T> {
    text: &'a str,
    count: usize,
    marker: PhantomData<T>,
}
impl<T: FromStr> SavedNumbers<'_, T> {
    pub fn len(&self) -> usize {
        self.count
    }
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }
    pub fn iter(&self) -> impl Iterator<Item = T> + '_ {
        self.text
            .split_ascii_whitespace()
            .map(|s| s.parse().ok().expect("validated saved number"))
    }
}

/// LF-separated string-array cells. Spaces, CRs and non-UTF8 bytes stay intact.
/// A final LF terminates the last cell; it does not add another empty cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SavedTexts<'a> {
    bytes: &'a [u8],
    count: usize,
}
impl<'a> SavedTexts<'a> {
    pub fn len(self) -> usize {
        self.count
    }
    pub fn is_empty(self) -> bool {
        self.count == 0
    }
    pub fn iter(self) -> impl Iterator<Item = &'a [u8]> {
        self.bytes.split(|b| *b == b'\n').take(self.count)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SavedValue<'a> {
    Int(i32),
    Real(f64),
    /// A zero-based position in the menu's item list, NOT its KSP item value.
    MenuIndex(i32),
    Text(&'a [u8]),
    Ints {
        values: SavedNumbers<'a, i32>,
        tail: ArrayTail,
    },
    Reals {
        values: SavedNumbers<'a, f64>,
        tail: ArrayTail,
    },
    Texts(SavedTexts<'a>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SavedEntry<'a> {
    /// Original sigil and spelling; script slot supplies the remaining identity.
    pub name: &'a str,
    pub value: SavedValue<'a>,
    pub raw: Bytes<'a>,
}
impl<'a> SavedEntry<'a> {
    /// Decode one entry (without its u32 length). `None` means a plain variable;
    /// UI entries require the owning declaration's existing KSP widget kind.
    /// Persistence scope is a script property, not a saved-entry field.
    /// Errors never expose saved text.
    pub fn parse(
        bytes: &'a [u8],
        widget: Option<WidgetKind>,
        limits: Limits,
    ) -> Result<Self, Error> {
        Self::from_bytes(
            Bytes {
                data: bytes,
                offset: 0,
            },
            widget,
            limits,
        )
    }
    /// Decode a borrowed script-table entry, retaining absolute source offsets.
    pub fn from_bytes(
        raw: Bytes<'a>,
        widget: Option<WidgetKind>,
        limits: Limits,
    ) -> Result<Self, Error> {
        use WidgetKind::*;
        if raw.data.len() > limits.bytes {
            return Err(raw.error(ErrorKind::Limit));
        }
        let invalid = || raw.error(ErrorKind::InvalidSavedValue);
        if raw.data.contains(&0) {
            return Err(invalid());
        }
        let space = raw
            .data
            .iter()
            .position(|b| *b == b' ')
            .ok_or_else(invalid)?;
        let name = std::str::from_utf8(&raw.data[..space]).map_err(|_| invalid())?;
        let payload = &raw.data[space + 1..];
        let sigil = *name.as_bytes().first().ok_or_else(invalid)?;
        if name.len() < 2
            || !name.as_bytes()[1..]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            return Err(invalid());
        }
        let expected = match widget {
            None => sigil,
            Some(TextEdit) => b'@',
            Some(Table) => b'%',
            Some(Xy) => b'?',
            Some(Label | FileSelector | LevelMeter | Panel | MouseArea) => {
                return Err(raw.error(ErrorKind::UnsupportedLayout));
            }
            _ => b'$',
        };
        if sigil != expected {
            return Err(invalid());
        }
        let text = || std::str::from_utf8(payload).map_err(|_| invalid());
        let tail = if widget.is_none() {
            ArrayTail::RepeatLast
        } else {
            ArrayTail::Exact
        };
        let value = match sigil {
            b'$' => {
                let n = text()?.trim_ascii().parse().map_err(|_| invalid())?;
                if widget == Some(Menu) {
                    SavedValue::MenuIndex(n)
                } else {
                    SavedValue::Int(n)
                }
            }
            b'~' => SavedValue::Real(text()?.trim_ascii().parse().map_err(|_| invalid())?),
            b'@' => SavedValue::Text(payload),
            b'%' => SavedValue::Ints {
                values: numbers(text()?, raw, limits)?,
                tail,
            },
            b'?' => SavedValue::Reals {
                values: numbers(text()?, raw, limits)?,
                tail,
            },
            b'!' => {
                let count = if payload.is_empty() {
                    0
                } else {
                    payload.iter().filter(|b| **b == b'\n').count()
                        + usize::from(!payload.ends_with(b"\n"))
                };
                if count > limits.records {
                    return Err(raw.error(ErrorKind::Limit));
                }
                SavedValue::Texts(SavedTexts {
                    bytes: payload,
                    count,
                })
            }
            _ => return Err(raw.error(ErrorKind::UnsupportedLayout)),
        };
        Ok(Self { name, value, raw })
    }
}
fn numbers<'a, T: FromStr>(
    text: &'a str,
    raw: Bytes<'_>,
    limits: Limits,
) -> Result<SavedNumbers<'a, T>, Error> {
    let mut count = 0;
    for token in text.split_ascii_whitespace() {
        count += 1;
        if count > limits.records {
            return Err(raw.error(ErrorKind::Limit));
        }
        token
            .parse::<T>()
            .map_err(|_| raw.error(ErrorKind::InvalidSavedValue))?;
    }
    if count == 0 {
        return Err(raw.error(ErrorKind::InvalidSavedValue));
    }
    Ok(SavedNumbers {
        text,
        count,
        marker: PhantomData,
    })
}
