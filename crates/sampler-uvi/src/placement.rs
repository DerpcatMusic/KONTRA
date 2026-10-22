//! Stereo placement of channel-bundle members. Every read path (streamed,
//! dynamic and resident decode) takes its gains here and mixes them with
//! [`mix_member`], so no path keeps its own fold. Dependency-free on purpose:
//! the standalone witness example includes this file by path.

/// Equal-power spread from hard left (first member) to hard right (last), for
/// a bundle whose program gives no placement. Its power is `cos² + sin² = 1`.
pub(crate) fn even_spread(members: usize) -> Vec<[f64; 2]> {
    (0..members)
        .map(|k| {
            let position = if members == 1 { 0.5 } else { k as f64 / (members - 1) as f64 };
            let angle = position * std::f64::consts::FRAC_PI_2;
            [angle.cos(), angle.sin()]
        })
        .collect()
}

/// Stereo gain of each member of a wide bundle: the authored placement when
/// the program gives one, else the even spread. Every read path (streamed,
/// dynamic and resident decode) takes its gains from here and mixes them with
/// [`mix_member`], so no path keeps its own fold.
pub(crate) fn member_gains(placement: Option<&[[f64; 2]]>, members: usize) -> Result<Vec<[f32; 2]>, String> {
    let gains = match placement {
        Some(placed) if placed.len() != members => return Err("bundle placement differs from its member count".into()),
        Some(placed) => placed.to_vec(),
        None => even_spread(members),
    };
    Ok(gains.iter().map(|[left, right]| [*left as f32, *right as f32]).collect())
}

/// Adds one member's samples to a stereo mix through its placed gain.
pub(crate) fn mix_member(out: &mut [[f32; 2]], [left, right]: [f32; 2], samples: &[f32]) {
    for (frame, &s) in out.iter_mut().zip(samples) {
        frame[0] += left * s;
        frame[1] += right * s;
    }
}
