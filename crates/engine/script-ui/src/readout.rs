//! A knob's text without its unit, as Kontakt 8.13.1 printed it to oracle
//! probes (2026-10-29); pan follows the binary's formatter at 0x1407b7ac0
//! (f32, Center inside ±0.0075, else half up).

use kontra_vm::Text;

/// What a knob's text shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Readout {
    /// A gain topping out at this many dB: 6 dB per doubling of the cubed knob.
    Db(f64),
    /// Padded to five places, thousands as `k`: a time in ms, or a route's
    /// intensity in % (oracle 2026-10-29: 0.5 reads " 25.0", 0 " 0.00").
    Padded(f64),
    /// One decimal, signed: a bipolar route depth in % (oracle 2026-10-29 on
    /// Morphology's routes: depth 100·(2x − 1)³ reads "-0.8" at 0.4; a
    /// route's intensity is 100·x², [`Readout::Padded`]).
    Signed(f64),
    /// Five places, one decimal (a phase in degrees).
    Wide(f64),
    /// A whole number (an attack curve, −100..=100).
    Whole(f64),
    /// `1:` and a ratio in four places.
    Ratio(f64),
    /// ±36 semitones.
    Tune,
    Pan,
}

/// Writes `readout` of a knob at `x` (0..=1).
pub fn write(readout: Readout, x: f64, out: &mut Text) -> bool {
    let pan = 2.0 * (x as f32) - 1.0;
    match readout {
        Readout::Db(top) if x > 0.0 => out.push_fmt(format_args!("{:.1}", 18.0 * x.log2() + top)),
        Readout::Db(_) => out.push("-inf"),
        Readout::Padded(ms) if ms.abs() < 1.0 => out.push_fmt(format_args!("{ms:5.2}")),
        Readout::Padded(ms) if ms < 1000.0 => out.push_fmt(format_args!("{ms:5.1}")),
        Readout::Padded(ms) => out.push_fmt(format_args!("{:5.1}k", ms / 1000.0)),
        Readout::Signed(v) => out.push_fmt(format_args!("{v:.1}")),
        Readout::Wide(v) => out.push_fmt(format_args!("{v:5.1}")),
        Readout::Whole(v) => out.push_fmt(format_args!("{v:.0}")),
        Readout::Ratio(v) => out.push_fmt(format_args!("1:{v:4.1}")),
        Readout::Tune => out.push_fmt(format_args!("{:.2}", (2.0 * x - 1.0) * 36.0)),
        Readout::Pan if f64::from(pan.abs()) < 0.0075 => out.push("Center"),
        Readout::Pan => {
            let side = if pan < 0.0 { "L" } else { "R" };
            out.push_fmt(format_args!("{} {side}", (pan.abs() * 100.0 + 0.5) as i32))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reverb time at 0, 0.1, 0.2 and 1 through its law, 0.5·e^(3.7x) s
    /// (oracle 2026-10-29, Areia's send reverb).
    #[test]
    fn a_time_reads_in_ms() {
        let shown: Vec<String> = [0.0, 0.1, 0.2, 1.0]
            .iter()
            .map(|x: &f64| {
                let mut t = Text::EMPTY;
                write(Readout::Padded(500.0 * (3.7 * x).exp()), 0.0, &mut t);
                t.as_str().to_owned()
            })
            .collect();
        assert_eq!(shown, ["500.0", "723.9", "  1.0k", " 20.2k"]);
    }
}
