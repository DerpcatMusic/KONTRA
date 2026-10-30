//! Skreamer (module 0x21) onto the engine's overdrive: its saved record
//! (setter FUN_140968150: tone, drive, bass, bright, mix at 0, 4, 8, 12,
//! 16) and the coefficient laws its core (FUN_140aaf220, audio 0x140aa5810)
//! reads, each a pitch index into the exponential table `2^(i/60 − 20)`
//! times `w = f32::from_bits(0x40c90e56)` over the rate.

use kontra_ir as ir;
use kontra_law::kontakt::PITCH_INDEX;

/// The table entry for index `i`.
fn table(i: f32) -> f32 {
    PITCH_INDEX.eval_unclamped(i)
}

/// Corners from the native coefficient, which multiplies by `w` where the
/// engine multiplies by 2π.
fn hz(coefficient_over_w: f32) -> f32 {
    coefficient_over_w * f32::from_bits(0x40c9_0e56) / std::f32::consts::TAU
}

/// The knobs, each 0..=1, as the engine's overdrive.
pub(crate) fn skreamer(tone: f32, drive: f32, bass: f32, bright: f32, mix: f32) -> ir::EffectKind {
    let [tone, drive, bass, bright, mix] =
        [tone, drive, bass, bright, mix].map(|x| x.clamp(0.0, 1.0));
    ir::EffectKind::Overdrive(ir::Overdrive {
        split_hz: [
            hz(table(570.0 - 400.0 * bass + 1_381.881_6)),
            129.429_81 / std::f32::consts::TAU,
            28.901_8 / std::f32::consts::TAU,
        ],
        smooth_hz: [
            50.264 / std::f32::consts::TAU,
            hz(table(200.0 * bright + 650.0 - 200.0 * drive + 1_381.881_6)),
            hz(table(tone * tone * 225.0 + 1_716.881_6)),
        ],
        drive: table(199.315_67 * drive + 1_550.795_7) * 4.0,
        mix: table(398.631_35 * mix + 1_070.444_8) * 0.5,
        level: 0.447,
        curve: &CURVE,
    })
}

const Q: f32 = 32_768.0;

