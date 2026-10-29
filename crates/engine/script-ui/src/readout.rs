//! A knob's text without its unit, as Kontakt 8.13.1 printed it to oracle
//! probes (2026-10-29); pan follows the binary's formatter at 0x1407b7ac0
//! (f32, Center inside ±0.0075, else half up).

use kontra_vm::Text;

/// What a knob's text shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Readout {
    /// A gain topping out at this many dB: 6 dB per doubling of the cubed knob.
    Db(f64),
    /// A time in ms, thousands as `k`.
    Ms(f64),
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
        Readout::Ms(ms) if ms < 1.0 => out.push_fmt(format_args!("{ms:5.2}")),
        Readout::Ms(ms) if ms < 1000.0 => out.push_fmt(format_args!("{ms:5.1}")),
        Readout::Ms(ms) => out.push_fmt(format_args!("{:5.1}k", ms / 1000.0)),
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
                write(Readout::Ms(500.0 * (3.7 * x).exp()), 0.0, &mut t);
                t.as_str().to_owned()
            })
            .collect();
        assert_eq!(shown, ["500.0", "723.9", "  1.0k", " 20.2k"]);
    }
}
