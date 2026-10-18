use repose_core::prelude::{Color, Modifier, View};
use repose_core::{AlignItems, Dp};
use repose_ui::{Box as RBox, Row, Spacer, ViewExt as _, ZStack};

use super::palette::*;
use super::{SharedUi, Sink, UiAction};

pub fn compose(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    if !matches!(ui.phase, crate::sim::state::Phase::Playing) {
        return Spacer();
    }
    let cluster_w = 3.0 * ICON_BUTTON + 2.0 * 12.0 + 24.0;
    let chips = Row(
        rect(vp, s, [0.0, 0.0, 1.0, 0.0], [20.0, 10.0, -cluster_w, 0.0])
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::START),
    )
    .child((
        chip_view("SCORE", &ui.score.to_string(), GOLD, s),
        chip_view(
            "TIME",
            &format!("{:02}", ui.time_left.ceil().max(0.0) as i32),
            CREAM,
            s,
        ),
        chip_view("BEST", &ui.best.to_string(), CREAM, s),
    ));
    let cluster = Row(rect(
        vp,
        s,
        [1.0, 0.0, 1.0, 0.0],
        [-(3.0 * ICON_BUTTON + 2.0 * 12.0), 10.0, -12.0, 0.0],
    )
    .gap(Dp(12.0 * s))
    .align_items(AlignItems::START))
    .child((
        icon_button("pause", s, UiAction::Pause, sink, ui.paused),
        icon_button(
            if ui.muted { "volume_off" } else { "volume_up" },
            s,
            UiAction::Mute,
            sink,
            false,
        ),
        icon_button("menu", s, UiAction::Menu, sink, false),
    ));
    ZStack(Modifier::new().fill_max_size()).child(vec![chips, cluster])
}

fn chip_view(label_text: &str, value: &str, bg: Color, s: f32) -> View {
    RBox(chip(bg, s)).child(
        Row(Modifier::new()
            .gap(Dp(7.0 * s))
            .align_items(AlignItems::CENTER))
        .child((
            label(Modifier::new(), label_text, 15.0, CREAM, s),
            label(Modifier::new(), value, 27.0, CREAM, s),
        )),
    )
}
