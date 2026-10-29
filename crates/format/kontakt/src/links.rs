//! MIDI automation: the controllers an instrument ties to its script
//! controls (Kontakt's Automation tab, "Learn MIDI CC# Automation").
//!
//! Each assignment is a BAutomationObject in the program's private data.
//! Its reader (Kontakt 8.13 FUN_140cfec70) takes, after the object's
//! structured flag and version: the source kind (FUN_140cf0360, 0..=2), a
//! byte, (0x70 only: a byte), the controller (u16), (0x71 only: an i32),
//! an i32, the range's from and to (f32), and the parameter's name
//! (FUN_140cf0710), which for a script control is
//! `pts_script_<knob|slider|switch|xy>_<slot>_<n>`: the slot's `n`th
//! declared control of that kind. Dolce's Legato ties CC 1 to
//! `pts_script_slider_2_40` (its Dynamics, slot 2's 41st slider) and
//! CC 11 to `_2_42` (Expression); in Kontakt CC 1 at 0 turns that knob to 0
//! and the sound drops 18.9 dB.

use kontra_ir as ir;

/// Source kind 1: a MIDI controller (every assignment in the corpus).
const MIDI: i32 = 1;

/// The script-control assignments in a program's private data, each with
/// its script slot.
// ponytail: finds the objects by their own framing instead of decoding
// the program's private fields before them; decode those when another
// field there is needed.
pub(crate) fn links(private: &[u8]) -> Vec<(u8, ir::CcLink)> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < private.len() {
        match record(private.get(at..).unwrap_or_default()) {
            Some((len, link)) => {
                out.extend(link);
                at += len;
            }
            None => at += 1,
        }
    }
    out
}

/// One BAutomationObject at the start of `b`: its length, and the link
/// when it is a MIDI controller on a script control.
fn record(b: &[u8]) -> Option<(usize, Option<(u8, ir::CcLink)>)> {
    let mut r = Reader(b, 0);
    let version = match (r.u8()?, r.u16()?) {
        (0, v @ (0x70 | 0x71)) => v,
        _ => return None,
    };
    let kind = r.i32()?;
    if !(0..=2).contains(&kind) {
        return None;
    }
    r.u8()?;
    if version == 0x70 {
        r.u8()?;
    }
    let cc = r.u16()?;
    if version == 0x71 {
        r.i32()?;
    }
    r.i32()?;
    let (from, to) = (r.f32()?, r.f32()?);
    let len = usize::try_from(r.u32()?).ok().filter(|&n| n <= 256)?;
    let name = std::str::from_utf8(r.bytes(len)?).ok()?;
    if !name.bytes().all(|c| c.is_ascii_graphic()) {
        return None;
    }
    let link = (kind == MIDI)
        .then(|| script_control(name))
        .flatten()
        .and_then(|(slot, control)| {
            Some((
                slot,
                ir::CcLink {
                    cc: u8::try_from(cc).ok().filter(|&c| c < 128)?,
                    control,
                    from,
                    to,
                },
            ))
        });
    Some((r.1, link))
}

/// `pts_script_slider_2_40` → slot 2, the 41st slider.
fn script_control(name: &str) -> Option<(u8, ir::Linked)> {
    let rest = name.strip_prefix("pts_script_")?;
    let (kind, rest) = rest.split_once('_')?;
    let (slot, n) = rest.split_once('_')?;
    let n = n.parse().ok()?;
    let control = match kind {
        "knob" => ir::Linked::Knob(n),
        "slider" => ir::Linked::Slider(n),
        "switch" => ir::Linked::Switch(n),
        // ponytail: an XY pad's two axes; none in the corpus.
        _ => return None,
    };
    Some((slot.parse().ok()?, control))
}

struct Reader<'a>(&'a [u8], usize);

impl Reader<'_> {
    fn bytes(&mut self, n: usize) -> Option<&[u8]> {
        let b = self.0.get(self.1..self.1.checked_add(n)?)?;
        self.1 += n;
        Some(b)
    }
    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        self.bytes(N)?.try_into().ok()
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.array::<1>()?[0])
    }
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_le_bytes(self.array()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.array()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.array()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.array()?))
    }
}

#[cfg(test)]
mod tests {
    /// Dolce Legato's first two assignments, as stored, after unrelated bytes.
    #[test]
    fn dolces_wheel_and_expression_move_its_dynamics_and_expression_sliders() {
        let mut b = vec![0x05, 0, 0, 0, 0x05, 0, 0, 0];
        for (cc, n) in [(1u8, "40"), (11, "42")] {
            b.extend([0, 0x71, 0, 1, 0, 0, 0, 0, cc, 0, 2, 0, 0, 0]);
            b.extend([0xff; 4]);
            b.extend(0f32.to_le_bytes());
            b.extend(1f32.to_le_bytes());
            let name = format!("pts_script_slider_2_{n}");
            b.extend(u32::try_from(name.len()).unwrap_or(0).to_le_bytes());
            b.extend(name.bytes());
        }
        let links = super::links(&b);
        let got: Vec<_> = links
            .iter()
            .map(|(s, l)| (*s, l.cc, l.control, l.from, l.to))
            .collect();
        use kontra_ir::Linked::Slider;
        assert_eq!(
            got,
            [(2, 1, Slider(40), 0.0, 1.0), (2, 11, Slider(42), 0.0, 1.0)]
        );
    }
}