/// The clipping curve's values (the table at 0x144f3a100 interleaves each
/// with its forward difference), exact in 32768ths.
#[rustfmt::skip]
static CURVE: [f32; 256] = [
    -21830.0 / Q, -21830.0 / Q, -21813.0 / Q, -21795.0 / Q, -21777.0 / Q, -21758.0 / Q, -21741.0 / Q, -21724.0 / Q,
    -21708.0 / Q, -21690.0 / Q, -21669.0 / Q, -21648.0 / Q, -21627.0 / Q, -21607.0 / Q, -21588.0 / Q, -21569.0 / Q,
    -21550.0 / Q, -21530.0 / Q, -21508.0 / Q, -21486.0 / Q, -21463.0 / Q, -21441.0 / Q, -21420.0 / Q, -21399.0 / Q,
    -21379.0 / Q, -21358.0 / Q, -21337.0 / Q, -21314.0 / Q, -21290.0 / Q, -21265.0 / Q, -21240.0 / Q, -21215.0 / Q,
    -21191.0 / Q, -21167.0 / Q, -21144.0 / Q, -21122.0 / Q, -21098.0 / Q, -21074.0 / Q, -21049.0 / Q, -21022.0 / Q,
    -20994.0 / Q, -20965.0 / Q, -20937.0 / Q, -20908.0 / Q, -20880.0 / Q, -20853.0 / Q, -20826.0 / Q, -20800.0 / Q,
    -20773.0 / Q, -20746.0 / Q, -20717.0 / Q, -20687.0 / Q, -20655.0 / Q, -20622.0 / Q, -20588.0 / Q, -20553.0 / Q,
    -20518.0 / Q, -20484.0 / Q, -20450.0 / Q, -20417.0 / Q, -20384.0 / Q, -20352.0 / Q, -20319.0 / Q, -20284.0 / Q,
    -20249.0 / Q, -20211.0 / Q, -20172.0 / Q, -20130.0 / Q, -20087.0 / Q, -20042.0 / Q, -19997.0 / Q, -19951.0 / Q,
    -19906.0 / Q, -19861.0 / Q, -19817.0 / Q, -19774.0 / Q, -19730.0 / Q, -19685.0 / Q, -19639.0 / Q, -19590.0 / Q,
    -19539.0 / Q, -19485.0 / Q, -19427.0 / Q, -19366.0 / Q, -19302.0 / Q, -19236.0 / Q, -19168.0 / Q, -19100.0 / Q,
    -19031.0 / Q, -18963.0 / Q, -18894.0 / Q, -18826.0 / Q, -18756.0 / Q, -18684.0 / Q, -18609.0 / Q, -18529.0 / Q,
    -18442.0 / Q, -18348.0 / Q, -18246.0 / Q, -18135.0 / Q, -18016.0 / Q, -17889.0 / Q, -17755.0 / Q, -17616.0 / Q,
    -17472.0 / Q, -17325.0 / Q, -17173.0 / Q, -17016.0 / Q, -16850.0 / Q, -16671.0 / Q, -16473.0 / Q, -16247.0 / Q,
    -15983.0 / Q, -15671.0 / Q, -15298.0 / Q, -14852.0 / Q, -14320.0 / Q, -13691.0 / Q, -12956.0 / Q, -12108.0 / Q,
    -11143.0 / Q, -10061.0 / Q, -8866.0 / Q, -7565.0 / Q, -6170.0 / Q, -4697.0 / Q, -3163.0 / Q, -1590.0 / Q,
    0.0 / Q, 1584.0 / Q, 3140.0 / Q, 4645.0 / Q, 6080.0 / Q, 7429.0 / Q, 8678.0 / Q, 9817.0 / Q,
    10842.0 / Q, 11751.0 / Q, 12545.0 / Q, 13230.0 / Q, 13815.0 / Q, 14309.0 / Q, 14724.0 / Q, 15072.0 / Q,
    15366.0 / Q, 15617.0 / Q, 15835.0 / Q, 16030.0 / Q, 16207.0 / Q, 16374.0 / Q, 16533.0 / Q, 16687.0 / Q,
    16837.0 / Q, 16982.0 / Q, 17121.0 / Q, 17255.0 / Q, 17381.0 / Q, 17500.0 / Q, 17610.0 / Q, 17712.0 / Q,
    17806.0 / Q, 17892.0 / Q, 17974.0 / Q, 18051.0 / Q, 18124.0 / Q, 18196.0 / Q, 18267.0 / Q, 18338.0 / Q,
    18409.0 / Q, 18479.0 / Q, 18549.0 / Q, 18618.0 / Q, 18685.0 / Q, 18749.0 / Q, 18811.0 / Q, 18869.0 / Q,
    18925.0 / Q, 18977.0 / Q, 19027.0 / Q, 19074.0 / Q, 19121.0 / Q, 19166.0 / Q, 19212.0 / Q, 19258.0 / Q,
    19304.0 / Q, 19351.0 / Q, 19397.0 / Q, 19443.0 / Q, 19489.0 / Q, 19533.0 / Q, 19575.0 / Q, 19616.0 / Q,
    19654.0 / Q, 19691.0 / Q, 19727.0 / Q, 19761.0 / Q, 19796.0 / Q, 19830.0 / Q, 19864.0 / Q, 19899.0 / Q,
    19935.0 / Q, 19970.0 / Q, 20006.0 / Q, 20041.0 / Q, 20075.0 / Q, 20107.0 / Q, 20138.0 / Q, 20168.0 / Q,
    20197.0 / Q, 20225.0 / Q, 20253.0 / Q, 20281.0 / Q, 20309.0 / Q, 20338.0 / Q, 20368.0 / Q, 20397.0 / Q,
    20427.0 / Q, 20455.0 / Q, 20483.0 / Q, 20509.0 / Q, 20535.0 / Q, 20559.0 / Q, 20583.0 / Q, 20607.0 / Q,
    20632.0 / Q, 20657.0 / Q, 20682.0 / Q, 20708.0 / Q, 20733.0 / Q, 20758.0 / Q, 20782.0 / Q, 20805.0 / Q,
    20827.0 / Q, 20848.0 / Q, 20870.0 / Q, 20892.0 / Q, 20915.0 / Q, 20938.0 / Q, 20961.0 / Q, 20984.0 / Q,
    21005.0 / Q, 21026.0 / Q, 21046.0 / Q, 21066.0 / Q, 21087.0 / Q, 21108.0 / Q, 21130.0 / Q, 21151.0 / Q,
    21170.0 / Q, 21188.0 / Q, 21205.0 / Q, 21223.0 / Q, 21242.0 / Q, 21261.0 / Q, 21278.0 / Q, 21278.0 / Q,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_index_preserves_unclamped_table_calculation() {
        for n in 0..=10_000 {
            let index = 1_000.0 + n as f32 / 10.0;
            assert_eq!(table(index), (index / 60.0 - 20.0).exp2(), "index {index}");
        }
    }

    #[test]
    fn the_laws_land_on_the_cores_coefficients() {
        let ir::EffectKind::Overdrive(o) = skreamer(0.5, 0.5, 0.5, 0.5, 0.5) else {
            panic!("an overdrive");
        };
        // 48 kHz coefficients, as the core computes them.
        let k = |hz: f32| std::f32::consts::TAU * hz / 48_000.0;
        let w = f32::from_bits(0x40c9_0e56) / 48_000.0;
        assert!((k(o.split_hz[0]) - table(1_751.881_6) * w).abs() < 1e-7);
        assert!((k(o.split_hz[1]) - 129.429_81 / 48_000.0).abs() < 1e-7);
        assert!((k(o.smooth_hz[2]) - table(56.25 + 1_716.881_6) * w).abs() < 1e-7);
        assert!((o.drive - table(1_650.453_5) * 4.0).abs() < 1e-6);
        assert_eq!(o.level, 0.447);
        // Every point of the curve table 0x144f3a100, against v2's dump of it.
        let bytes = include_bytes!("../../../dsp/drive/tests/fixtures/skreamer.f32le");
        for (c, b) in o.curve.iter().zip(bytes.as_chunks::<8>().0) {
            assert_eq!(*c, f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        }
    }
}
