//! Line icons on a 16-unit square: crisp at any size, no icon font.

use std::f64::consts::PI;

use mui::prelude::*;

/// The icons the editor draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Left,
    Right,
    Down,
    More,
    Close,
    Sidebar,
    Search,
    Star,
    Menu,
    Warning,
    Grip,
    Piano,
    Clock,
    Filter,
    Plus,
    /// A MIDI socket: the part's channel.
    Midi,
    /// A wave and an arrow: the part's output.
    Output,
    /// A tuning fork.
    Tune,
}

/// `icon` in `ink`, `size` points square.
pub fn glyph(icon: Icon, size: f64, ink: Fill) -> El {
    canvas(move |s| {
        let u = s.width.min(s.height) / 16.;
        let (ox, oy) = ((s.width - 16. * u) / 2., (s.height - 16. * u) / 2.);
        let p = |&(x, y): &(f64, f64)| Point::new(ox + x * u, oy + y * u);
        let line = |pts: &[(f64, f64)], closed: bool| {
            Draw::stroke(
                Path::polyline(pts.iter().map(p), closed),
                ink.clone(),
                1.5 * u.max(0.75),
            )
        };
        let shape =
            |pts: &[(f64, f64)]| Draw::fill(Path::polyline(pts.iter().map(p), true), ink.clone());
        let dot = |x: f64, y: f64| {
            shape(&[
                (x - 1., y - 1.),
                (x + 1., y - 1.),
                (x + 1., y + 1.),
                (x - 1., y + 1.),
            ])
        };
        let round = |cx: f64, cy: f64, r: f64| -> Vec<(f64, f64)> {
            (0..24)
                .map(|n| f64::from(n) * PI / 12.)
                .map(|a| (cx + r * a.cos(), cy + r * a.sin()))
                .collect()
        };
        match icon {
            Icon::Left => vec![line(&[(10., 4.), (6., 8.), (10., 12.)], false)],
            Icon::Right => vec![line(&[(6., 4.), (10., 8.), (6., 12.)], false)],
            Icon::Down => vec![line(&[(4., 6.), (8., 10.), (12., 6.)], false)],
            Icon::More => vec![dot(3.5, 8.), dot(8., 8.), dot(12.5, 8.)],
            Icon::Close => vec![
                line(&[(4.5, 4.5), (11.5, 11.5)], false),
                line(&[(11.5, 4.5), (4.5, 11.5)], false),
            ],
            Icon::Sidebar => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5)], true),
                line(&[(6.5, 3.5), (6.5, 12.5)], false),
            ],
            Icon::Search => vec![
                line(&round(7., 7., 4.), true),
                line(&[(10., 10.), (13.5, 13.5)], false),
            ],
            Icon::Star => {
                let points: Vec<_> = (0..10)
                    .map(|n| {
                        let (r, a) = (
                            if n % 2 == 0 { 6. } else { 2.6 },
                            -PI / 2. + f64::from(n) * PI / 5.,
                        );
                        (8. + r * a.cos(), 8.6 + r * a.sin())
                    })
                    .collect();
                vec![line(&points, true)]
            }
            Icon::Menu => [4.5, 8., 11.5]
                .map(|y| line(&[(3., y), (13., y)], false))
                .into(),
            Icon::Warning => vec![
                line(&[(8., 2.5), (14., 13.), (2., 13.)], true),
                line(&[(8., 6.5), (8., 9.5)], false),
                dot(8., 11.3),
            ],
            Icon::Grip => [
                (6., 4.),
                (10., 4.),
                (6., 8.),
                (10., 8.),
                (6., 12.),
                (10., 12.),
            ]
            .map(|(x, y)| dot(x, y))
            .into(),
            Icon::Piano => vec![
                line(&[(2.5, 3.5), (13.5, 3.5), (13.5, 12.5), (2.5, 12.5)], true),
                line(&[(6.2, 3.5), (6.2, 12.5)], false),
                line(&[(9.8, 3.5), (9.8, 12.5)], false),
            ],
            Icon::Clock => vec![
                line(&round(8., 8., 5.5), true),
                line(&[(8., 5.), (8., 8.), (10.5, 9.5)], false),
            ],
            Icon::Filter => vec![line(
                &[
                    (2.5, 3.5),
                    (13.5, 3.5),
                    (9., 8.5),
                    (9., 13.),
                    (7., 12.),
                    (7., 8.5),
                ],
                true,
            )],
            Icon::Plus => vec![
                line(&[(8., 3.), (8., 13.)], false),
                line(&[(3., 8.), (13., 8.)], false),
            ],
            Icon::Midi => vec![
                line(&round(6.5, 8., 4.5), true),
                dot(4.8, 8.),
                dot(8.2, 8.),
                dot(6.5, 6.),
                line(&[(11., 8.), (14.5, 8.)], false),
            ],
            Icon::Output => vec![
                line(
                    &[
                        (1.5, 8.),
                        (3., 5.),
                        (4.5, 11.),
                        (6., 4.),
                        (7.5, 12.),
                        (9., 8.),
                    ],
                    false,
                ),
                line(&[(10., 8.), (14.5, 8.)], false),
                line(&[(12.5, 6.), (14.5, 8.), (12.5, 10.)], false),
            ],
            Icon::Tune => vec![
                line(&[(6., 2.5), (6., 8.5), (10., 8.5), (10., 2.5)], false),
                line(&[(8., 8.5), (8., 13.5)], false),
            ],
        }
    })
    .square(size)
    .shrink(0)
}
