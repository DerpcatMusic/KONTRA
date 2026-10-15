//! Authored scalar/vector oracles from the complete original decoded functions.
//!
//! Source image c997d9ce5ca6619a74d159a3424003cb5f76f7c51825f2234b2d057ee1ac9925;
//! caller-reviewed private profile 34edda2b4916b75f22822e4dc9af8f7d0b6fd9a282097889099dd2f08909dc25.
//! Generator case 2d1a7f66b61b35cdbb9d1d45f5f3f8c30a3d4397ef39152d4318cca15cb808a1;
//! math case 78f8f4c7c2d445c3420d66dc0495d7ed66c1527dec55bbb58e95360005dba994.
//! The explicit table is authored. No vendor tables, code or app defaults are included.

use super::*;

fn ramp() -> [f32; 257] {
    std::array::from_fn(|i| {
        if i == 256 {
            -1.0
        } else {
            (i as f32 - 128.0) / 128.0
        }
    })
}
fn bits(points: Points) -> [u32; 3] {
    points.values.map(f32::to_bits)
}

struct Case {
    shape: u8,
    rate: f32,
    count: u32,
    len: usize,
    bits: [u32; 3],
    xyz: [u32; 3],
    phase: u32,
}
const NATIVE: [Case; 42] = [
    Case {
        shape: 7,
        rate: 44100.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 1,
        len: 2,
        bits: [1028730567, 1028731342, 0],
        xyz: [1065353216, 1065371478, 1065350876],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 31,
        len: 2,
        bits: [1028731342, 1028732141, 0],
        xyz: [1065353690, 1065937550, 1065278437],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 32,
        len: 2,
        bits: [1028756908, 1028756908, 0],
        xyz: [1065369334, 1066520497, 1065207215],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 33,
        len: 3,
        bits: [1028807314, 1028809645, 1028882594],
        xyz: [1065401604, 1067121340, 1065137731],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 63,
        len: 3,
        bits: [1028884947, 1028980691, 1028984476],
        xyz: [1065506268, 1068271771, 1065016385],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 44100.0,
        count: 64,
        len: 3,
        bits: [1029101785, 1029245384, 1029245384],
        xyz: [1065668235, 1069451817, 1064909077],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 1,
        len: 2,
        bits: [1028730567, 1028731221, 0],
        xyz: [1065353216, 1065369994, 1065351066],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 31,
        len: 2,
        bits: [1028731221, 1028731896, 0],
        xyz: [1065353616, 1065890076, 1065284504],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 32,
        len: 2,
        bits: [1028752802, 1028752802, 0],
        xyz: [1065366822, 1066425763, 1065218795],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 33,
        len: 3,
        bits: [1028795404, 1028797375, 1028859097],
        xyz: [1065394096, 1066977834, 1065154373],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 63,
        len: 3,
        bits: [1028861087, 1028942173, 1028945381],
        xyz: [1065482699, 1068034115, 1065040880],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 48000.0,
        count: 64,
        len: 3,
        bits: [1029044815, 1029166626, 1029166626],
        xyz: [1065620043, 1069115621, 1064938918],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 1,
        len: 2,
        bits: [1028730567, 1028730730, 0],
        xyz: [1065353216, 1065361605, 1065352141],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 31,
        len: 2,
        bits: [1028730730, 1028730899, 0],
        xyz: [1065353316, 1065621655, 1065318837],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 32,
        len: 2,
        bits: [1028736128, 1028736128, 0],
        xyz: [1065356619, 1065889798, 1065285208],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 33,
        len: 3,
        bits: [1028746854, 1028747354, 1028762994],
        xyz: [1065363488, 1066166111, 1065251356],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 63,
        len: 3,
        bits: [1028763499, 1028784165, 1028784987],
        xyz: [1065386013, 1066693463, 1065189046],
        phase: 4294967040,
    },
    Case {
        shape: 7,
        rate: 96000.0,
        count: 64,
        len: 3,
        bits: [1028810463, 1028841829, 1028841829],
        xyz: [1065421298, 1067229668, 1065128913],
        phase: 4294967040,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 1,
        len: 2,
        bits: [3212836354, 3212580320, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 1025930,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 31,
        len: 2,
        bits: [3212828849, 3212572302, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 32837696,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 32,
        len: 2,
        bits: [3212580320, 3212323773, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 65675648,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 33,
        len: 3,
        bits: [3212323773, 3212067226, 3211810680],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 99539786,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 63,
        len: 3,
        bits: [3212059210, 3211802662, 3211546116],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 164189504,
    },
    Case {
        shape: 9,
        rate: 44100.0,
        count: 64,
        len: 3,
        bits: [3211554134, 3211297587, 3211041040],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 229865408,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 1,
        len: 2,
        bits: [3212836354, 3212601164, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 942553,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 31,
        len: 2,
        bits: [3212829500, 3212593798, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 30169632,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 32,
        len: 2,
        bits: [3212601164, 3212365462, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 60339520,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 33,
        len: 3,
        bits: [3212365462, 3212129760, 3211894057],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 91452217,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 63,
        len: 3,
        bits: [3212122394, 3211886692, 3211650990],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 150849184,
    },
    Case {
        shape: 9,
        rate: 48000.0,
        count: 64,
        len: 3,
        bits: [3211658355, 3211422653, 3211186950],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 211188960,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 0,
        len: 0,
        bits: [0, 0, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 4294967040,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 1,
        len: 2,
        bits: [3212836354, 3212719015, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 471148,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 31,
        len: 2,
        bits: [3212833183, 3212715332, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 15084672,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 32,
        len: 2,
        bits: [3212719015, 3212601164, 0],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 30169600,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 33,
        len: 3,
        bits: [3212601164, 3212483313, 3212365462],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 45725932,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 63,
        len: 3,
        bits: [3212479630, 3212361780, 3212243928],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 75424384,
    },
    Case {
        shape: 9,
        rate: 96000.0,
        count: 64,
        len: 3,
        bits: [3212247611, 3212129760, 3212011909],
        xyz: [1065353216, 1065353216, 1065353216],
        phase: 105594240,
    },
];
const ORIGINAL_MATH_GRID: [(u32, u32, u32); 129] = [
    (0x0, 0x3f3504f3, 0x3f3504f3),
    (0x3c000000, 0x3f3daefa, 0x3f2beb48),
    (0x3c800000, 0x3f45e403, 0x3f226799),
    (0x3cc00000, 0x3f4d9f02, 0x3f187fc1),
    (0x3d000000, 0x3f54db32, 0x3f0e39d9),
    (0x3d200000, 0x3f5b941a, 0x3f039c3a),
    (0x3d400000, 0x3f61c597, 0x3ef15aed),
    (0x3d600000, 0x3f676bd8, 0x3edae87f),
    (0x3d800000, 0x3f6c835e, 0x3ec3ef10),
    (0x3d900000, 0x3f710908, 0x3eac7cd2),
    (0x3da00000, 0x3f74fa0b, 0x3e94a032),
    (0x3db00000, 0x3f7853f8, 0x3e78cfc5),
    (0x3dc00000, 0x3f7b14be, 0x3e47c5b2),
    (0x3dd00000, 0x3f7d3aac, 0x3e164089),
    (0x3de00000, 0x3f7ec46c, 0x3dc8bd2f),
    (0x3df00000, 0x3f7fb10e, 0x3d48fafe),
    (0x3e000000, 0x3f7fffff, 0xb3bbbd2e),
    (0x3e080000, 0x3f7fb10f, 0xbd48fb2c),
    (0x3e100000, 0x3f7ec46e, 0xbdc8bd47),
    (0x3e180000, 0x3f7d3aaa, 0xbe164095),
    (0x3e200000, 0x3f7b14be, 0xbe47c5bd),
    (0x3e280000, 0x3f7853f7, 0xbe78cfd0),
    (0x3e300000, 0x3f74fa0a, 0xbe94a038),
    (0x3e380000, 0x3f710908, 0xbeac7cd0),
    (0x3e400000, 0x3f6c835e, 0xbec3ef16),
    (0x3e480000, 0x3f676bd8, 0xbedae885),
    (0x3e500000, 0x3f61c596, 0xbef15af2),
    (0x3e580000, 0x3f5b941a, 0xbf039c3c),
    (0x3e600000, 0x3f54db30, 0xbf0e39db),
    (0x3e680000, 0x3f4d9f02, 0xbf187fc3),
    (0x3e700000, 0x3f45e404, 0xbf226797),
    (0x3e780000, 0x3f3daef8, 0xbf2beb4a),
    (0x3e800000, 0x3f3504f3, 0xbf3504f5),
    (0x3e840000, 0x3f2beb48, 0xbf3daefc),
    (0x3e880000, 0x3f226799, 0xbf45e402),
    (0x3e8c0000, 0x3f187fbe, 0xbf4d9f03),
    (0x3e900000, 0x3f0e39d9, 0xbf54db33),
    (0x3e940000, 0x3f039c3d, 0xbf5b941a),
    (0x3e980000, 0x3ef15ae6, 0xbf61c598),
    (0x3e9c0000, 0x3edae87f, 0xbf676bd9),
    (0x3ea00000, 0x3ec3ef10, 0xbf6c8361),
    (0x3ea40000, 0x3eac7cd2, 0xbf710908),
    (0x3ea80000, 0x3e94a02b, 0xbf74fa0b),
    (0x3eac0000, 0x3e78cfc5, 0xbf7853f9),
    (0x3eb00000, 0x3e47c5b2, 0xbf7b14bf),
    (0x3eb40000, 0x3e16407a, 0xbf7d3aac),
    (0x3eb80000, 0x3dc8bd2f, 0xbf7ec46d),
    (0x3ebc0000, 0x3d48fafe, 0xbf7fb110),
    (0x3ec00000, 0xb3bbbd2e, 0xbf7fffff),
    (0x3ec40000, 0xbd48fb6c, 0xbf7fb10f),
    (0x3ec80000, 0xbdc8bd47, 0xbf7ec46c),
    (0x3ecc0000, 0xbe164095, 0xbf7d3aab),
    (0x3ed00000, 0xbe47c5cd, 0xbf7b14be),
    (0x3ed40000, 0xbe78cfd0, 0xbf7853f7),
    (0x3ed80000, 0xbe94a038, 0xbf74fa09),
    (0x3edc0000, 0xbeac7cd7, 0xbf710908),
    (0x3ee00000, 0xbec3ef1d, 0xbf6c835e),
    (0x3ee40000, 0xbedae885, 0xbf676bd5),
    (0x3ee80000, 0xbef15aeb, 0xbf61c598),
    (0x3eec0000, 0xbf039c3f, 0xbf5b941a),
    (0x3ef00000, 0xbf0e39db, 0xbf54db2f),
    (0x3ef40000, 0xbf187fc3, 0xbf4d9efe),
    (0x3ef80000, 0xbf22679b, 0xbf45e402),
    (0x3efc0000, 0xbf2beb4d, 0xbf3daef7),
    (0x3f000000, 0xbf3504f5, 0xbf3504ef),
    (0x3f020000, 0xbf3daefa, 0xbf2beb4a),
    (0x3f040000, 0xbf45e402, 0xbf226797),
    (0x3f060000, 0xbf4d9f03, 0xbf187fbc),
    (0x3f080000, 0xbf54db33, 0xbf0e39d4),
    (0x3f0a0000, 0xbf5b941a, 0xbf039c3c),
    (0x3f0c0000, 0xbf61c598, 0xbef15ae3),
    (0x3f0e0000, 0xbf676bd9, 0xbedae876),
    (0x3f100000, 0xbf6c835e, 0xbec3ef15),
    (0x3f120000, 0xbf710908, 0xbeac7ccf),
    (0x3f140000, 0xbf74fa0b, 0xbe94a028),
    (0x3f160000, 0xbf7853f9, 0xbe78cfaf),
    (0x3f180000, 0xbf7b14be, 0xbe47c5bb),
    (0x3f1a0000, 0xbf7d3aac, 0xbe164074),
    (0x3f1c0000, 0xbf7ec46d, 0xbdc8bd04),
    (0x3f1e0000, 0xbf7fb10f, 0xbd48fb26),
    (0x3f200000, 0xbf7fffff, 0x343bbd2e),
    (0x3f220000, 0xbf7fb10f, 0x3d48fb84),
    (0x3f240000, 0xbf7ec46d, 0x3dc8bd32),
    (0x3f260000, 0xbf7d3aac, 0x3e16408b),
    (0x3f280000, 0xbf7b14be, 0x3e47c5d3),
    (0x3f2a0000, 0xbf7853f9, 0x3e78cfc7),
    (0x3f2c0000, 0xbf74fa0b, 0x3e94a033),
    (0x3f2e0000, 0xbf710908, 0x3eac7cda),
    (0x3f300000, 0xbf6c835e, 0x3ec3ef20),
    (0x3f320000, 0xbf676bd9, 0x3edae880),
    (0x3f340000, 0xbf61c598, 0x3ef15aee),
    (0x3f360000, 0xbf5b941a, 0x3f039c41),
    (0x3f380000, 0xbf54db33, 0x3f0e39d9),
    (0x3f3a0000, 0xbf4d9f03, 0x3f187fc1),
    (0x3f3c0000, 0xbf45e402, 0x3f22679c),
    (0x3f3e0000, 0xbf3daef7, 0x3f2beb4e),
    (0x3f400000, 0xbf3504f5, 0x3f3504f3),
    (0x3f420000, 0xbf2beb4a, 0x3f3daefb),
    (0x3f440000, 0xbf226797, 0x3f45e406),
    (0x3f460000, 0xbf187fc3, 0x3f4d9f02),
    (0x3f480000, 0xbf0e39db, 0x3f54db32),
    (0x3f4a0000, 0xbf039c3c, 0x3f5b941c),
    (0x3f4c0000, 0xbef15ae3, 0x3f61c59a),
    (0x3f4e0000, 0xbedae884, 0x3f676bd8),
    (0x3f500000, 0xbec3ef15, 0x3f6c835f),
    (0x3f520000, 0xbeac7ccf, 0x3f71090a),
    (0x3f540000, 0xbe94a037, 0x3f74fa0a),
    (0x3f560000, 0xbe78cfcf, 0x3f7853f8),
    (0x3f580000, 0xbe47c5bb, 0x3f7b14bf),
    (0x3f5a0000, 0xbe164093, 0x3f7d3aaa),
    (0x3f5c0000, 0xbdc8bd44, 0x3f7ec46e),
    (0x3f5e0000, 0xbd48fb26, 0x3f7fb10e),
    (0x3f600000, 0x343bbd2e, 0x3f7fffff),
    (0x3f620000, 0x3d48fb04, 0x3f7fb10e),
    (0x3f640000, 0x3dc8bd32, 0x3f7ec46c),
    (0x3f660000, 0x3e16408b, 0x3f7d3aac),
    (0x3f680000, 0x3e47c5b4, 0x3f7b14be),
    (0x3f6a0000, 0x3e78cfc7, 0x3f7853f8),
    (0x3f6c0000, 0x3e94a033, 0x3f74fa0a),
    (0x3f6e0000, 0x3eac7cda, 0x3f710906),
    (0x3f700000, 0x3ec3ef11, 0x3f6c8362),
    (0x3f720000, 0x3edae880, 0x3f676bda),
    (0x3f740000, 0x3ef15aee, 0x3f61c599),
    (0x3f760000, 0x3f039c3a, 0x3f5b941a),
    (0x3f780000, 0x3f0e39d9, 0x3f54db30),
    (0x3f7a0000, 0x3f187fc1, 0x3f4d9f00),
    (0x3f7c0000, 0x3f22679c, 0x3f45e400),
    (0x3f7e0000, 0x3f2beb48, 0x3f3daefe),
    (0x3f800000, 0x3f3504f3, 0x3f3504f7),
];

#[test]
fn original_actual_partitions_and_committed_state_are_bit_exact() {
    let values = ramp();
    let table = Table::checked(&values).unwrap();
    for group in NATIVE.chunks_exact(7) {
        let shape = group[0].shape;
        let mut state = State::explicit([1.0; 3], 0xffff_ff00).unwrap();
        let frequency = if shape == 7 { 3.692_490_8 } else { 10.536_713 };
        let parameters = Parameters::checked(group[0].rate, frequency).unwrap();
        for case in group {
            let waveform = if shape == 7 {
                Waveform::Lorenz
            } else {
                Waveform::User(table)
            };
            let points = state.generate(waveform, parameters, case.count).unwrap();
            assert_eq!(points.as_slice().len(), case.len);
            assert_eq!(
                &bits(points)[..case.len],
                &case.bits[..case.len],
                "shape {shape} rate {} frames {}",
                case.rate,
                case.count
            );
            assert_eq!(state.coordinates().map(f32::to_bits), case.xyz);
            assert_eq!(state.phase(), case.phase);
        }
    }
}

#[test]
fn original_finite_math_both_variants_match_129_phase_vectors() {
    for &(phase, sine, cosine) in &ORIGINAL_MATH_GRID {
        let angle =
            f32::from_bits(phase) * f32::from_bits(0x40c9_0fdb) + f32::from_bits(0x3f49_0fdb);
        assert_eq!(finite_sine(angle).to_bits(), sine);
        assert_eq!(
            finite_sine(angle.abs() + f32::from_bits(0x3fc9_0fdb)).to_bits(),
            cosine
        );
    }
}

#[test]
fn original_reseed_and_generator_vectors_preserve_endpoint_precision() {
    let mut state = State::explicit([0.0; 3], 123).unwrap();
    state.reseed(0.0).unwrap();
    assert_eq!(
        state.coordinates().map(f32::to_bits),
        [0x3f7f_ffff, 0x3f7f_ffff, 0x3f80_0000]
    );
    let points = state
        .generate(
            Waveform::Lorenz,
            Parameters::checked(48_000.0, 3.692_490_8).unwrap(),
            32,
        )
        .unwrap();
    assert_eq!(&bits(points)[..2], &[0x3d51_2ec6; 2]);
    assert_eq!(
        state.coordinates().map(f32::to_bits),
        [0x3f7f_ffff, 0x3f88_3141, 0x3f7e_f33b]
    );
    state.reseed(0.25).unwrap();
    assert_eq!(
        state.coordinates().map(f32::to_bits),
        [0xbf80_0001, 0x3f7f_ffff, 0x3f80_0000]
    );
    let values = ramp();
    let points = state
        .generate(
            Waveform::User(Table::checked(&values).unwrap()),
            Parameters::checked(48_000.0, 10.536_713).unwrap(),
            1,
        )
        .unwrap();
    assert_eq!(&bits(points)[..2], &[0xbf00_0000, 0xbef8_ce94]);
    assert_eq!(state.phase(), 0x400e_62d9);
    state.reseed(1.0).unwrap();
    assert_eq!(state.phase(), 0);
    assert_eq!(state.coordinates()[0].to_bits(), 0x3f80_0003);
}

#[test]
fn original_depth_and_unipolar_vectors_are_source_order_exact() {
    let parameters = Parameters::checked(48_000.0, 3.692_490_8).unwrap();
    let mut state = State::explicit([1.0; 3], 0xffff_ff00).unwrap();
    let mut points = state.generate(Waveform::Lorenz, parameters, 64).unwrap();
    points.apply_amplitude(0.125, false).unwrap();
    assert_eq!(bits(points), [0x3d86_8976, 0x3d86_8c19, 0x3d86_8c19]);
    let values = ramp();
    let mut state = State::explicit([1.0; 3], 0xffff_ff00).unwrap();
    let mut points = state
        .generate(
            Waveform::User(Table::checked(&values).unwrap()),
            Parameters::checked(48_000.0, 10.536_713).unwrap(),
            64,
        )
        .unwrap();
    points.apply_amplitude(0.125, false).unwrap();
    assert_eq!(bits(points), [0x35ff_0000, 0x3a66_2d00, 0x3ae6_2d40]);
    let mut bipolar = Points {
        values: [-0.5, 0.25, 1.0],
        len: 3,
    };
    bipolar.apply_amplitude(0.125, true).unwrap();
    assert_eq!(
        bits(bipolar),
        [-0.0625_f32, 0.03125, 0.125].map(f32::to_bits)
    );
}

#[test]
fn explicit_nonwrapping_guard_is_preserved_at_the_seam() {
    let mut values = [0.0; 257];
    values[256] = 0.75;
    let table = Table::checked(&values).unwrap();
    let mut state = State::explicit([1.0; 3], 0xff80_0000).unwrap();
    let points = state
        .generate(
            Waveform::User(table),
            Parameters::checked(48_000.0, 0.0).unwrap(),
            1,
        )
        .unwrap();
    assert_eq!(&bits(points)[..2], &[0.375_f32.to_bits(); 2]);
    assert_eq!(values[256], 0.75);
    assert_eq!(state.phase(), 0xff80_0000);
}

#[test]
fn invalid_inputs_and_failed_generation_leave_state_or_points_intact() {
    for rate in [0.0, -1.0, f32::NAN, f32::INFINITY, f32::MIN_POSITIVE] {
        assert_eq!(
            Parameters::checked(rate, 1.0).unwrap_err(),
            DomainError::SampleRate
        );
    }
    // A rate outside the observed three-rate grid is not a public admission gate.
    Parameters::checked(88_200.0, 1.0).unwrap();
    for frequency in [-1.0, 20.01, f32::NAN, f32::INFINITY] {
        assert_eq!(
            Parameters::checked(48_000.0, frequency).unwrap_err(),
            DomainError::Frequency
        );
    }
    assert_eq!(
        State::explicit([f32::NAN, 1.0, 1.0], 0).unwrap_err(),
        DomainError::Coordinates
    );
    let mut values = [0.0; 257];
    values[256] = f32::INFINITY;
    assert_eq!(
        Table::checked(&values).unwrap_err(),
        DomainError::TableNonFinite
    );
    let parameters = Parameters::checked(48_000.0, 20.0).unwrap();
    let mut state = State::explicit([f32::MAX, -f32::MAX, 1.0], 19).unwrap();
    let before = state;
    assert_eq!(
        state
            .generate(Waveform::Lorenz, parameters, 32)
            .unwrap_err(),
        DomainError::NonFiniteResult
    );
    assert_eq!(state, before);
    assert_eq!(
        state
            .generate(Waveform::Lorenz, parameters, 65)
            .unwrap_err(),
        DomainError::FrameCount
    );
    assert_eq!(state, before);
    for phase in [-0.01, 1.01, f32::NAN, f32::INFINITY] {
        assert_eq!(state.reseed(phase), Err(DomainError::Phase));
        assert_eq!(state, before);
    }
    let mut points = Points {
        values: [0.25, -0.5, 0.0],
        len: 2,
    };
    let before = points;
    for depth in [0.0, -1.0, 1.01, f32::NAN, f32::INFINITY] {
        assert_eq!(
            points.apply_amplitude(depth, false),
            Err(DomainError::Amplitude)
        );
        assert_eq!(points, before);
    }
}

#[test]
fn stack_state_is_copy_without_drop_and_keeps_the_table_immutable() {
    assert!(!std::mem::needs_drop::<State>());
    assert!(!std::mem::needs_drop::<Points>());
    assert!(!std::mem::needs_drop::<Waveform<'_>>());
    let values = ramp();
    let before = values.map(f32::to_bits);
    let table = Table::checked(&values).unwrap();
    let parameters = Parameters::checked(48_000.0, 10.536_713).unwrap();
    {
        let mut state = State::explicit([1.0; 3], 0xffff_ff00).unwrap();
        let unchanged = state;
        assert!(
            state
                .generate(Waveform::User(table), parameters, 0)
                .unwrap()
                .as_slice()
                .is_empty()
        );
        assert_eq!(state, unchanged);
        for frames in [1, 31, 32, 33, 63, 64] {
            let mut points = state
                .generate(Waveform::User(table), parameters, frames)
                .unwrap();
            points.apply_amplitude(0.125, true).unwrap();
            let mut points = state
                .generate(Waveform::Lorenz, parameters, frames)
                .unwrap();
            points.apply_amplitude(0.125, false).unwrap();
        }
        state.reseed(1.0).unwrap();
    }
    assert_eq!(values.map(f32::to_bits), before);
}
