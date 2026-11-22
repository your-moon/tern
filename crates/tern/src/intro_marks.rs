// Adapted from zeron apps/zgx/ui/intro.luau, AGENTS, agent() and rise() (MIT).
//! Full-color card rotations are baked into six-column atlases for GPUI's image primitive.
use gpui::{Div, ParentElement as _, Styled as _, div, img, px, relative};

struct Mark {
    asset: &'static str,
    position: (f32, f32),
    size: f32,
    fly: (f32, f32),
    delay: f32,
    period: f32,
    drift: (f32, f32),
    wobble: f32,
    first_angle: i32,
}

const MARKS: &[Mark] = &[
    Mark {
        asset: "intro/marks/remote-server.webp",
        position: (-0.33, 0.2),
        size: 96.0,
        fly: (-160.0, -60.0),
        delay: 0.38,
        period: 10.5,
        drift: (4.0, -12.0),
        wobble: -2.0,
        first_angle: -2,
    },
    Mark {
        asset: "intro/marks/terminal.webp",
        position: (0.28, 0.12),
        size: 84.0,
        fly: (140.0, -90.0),
        delay: 0.46,
        period: 9.2,
        drift: (-4.0, -10.0),
        wobble: 2.0,
        first_angle: 0,
    },
    Mark {
        asset: "intro/marks/folder.webp",
        position: (-0.08, 0.1),
        size: 48.0,
        fly: (-30.0, -100.0),
        delay: 0.62,
        period: 8.5,
        drift: (2.0, -8.0),
        wobble: 3.0,
        first_angle: 0,
    },
    Mark {
        asset: "intro/marks/key-minimalistic.webp",
        position: (0.11, 0.21),
        size: 56.0,
        fly: (50.0, -80.0),
        delay: 0.58,
        period: 9.8,
        drift: (-2.0, -9.0),
        wobble: -3.0,
        first_angle: -3,
    },
    Mark {
        asset: "intro/marks/magnifer.webp",
        position: (-0.44, 0.46),
        size: 64.0,
        fly: (-200.0, 10.0),
        delay: 0.66,
        period: 10.1,
        drift: (3.0, -8.0),
        wobble: 2.0,
        first_angle: 0,
    },
    Mark {
        asset: "intro/marks/file-code.webp",
        position: (0.21, 0.71),
        size: 60.0,
        fly: (80.0, 120.0),
        delay: 0.5,
        period: 8.8,
        drift: (-3.0, -10.0),
        wobble: -2.0,
        first_angle: -2,
    },
    Mark {
        asset: "intro/marks/cloud.webp",
        position: (0.41, 0.4),
        size: 76.0,
        fly: (190.0, -20.0),
        delay: 0.7,
        period: 7.8,
        drift: (-3.0, -11.0),
        wobble: 3.0,
        first_angle: 0,
    },
    Mark {
        asset: "intro/marks/keyboard.webp",
        position: (-0.17, 0.67),
        size: 68.0,
        fly: (-90.0, 110.0),
        delay: 0.54,
        period: 11.5,
        drift: (3.0, -9.0),
        wobble: -3.0,
        first_angle: -3,
    },
    Mark {
        asset: "intro/marks/archive-up-minimalistic.webp",
        position: (0.39, 0.6),
        size: 52.0,
        fly: (170.0, 90.0),
        delay: 0.74,
        period: 9.5,
        drift: (2.0, -7.0),
        wobble: 2.0,
        first_angle: 0,
    },
    Mark {
        asset: "intro/marks/settings.webp",
        position: (-0.36, 0.82),
        size: 80.0,
        fly: (-140.0, 140.0),
        delay: 0.78,
        period: 10.8,
        drift: (-2.0, -8.0),
        wobble: -2.0,
        first_angle: -2,
    },
];

pub(super) fn scene(time: f32) -> Div {
    let mut layer = div()
        .absolute()
        .inset_0()
        .image_cache(gpui::retain_all("intro-marks"));
    for mark in MARKS {
        let elapsed = time - (4.2 + mark.delay);
        if elapsed < 0. {
            continue;
        }
        let fly = (1. - elapsed.min(1.)).powi(4);
        let phase = (elapsed - 1.).max(0.) * std::f32::consts::TAU / mark.period;
        let drift = (1. - phase.cos()) / 2.;
        let angle = (24. * fly + mark.wobble * drift).round() as i32;
        let slot = angle - mark.first_angle;
        let rows = (25 - mark.first_angle + 5) / 6;
        let side = (mark.size + 112.) * (1. - 0.4 * fly);
        let card = div()
            .absolute()
            .size(px(side))
            .overflow_hidden()
            .left(relative(0.5 + mark.position.0))
            .top(relative(mark.position.1))
            .ml(px(mark.fly.0 * fly + mark.drift.0 * drift - side / 2.))
            .mt(px(mark.fly.1 * fly + mark.drift.1 * drift - side / 2.))
            .opacity(1. - fly)
            .child(
                img(mark.asset)
                    .absolute()
                    .w(px(side * 6.))
                    .h(px(side * rows as f32))
                    .left(px(-side * (slot % 6) as f32))
                    .top(px(-side * (slot / 6) as f32)),
            );
        layer = layer.child(card);
    }
    layer
}
