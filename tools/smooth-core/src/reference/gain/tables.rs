//! `.rdata` tables the gain body reads (read from the restored binary, f32 bit patterns).

/// `0x209a800`: low-band weights of the floor estimate (`0x8e199b`, DotProd over 32 bins).
pub const T800: [f32; 32] = [
    f32::from_bits(0x3d4448a8), f32::from_bits(0x3d440c21), f32::from_bits(0x3d4356b2), f32::from_bits(0x3d4228ca),
    f32::from_bits(0x3d408325), f32::from_bits(0x3d3e66c4), f32::from_bits(0x3d3bd4f7), f32::from_bits(0x3d38cf53),
    f32::from_bits(0x3d3557b5), f32::from_bits(0x3d31703f), f32::from_bits(0x3d2d1b5c), f32::from_bits(0x3d285bb6),
    f32::from_bits(0x3d23343b), f32::from_bits(0x3d1da819), f32::from_bits(0x3d17babd), f32::from_bits(0x3d116fcc),
    f32::from_bits(0x3d0acb2a), f32::from_bits(0x3d03d0ef), f32::from_bits(0x3cf90ad2), f32::from_bits(0x3ce9da2e),
    f32::from_bits(0x3cda1951), f32::from_bits(0x3cc9d1f2), f32::from_bits(0x3cb90e1b), f32::from_bits(0x3ca7d823),
    f32::from_bits(0x3c963aa8), f32::from_bits(0x3c844086), f32::from_bits(0x3c63e9a9), f32::from_bits(0x3c3ec5b6),
    f32::from_bits(0x3c192c1b), f32::from_bits(0x3be66813), f32::from_bits(0x3b99e9d6), f32::from_bits(0x3b1a195b),
];

/// `0x209a900`: band tilt, added with weight `(b/a)*a` (`0x8e2e44`).
pub const T900: [f32; 16] = [
    f32::from_bits(0x3f7ffec2), f32::from_bits(0x3f7ff243), f32::from_bits(0x3f7fad05), f32::from_bits(0x3f7e7d7d),
    f32::from_bits(0x3f79ee42), f32::from_bits(0x3f6a6e2f), f32::from_bits(0x3f401d26), f32::from_bits(0x3eec4fc8),
    f32::from_bits(0x3e4c308b), f32::from_bits(0x3d8bd783), f32::from_bits(0x3cae53ca), f32::from_bits(0x3bd32bdb),
    f32::from_bits(0x3afe0d07), f32::from_bits(0x3a18b4ee), f32::from_bits(0x3937b45f), f32::from_bits(0x385d2c1e),
];

/// `0x209a940`: band tilt dot product `b` (`0x8e2e2b`).
pub const T940: [f32; 16] = [
    f32::from_bits(0x331c826c), f32::from_bits(0x3570f425), f32::from_bits(0x3725e208), f32::from_bits(0x38a24069),
    f32::from_bits(0x3a048c53), f32::from_bits(0x3b419efe), f32::from_bits(0x3c7d8809), f32::from_bits(0x3d828767),
    f32::from_bits(0x3e1bf7eb), f32::from_bits(0x3e549496), f32::from_bits(0x3e64094d), f32::from_bits(0x3e565124),
    f32::from_bits(0x3dc93536), f32::from_bits(0x3cb29618), f32::from_bits(0x3b9083cb), f32::from_bits(0x3a6936cd),
];

/// `0x209a980`: band tilt dot product `a` (`0x8e2dfd`).
pub const T980: [f32; 16] = [
    f32::from_bits(0x3e0a7923), f32::from_bits(0x3e0a7260), f32::from_bits(0x3e0a4cec), f32::from_bits(0x3e09a8bc),
    f32::from_bits(0x3e07315b), f32::from_bits(0x3dfd9de1), f32::from_bits(0x3dcfd63f), f32::from_bits(0x3d7fa6e4),
    f32::from_bits(0x3cdce6a6), f32::from_bits(0x3c17496c), f32::from_bits(0x3b3c983b), f32::from_bits(0x3a64743c),
    f32::from_bits(0x39896be7), f32::from_bits(0x38a53462), f32::from_bits(0x37c6bd48), f32::from_bits(0x36ef4609),
];

/// `0x209a9c0`: band tilt, added with weight `(a/b)*b*(1-focus)` (`0x8e2e6d`).
pub const T9C0: [f32; 16] = [
    f32::from_bits(0x342bcc75), f32::from_bits(0x36843eff), f32::from_bits(0x3836167b), f32::from_bits(0x39b21a0c),
    f32::from_bits(0x3b117f21), f32::from_bits(0x3c548921), f32::from_bits(0x3d8b2637), f32::from_bits(0x3e8f47b5),
    f32::from_bits(0x3f2b346d), f32::from_bits(0x3f6958dd), f32::from_bits(0x3f7a501b), f32::from_bits(0x3f6b40d8),
    f32::from_bits(0x3edcdd14), f32::from_bits(0x3dc4083b), f32::from_bits(0x3c9ea1dd), f32::from_bits(0x3b7fff17),
];

