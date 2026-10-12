//! Literal native prepared ready-source captures; no production reference kernel.
//! Artifact SHA256 0fe6356e0879d058b6e5b73507c54c5e345cea451b35287c974e438291d4dae8.
//! Native producer 14087ab10, physical loader 140828500, partner 140889570.
//! This fixture tests the producer law and declared engine request policy. It
//! does not assert vendor plugin host framing or asynchronous NI disk parity.
use super::*;

struct Capture {
    name: &'static str, constant: bool, position: f64, step: f64,
    frames: usize, start: i32, length: i32,
    width: i32, count: i32, mode: i32, ordered: bool, release: Option<usize>,
    chunks: &'static [usize], ends: &'static [f64], pcm: &'static [u32],
}
const CAPTURES: &[Capture] = &[
    Capture { frames: 4096, start: 256, length: 768, name: "begin-short-constant", constant: true, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[1], ends: &[895.75], pcm: &[0x3f801000] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-short-periodic", constant: false, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[1], ends: &[895.75], pcm: &[0xbd4a1a80] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-cross-constant", constant: true, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[2], ends: &[896.0], pcm: &[0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-cross-periodic", constant: false, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[2], ends: &[896.0], pcm: &[0xbd4a2d00,0xbcd22ec0] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-fragmented-constant", constant: true, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[1, 1], ends: &[895.75, 896.0], pcm: &[0x3f801000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-fragmented-periodic", constant: false, position: 895.5, step: 0.25, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[1, 1], ends: &[895.75, 896.0], pcm: &[0xbd4a1a80,0xbcd22ec0] },
    Capture { frames: 4096, start: 256, length: 768, name: "interior-constant", constant: true, position: 897.25, step: 0.75, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[921.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "interior-periodic", constant: false, position: 897.25, step: 0.75, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[921.25], pcm: &[0x3d06f860,0xbdd32800,0xbd8003d0,0xba4a4000,0x3d800e00,0x3dd90800,0xbd06e400,0xbd5df800,0x3cc37260,0x3d8b4000,0xbd6728f0,0xbda86fc0,0xbcc63fe0,0x3cf5e000,0x3dc6a200,0x3c34a000,0xbd997a00,0xbc028000,0x3d6aad00,0xbc188000,0xbdb28d80,0xbd3c3000,0x3bc73800,0x3d9c2400,0x3d613400,0xbdabe000,0xbd25cc00,0x3d112c80,0x3cf04ca0,0xbd9d4c00,0xbd876cd8,0xbc954700] },
    Capture { frames: 4096, start: 256, length: 768, name: "wrap-constant", constant: true, position: 1022.25, step: 0.75, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[278.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "wrap-periodic", constant: false, position: 1022.25, step: 0.75, width: 128, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[278.25], pcm: &[0xbda9e958,0xbc85a000,0x3d4f7360,0xbd100000,0xbdf79c00,0xbd5c0000,0xb9800000,0x3d8d2000,0x3d441000,0xbdba0000,0xbd412800,0x3c840000,0x3da29400,0x3dfc0000,0xbc802000,0xbd164000,0x3d2d2800,0x3db00000,0xbd581000,0xbd972000,0xbb980000,0x3d480000,0x3ded9c00,0x3cf80000,0xbd633800,0x3c400000,0x3da19c00,0xbbe00000,0xbdbd9c00,0xbcd00000,0x3ce60000,0x3dc72000] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-width127-constant", constant: true, position: 896.5, step: 0.25, width: 127, count: 0, mode: 1, ordered: false, release: None, chunks: &[2, 32], ends: &[897.0, 905.0], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800002,0x3f800004,0x3f800006,0x3f800008,0x3f800009,0x3f800009,0x3f800008,0x3f800008,0x3f800009,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800009,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800008,0x3f800009,0x3f800008,0x3f800009,0x3f800008,0x3f800008] },
    Capture { frames: 4096, start: 256, length: 768, name: "begin-width127-periodic", constant: false, position: 896.5, step: 0.25, width: 127, count: 0, mode: 1, ordered: false, release: None, chunks: &[2, 32], ends: &[897.0, 905.0], pcm: &[0x3d59d1a7,0x3d8d6bb9,0x3d960000,0x3d060ab6,0xbc7fe23b,0xbd832314,0xbdd68d36,0xbdcf09ab,0xbdafbf0e,0xbd8084d3,0xbd1264ce,0xbc944568,0xb9bfbc28,0x3c8ec907,0x3d1068e0,0x3d80a228,0x3db107de,0x3dd15d94,0x3dd99b47,0x3d857cf6,0x3c853a88,0xbd05bf63,0xbd96fe04,0xbd8ec050,0xbd5cd535,0xbcf8138e,0xbb32e588,0x3cc64b10,0x3d426c6d,0x3d81ccad,0x3d8bcfab,0x3d011794,0xbc5234b1,0xbd6c37f7] },
    Capture { frames: 4096, start: 256, length: 768, name: "final-constant", constant: true, position: 1022.25, step: 0.75, width: 128, count: 1, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[1046.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "final-periodic", constant: false, position: 1022.25, step: 0.75, width: 128, count: 1, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[1046.25], pcm: &[0xbda9e958,0xbc85a000,0x3d4f7360,0x3b000000,0xbdab9c00,0xbc880000,0x3d4f3800,0xbd100000,0xbdf79c00,0xbd5c0000,0xb9800000,0x3d8d2000,0x3d441000,0xbdba0000,0xbd412800,0x3c840000,0x3da29400,0x3dfc0000,0xbc802000,0xbd164000,0x3d2d2800,0x3db00000,0xbd581000,0xbd972000,0xbb980000,0x3d480000,0x3ded9c00,0x3cf80000,0xbd633800,0x3c400000,0x3da19c00,0xbbe00000] },
    Capture { frames: 4096, start: 256, length: 768, name: "negative-constant", constant: true, position: 1022.25, step: 0.75, width: 128, count: -1, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[278.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "negative-periodic", constant: false, position: 1022.25, step: 0.75, width: 128, count: -1, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[278.25], pcm: &[0xbda9e958,0xbc85a000,0x3d4f7360,0xbd100000,0xbdf79c00,0xbd5c0000,0xb9800000,0x3d8d2000,0x3d441000,0xbdba0000,0xbd412800,0x3c840000,0x3da29400,0x3dfc0000,0xbc802000,0xbd164000,0x3d2d2800,0x3db00000,0xbd581000,0xbd972000,0xbb980000,0x3d480000,0x3ded9c00,0x3cf80000,0xbd633800,0x3c400000,0x3da19c00,0xbbe00000,0xbdbd9c00,0xbcd00000,0x3ce60000,0x3dc72000] },
    Capture { frames: 4096, start: 256, length: 768, name: "ordered-constant", constant: true, position: 1022.25, step: 16.25, width: 128, count: 1, mode: 1, ordered: true, release: None, chunks: &[32], ends: &[1542.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "ordered-periodic", constant: false, position: 1022.25, step: 16.25, width: 128, count: 1, mode: 1, ordered: true, release: None, chunks: &[32], ends: &[1542.25], pcm: &[0xbda9e958,0xbb800000,0x3cce0000,0xbd920000,0x3cc82000,0xbc580000,0xbd501000,0x3d380000,0xbd859c00,0xbcb80000,0x3bd80000,0xbdb80000,0x3d989400,0xbd424000,0xbd8e0800,0x3cd80000,0xbdab9c00,0x3dcd2000,0xbcb25000,0xbdde0000,0x3d652800,0xbd872000,0x3df19c00,0x3c000000,0xbdd19c00,0x3da72000,0xbd252800,0x3dfe0000,0x3d192800,0xbdad2000,0x3dcb9c00,0xbc300000] },
    Capture { frames: 4096, start: 256, length: 768, name: "until-release-constant", constant: true, position: 897.25, step: 0.75, width: 128, count: 0, mode: 2, ordered: false, release: Some(1), chunks: &[32, 256], ends: &[921.25, 1113.25], pcm: &[0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 4096, start: 256, length: 768, name: "until-release-periodic", constant: false, position: 897.25, step: 0.75, width: 128, count: 0, mode: 2, ordered: false, release: Some(1), chunks: &[32, 256], ends: &[921.25, 1113.25], pcm: &[0x3d06f860,0xbdd32800,0xbd8003d0,0xba4a4000,0x3d800e00,0x3dd90800,0xbd06e400,0xbd5df800,0x3cc37260,0x3d8b4000,0xbd6728f0,0xbda86fc0,0xbcc63fe0,0x3cf5e000,0x3dc6a200,0x3c34a000,0xbd997a00,0xbc028000,0x3d6aad00,0xbc188000,0xbdb28d80,0xbd3c3000,0x3bc73800,0x3d9c2400,0x3d613400,0xbdabe000,0xbd25cc00,0x3d112c80,0x3cf04ca0,0xbd9d4c00,0xbd876cd8,0xbc954700,0x3d432c00,0x3dba9000,0xbd43d400,0xbd8d7400,0x3bf19500,0x3d599000,0xbd19bf20,0xbd977880,0xbd294920,0x3c780000,0x3da82a00,0xbb7c8000,0xbdb7f200,0xbcbb2000,0x3d2483d0,0xba863000,0xbd8bd898,0xbd792000,0xbc2a7e40,0x3d7b5800,0x3d244400,0xbdca5800,0xbd62bc00,0x3c936400,0x3cd2a880,0xbd4ee000,0xbd8ed5e0,0xbd121e00,0x3cee5a00,0x3d9c1800,0x3c6ddc00,0xbcfe0000,0xbca52200,0x3d1ca000,0x3dd77a00,0x3c9db000,0xbd88a200,0x39880000,0x3d7b3bf0,0x3d2bbf80,0xbc84f620,0xbd1a8000,0x3c396b40,0x3dacfc00,0x3d817200,0xbd9b0800,0xbd041c00,0x3d1f4900,0x3d6f7ba0,0x3c014000,0xbd178460,0xbc721c00,0x3d64dc00,0x3dcb6800,0xbd222400,0xbd793800,0x3c4e99c0,0x3d7b4000,0x3d08e0d0,0xbcba2300,0xbd18ba30,0x3cbf6000,0x3db90200,0x3b8f4000,0xbda71a00,0xbc6f8000,0x3d3512c0,0x3d4dae00,0x3b2eec00,0xbd577000,0xbba95e00,0x3d8e8400,0x3d45f400,0xbdb98000,0xbd410c00,0x3caf9d00,0x3d60a990,0x3d0c0800,0xbd265670,0xbd040180,0x3d27ec00,0x3dacf000,0xbd5f1400,0xbd9b1400,0xbb7e0200,0x3d3e5000,0x3d564aa0,0xbc6c8c00,0xbd5ee360,0x3c0b0000,0x3d9a8a00,0xbc2c2000,0xbdc59200,0xbcf1a000,0x3cddd320,0x3d6f9c80,0x3cb0b120,0xbd8a3000,0xbcb164a0,0x3d601800,0x3d090400,0xbdd7f800,0xbd7dfc00,0x3b82a000,0x3d51d780,0x3d77c000,0xbd352880,0xbd4b7c00,0x3cd5f800,0x3d8e7800,0xbd8e0200,0xbdb98c00,0xbca6cd60,0x3d016000,0x3d91da38,0xbbc9a400,0xbd928648,0xbbd18000,0x3d782400,0xbccff000,0xbde40a00,0xbd35c000,0x3c230180,0x3d88c580,0x3d25c260,0xbda8a800,0xbd1c38e0,0x3d232800,0x3c982800,0xbdf67000,0xbd9d7600,0xbc5c9a00,0x3d430570,0x3db1bc00,0xbd43fa90,0xbd897b40,0x3c380080,0x3dbc0000,0xbd401000,0xbd8b2000,0x3c34a000,0x3d600000,0xbdac0800,0xbdd72000,0xbd130000,0x3c900000,0x3dad9c00,0xba800000,0xbdb19c00,0xbca00000,0x3d0b0000,0x3dd32000,0x3da80800,0xbd680000,0xbc54a000,0x3d872000,0x3d381000,0xbdc00000,0xbd4d2800,0x3c580000,0x3d9c9400,0x3df60000,0xbc982000,0xbd224000,0x3d212800,0x3daa0000,0xbd641000,0xbd9d2000,0xbbf80000,0x3d3c0000,0x3de79c00,0x3ce00000,0xbd6f3800,0x3c100000,0x3d9b9c00,0xbc200000,0xbdc39c00,0xbce80000,0x3cce0000,0x3dc12000,0x3d960800,0xbd860000,0xbcb25000,0x3d6a4000,0x3d141000,0xbdd20000,0xbd712800,0x3b900000,0x3d8a9400,0x3de40000,0xbce02000,0xbd464000,0x3cfa5000,0x3d980000,0xbd840800,0xbdaf2000,0xbc860000,0x3d180000,0x3dd59c00,0x3c980000,0xbd899c00,0x0,0x3d899c00,0xbc980000,0xbdd59c00,0xbd180000,0x3c860000,0x3daf2000,0x3d840800,0xbd980000,0xbcfa5000,0x3d464000,0x3ce02000,0xbde40000,0xbd8a9400,0xbb900000,0x3d712800,0x3dd20000,0xbd141000,0xbd6a4000,0x3cb25000,0x3d860000,0xbd960800,0xbdc12000,0xbcce0000,0x3ce80000,0x3dc39c00,0x3c200000,0xbd9b9c00,0xbc100000,0x3d6f3800,0xbce00000,0xbde79c00,0xbd3c0000,0x3bf80000,0x3d9d2000,0x3d641000,0xbdaa0000,0xbd212800,0x3d224000,0x3c982000,0xbdf60000,0xbd9c9400,0xbc580000,0x3d4d2800,0x3dc00000,0xbd381000,0xbd872000,0x3c54a000,0x3d680000,0xbda80800,0xbdd32000,0xbd0b0000,0x3ca00000,0x3db19c00,0x3a800000] },
    Capture { frames: 262144, start: 130484, length: 120001, name: "vista-width1000-quarter-begin-constant", constant: true, position: 249484.5, step: 0.25, width: 1000, count: 0, mode: 1, ordered: false, release: None, chunks: &[1, 2, 32, 128], ends: &[249484.75, 249485.25, 249493.25, 249525.25], pcm: &[0x3f8001f1,0x3f7fffb1,0x3f7fff93,0x3f7fff82,0x3f7fff78,0x3f7fff70,0x3f7fff6b,0x3f7fff69,0x3f7fff68,0x3f7fff6a,0x3f7fff6b,0x3f7fff6e,0x3f7fff6b,0x3f7fff6b,0x3f7fff6c,0x3f7fff6a,0x3f7fff6a,0x3f7fff6b,0x3f7fff6c,0x3f7fff6c,0x3f7fff6d,0x3f7fff6b,0x3f7fff6c,0x3f7fff6c,0x3f7fff6d,0x3f7fff6b,0x3f7fff6c,0x3f7fff6b,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6c,0x3f7fff6d,0x3f7fff6e,0x3f7fff6d,0x3f7fff6c,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6c,0x3f7fff6c,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6e,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6d,0x3f7fff6e,0x3f7fff6f,0x3f7fff6f,0x3f7fff6e,0x3f7fff6e,0x3f7fff6c,0x3f7fff6e,0x3f7fff6e,0x3f7fff6e,0x3f7fff6e,0x3f7fff6e,0x3f7fff6e,0x3f7fff6e,0x3f7fff6f,0x3f7fff6e,0x3f7fff6d,0x3f7fff6f,0x3f7fff6f,0x3f7fff6e,0x3f7fff6f,0x3f7fff6f,0x3f7fff6f,0x3f7fff6e,0x3f7fff70,0x3f7fff6f,0x3f7fff6f,0x3f7fff71,0x3f7fff70,0x3f7fff6f,0x3f7fff72,0x3f7fff70,0x3f7fff70,0x3f7fff6f,0x3f7fff6f,0x3f7fff70,0x3f7fff70,0x3f7fff70,0x3f7fff6f,0x3f7fff70,0x3f7fff71,0x3f7fff70,0x3f7fff6f,0x3f7fff6e,0x3f7fff70,0x3f7fff70,0x3f7fff6f,0x3f7fff71,0x3f7fff70,0x3f7fff70,0x3f7fff72,0x3f7fff71,0x3f7fff70,0x3f7fff70,0x3f7fff72,0x3f7fff71,0x3f7fff70,0x3f7fff71,0x3f7fff6f,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff6f,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff71,0x3f7fff72,0x3f7fff71,0x3f7fff70,0x3f7fff72,0x3f7fff72,0x3f7fff71,0x3f7fff70,0x3f7fff72,0x3f7fff72,0x3f7fff73,0x3f7fff73,0x3f7fff72,0x3f7fff71,0x3f7fff72,0x3f7fff73,0x3f7fff72,0x3f7fff72,0x3f7fff72,0x3f7fff73,0x3f7fff72,0x3f7fff74,0x3f7fff72,0x3f7fff72,0x3f7fff73,0x3f7fff72,0x3f7fff71,0x3f7fff72,0x3f7fff73,0x3f7fff74,0x3f7fff73,0x3f7fff73,0x3f7fff73,0x3f7fff72,0x3f7fff74,0x3f7fff73,0x3f7fff73,0x3f7fff72,0x3f7fff74,0x3f7fff73,0x3f7fff73,0x3f7fff74,0x3f7fff74,0x3f7fff75,0x3f7fff74] },
    Capture { frames: 262144, start: 130484, length: 120001, name: "vista-width1000-quarter-begin-periodic", constant: false, position: 249484.5, step: 0.25, width: 1000, count: 0, mode: 1, ordered: false, release: None, chunks: &[1, 2, 32, 128], ends: &[249484.75, 249485.25, 249493.25, 249525.25], pcm: &[0xbda11e7f,0xbd612972,0xbcdfffa1,0xbc1b9667,0x3c08b705,0x3cd66329,0x3d341d37,0x3d925e51,0x3dc2a1f0,0x3de2ddcf,0x3deb1645,0x3d975168,0x3cce4470,0xbcc09bdf,0xbd83d331,0xbd76ba3f,0xbd35b6dd,0xbca947f6,0x3be3b533,0x3d0da5d3,0x3d6ec505,0x3d97e227,0x3da049d8,0x3d18ab7d,0xbc3d728b,0xbd7764c2,0xbdcfa67b,0xbdc73eca,0xbda6bf26,0xbd6c5f17,0xbcf65fd7,0xbc4730a8,0x3bbb94bf,0x3cc0ceb4,0x3d28ed1e,0x3d8bca68,0x3dbb03c7,0x3dda522e,0x3de1e523,0x3d8ec5c6,0x3cafcbcc,0xbcdaeb86,0xbd896b3f,0xbd80c2ce,0xbd4073d6,0xbcbf1840,0x3b8a3466,0x3d0275b8,0x3d6394ed,0x3d924a1a,0x3d9ab1cc,0x3d0d7b63,0xbc6a32f0,0xbd814a6e,0xbdd53e89,0xbdccd6d7,0xbdac5733,0xbd778f32,0xbd066006,0xbc72d13c,0x3b4b5a0f,0x3cab37fe,0x3d1dbd05,0x3d853681,0x3db3659f,0x3dd1c690,0x3dd8b402,0x3d863a27,0x3c915325,0xbcf53b2d,0xbd8f034d,0xbd86287e,0xbd4b30cf,0xbcd4e88c,0x3ac2ce5a,0x3cee8b40,0x3d5864d6,0x3d8cb20d,0x3d9519c0,0x3d024b4b,0xbc8b79af,0xbd86e27d,0xbddad697,0xbdd26ee6,0xbdb1ef41,0xbd815fa7,0xbd119021,0xbc8f38e7,0x39fc5488,0x3c95a149,0x3d128cec,0x3d7d4531,0x3dabc777,0x3dc93aef,0x3dcf82df,0x3d7b5d0c,0x3c65b504,0xbd07c56b,0xbd949b5b,0xbd8b8e2b,0xbd55edc6,0xbceab8d6,0xbaa334e6,0x3cd82b0d,0x3d4d34be,0x3d871a03,0x3d8f81b4,0x3cee3664,0xbca1d9e2,0xbd8c7a8b,0xbde06ea5,0xbdd806f5,0xbdb78750,0xbd86f7b4,0xbd1cc03b,0xbca5092f,0xbb0c44dc,0x3c800a93,0x3d075cd3,0x3d701d60,0x3da4294f,0x3dc0af4f,0x3dc651bc,0x3d6a45c9,0x3c28c3b6,0xbd14ed3c,0xbd9a3369,0xbd90f3da,0xbd60aabf,0xbd00448f,0xbb824e0b,0x3cc1cad8,0x3d4204a3,0x3d8181f6,0x3d89e9a7,0x3cd7d630,0xbcb83a19,0xbd921298,0xbde606b4,0xbddd9f04,0xbdbd1f5e,0xbd8c8fc2,0xbd27f057,0xbcbad97a,0xbb9c0a28,0x3c54e7b9,0x3cf85974,0x3d62f58e,0x3d9c8b25,0x3db823af,0x3dbd209a,0x3d592e88,0x3bd7a4c6,0xbd22150f,0xbd9fcb77,0xbd965988,0xbd6b67b9,0xbd0b2cb4,0xbbdbcede,0x3cab6aa6,0x3d36d48b,0x3d77d3d3,0x3d84519b] },
    Capture { frames: 262144, start: 130484, length: 120001, name: "vista-width1000-quarter-wrap-constant", constant: true, position: 250483.75, step: 0.75, width: 1000, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[130506.75], pcm: &[0x3f7fffa1,0x3f7fff9a,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000,0x3f800000] },
    Capture { frames: 262144, start: 130484, length: 120001, name: "vista-width1000-quarter-wrap-periodic", constant: false, position: 250483.75, step: 0.75, width: 1000, count: 0, mode: 1, ordered: false, release: None, chunks: &[32], ends: &[130506.75], pcm: &[0xbc906a05,0xbd1e4cc9,0x3d252800,0x3dac0000,0xbd601000,0xbd9b2000,0xbbd80000,0x3d400000,0x3de99c00,0x3ce80000,0xbd6b3800,0x3c200000,0x3d9d9c00,0xbc100000,0xbdc19c00,0xbce00000,0x3cd60000,0x3dc32000,0x3d980800,0xbd840000,0xbcaa5000,0x3d6e4000,0x3d181000,0xbdd00000,0xbd6d2800,0x3bb00000,0x3d8c9400,0x3de60000,0xbcd82000,0xbd424000,0x3d012800,0x3d9a0000] },
 ];

#[test]
fn native_normal_positive_crossfade_matches_whole_native_pcm_and_pending_phases_without_heap() {
    use crate::{audio::Sample, engine::{Engine, Ahdsr},
        import::{Group, Instrument, NativeLoop, SamplingProfile, Zone}};
    let dir = tempfile::tempdir().unwrap();
    for (case_index, case) in CAPTURES.iter().enumerate() {
        let path = dir.path().join(format!("crossfade-{case_index}.wav"));
        let frames: Vec<_> = (0..case.frames).map(|i| [if case.constant { 1. }
            else { (((i * 73) % 257) as f32 - 128.) / 1024. }; 2]).collect();
        let rate = (case.step * 48000.) as u32;
        let mut wav = hound::WavWriter::create(&path, hound::WavSpec {
            channels: 2, sample_rate: rate, bits_per_sample: 32,
            sample_format: hound::SampleFormat::Float }).unwrap();
        for frame in &frames { for value in frame { wav.write_sample(*value).unwrap(); } }
        wav.finalize().unwrap();
        for source_mode in [0, 3] {
            let group = Group { source_mode: Some(source_mode), interp_quality: 0, ..Default::default() };
            let mut rows = vec![NativeLoop { slot: 0, mode: case.mode,
                start: case.start, length: case.length, count: case.count, alternating: false,
                tuning: 1., crossfade: case.width }];
            if case.ordered { rows.push(NativeLoop { slot: 1, mode: 1,
                start: 1280, length: 256, count: 2, alternating: false,
                tuning: 1., crossfade: 0 }); }
            let zone = Zone { sample: path.clone(), native_loops: rows, ..Default::default() };
            for streamed in [false, true] {
                let bank = if streamed {
                    Bank::load_bare(&Instrument { groups: vec![group.clone()], zones: vec![zone.clone()],
                        sampling_profile: SamplingProfile::Kontakt, ..Default::default() }).unwrap()
                } else {
                    Bank::from_samples_with_profile(vec![group.clone()], vec![zone.clone()],
                        vec![(path.clone(), Sample { rate, frames: frames.clone() })],
                        SamplingProfile::Kontakt).unwrap()
                }.with_prepared_settings(|settings| settings[0].envelope = Some(Ahdsr::UNITY)).unwrap();
                let mut engine = Engine::default();
                engine.reset(48000.); engine.attack = 0.; engine.blocking_streams = true;
                engine.set_bank(Some(Box::new(bank)));
                engine.note_on(0, 60, 127);
                assert_eq!(engine.player.voices.len(), 1);
                engine.player.voices[0].counted.as_mut().unwrap().position = case.position;
                engine.player.voices[0].pos = case.position;
                // Several local subdivisions share one native producer horizon.
                // For begin-cross n2 this must differ from begin-fragmented n1+n1.
                for fragment in [1, 7, 128] {
                    let voice = &mut engine.player.voices[0];
                    voice.counted = Some(super::super::counted_loop::Cursor::new(
                        voice.counted.unwrap().rows, &engine.bank.as_ref().unwrap().counted_loops[voice.counted.unwrap().rows as usize], 0., false));
                    voice.counted.as_mut().unwrap().position = case.position;
                    voice.pos = case.position;
                    let mut scratch = [[0.; 2]; WINDOW];
                    let positions = [0; MAX_BLOCK];
                    let mut at = 0;
                    assert_eq!(crate::plugin::tests::allocations(|| {
                        for (chunk_index, &count) in case.chunks.iter().enumerate() {
                            let bank = engine.bank.as_ref().unwrap();
                            if case.release == Some(chunk_index) {
                                let cursor = engine.player.voices[0].counted.as_mut().unwrap();
                                cursor.release(&bank.counted_loops[cursor.rows as usize]);
                            }
                            let end = at + count;
                            while at < end {
                                let n = fragment.min(MAX_BLOCK).min(end - at);
                                let mut cx = Context::new(bank, &engine.state,
                                    &engine.player.midi, &engine.player.host_notes, &engine.player.per_note,
                                    engine.player.instrument_controls(&engine.state), false,
                                    48000., 120., &engine.player.stream_error, at as u64, true, 0.);
                                cx.source_window = Some(((end - count) as u64, end as u64));
                                let voice = &mut engine.player.voices[0];
                                assert!(voice.plan(&cx, n, None, None).is_ok(), "native source must be admitted");
                                let mut underrun = false;
                                let (pcm, _) = voice.window(&cx, &mut scratch, &positions, &mut underrun);
                                assert!(!underrun, "{} mode{source_mode} streamed{streamed}", case.name);
                                for (i, frame) in pcm[1..n + 1].iter().enumerate() {
                                    let expected = f32::from_bits(case.pcm[at + i]);
                                    for value in frame { assert_eq!(value.to_bits(), expected.to_bits(),
                                        "{} mode{source_mode} streamed{streamed} fragment{fragment} frame{}", case.name, at + i); }
                                }
                                voice.advance(&cx);
                                at += n;
                            }
                            assert_eq!(engine.player.voices[0].counted.unwrap().position,
                                case.ends[chunk_index], "{} pending phase", case.name);
                        }
                    }), 0, "{} callback allocation", case.name);
                }
            }
        }
    }
}

#[test]
fn curved_future_overflow_preserves_current_source_pcm_and_legal_retune_without_heap() {
    use crate::{audio::Sample, engine::{Engine, Ahdsr},
        import::{Group, ModAssignment, ModSource, ModTarget, NativeLoop, SamplingProfile, Zone}};
    let group = Group { source_mode: Some(0), interp_quality: 0,
        mods: vec![ModAssignment { name: "volume lag".into(), source: ModSource::MidiCc(1),
            target: ModTarget::Volume, intensity: 0.5, invert: false, lag_ms: 13,
            cc_default: None, shaper: None }], ..Default::default() };
    let zone = Zone { native_loops: vec![NativeLoop { slot: 0, mode: 1,
        start: 256, length: 768, count: 0, alternating: false, tuning: 1., crossfade: 128 }],
        ..Default::default() };
    let bank = Bank::from_samples_with_profile(vec![group], vec![zone],
        vec![(Default::default(), Sample { rate: 48000, frames: vec![[0.25; 2]; 4096] })],
        SamplingProfile::Kontakt).unwrap()
        .with_prepared_settings(|settings| settings[0].envelope = Some(Ahdsr::UNITY)).unwrap();
    let mut engine = Engine::default(); engine.reset(48000.); engine.attack = 0.;
    engine.set_bank(Some(Box::new(bank))); engine.note_on(0, 60, 127); engine.cc(0, 1, 127);
    assert_eq!(engine.player.voices.len(), 1);
    let bank = engine.bank.as_ref().unwrap();
    let mut cx = Context::new(bank, &engine.state, &engine.player.midi,
        &engine.player.host_notes, &engine.player.per_note,
        engine.player.instrument_controls(&engine.state), false, 48000., 120.,
        &engine.player.stream_error, 0, false, 0.);
    cx.source_window = Some((0, 1024));
    let voice = &mut engine.player.voices[0];
    voice.step = (1u64 << 25) as f64;
    let mut scratch = [[0.; 2]; WINDOW];
    let positions = [0; MAX_BLOCK];
    assert_eq!(crate::plugin::tests::allocations(|| {
        let current = voice.prepare_source(&cx, 1).unwrap_or_else(|_| panic!("current source must be admitted"));
        assert!(current.curved_pitch, "volume-only lag takes the existing curved source preview");
        assert_eq!(current.travel, 1u64 << 57, "the current increment is representable");
        assert_eq!(current.crossfade_partners[0] & 1, 1,
            "current pending phase itself proves the first segment crosses fade.begin");
        assert!(voice.plan(&cx, 1, None, None).is_ok(), "current source must be admitted");
        let mut underrun = false;
        let (pcm, _) = voice.window(&cx, &mut scratch, &positions, &mut underrun);
        assert!(!underrun); assert_eq!(pcm[1], [0.25; 2]);
        // A future live retune must still be admitted; the earlier speculative
        // overflow neither silences nor commits a fabricated source trajectory.
        voice.step = 1.;
        let retuned = voice.prepare_source(&cx, 1).unwrap_or_else(|_| panic!("current source must be admitted"));
        assert_eq!(retuned.travel, 1u64 << 32);
        assert_eq!(voice.counted.unwrap().position, 0.);
        assert_eq!(voice.pos, 0.);
    }), 0);
    assert!(engine.player.stream_error.get().is_none());
}
