
use repose_core::prelude::{AlignItems, Color, Dp, Modifier, View};
use repose_core::{JustifyContent, PaddingValues};
use repose_ui::{Box as RBox, Column, ViewExt as _};

use super::palette::*;
use super::{SharedUi, Sink, UiAction};

pub fn compose_main(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let content = Column(
        Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        centered("WEB GAME", 62.0, s),
        centered("COLLECT ORBS BEFORE THE CLOCK RUNS OUT", 18.0, s),
        chip_row("BEST", &ui.best.to_string(), GOLD, s),
        button("PLAY", GOLD, 34.0, s, UiAction::Play, sink, false),
        button(
            if ui.muted { "SOUND OFF" } else { "SOUND ON" },
            CORAL,
            20.0,
            s,
            UiAction::Mute,
            sink,
            false,
        ),
    ));
    panel(vp, s, content)
}

pub fn compose_death(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let can_revive = ui.revives_left > 0 && !ui.revive_pending;
    let content = Column(
        Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        centered("TIME UP", 56.0, s),
        chip_row("SCORE", &ui.score.to_string(), GOLD, s),
        chip_row("BEST", &ui.best.to_string(), CREAM, s),
        if ui.record {
            centered("NEW RECORD", 21.0, s)
        } else {
            centered("", 21.0, s)
        },
        button(
            if ui.revive_pending {
                "LOADING AD…"
            } else {
                "REVIVE (WATCH AD)"
            },
            GOLD,
            25.0,
            s,
            UiAction::Revive,
            sink,
            !can_revive,
        ),
        button("RETRY", CYAN, 25.0, s, UiAction::Retry, sink, false),
        button("MENU", CORAL, 20.0, s, UiAction::Menu, sink, false),
    ));
    panel(vp, s, content)
}

pub fn compose_pause(ui: &SharedUi, s: f32, vp: [f32; 2], sink: &Sink) -> View {
    let content = Column(
        Modifier::new()
            .gap(Dp(10.0 * s))
            .align_items(AlignItems::CENTER),
    )
    .child((
        centered("PAUSED", 56.0, s),
        chip_row("SCORE", &ui.score.to_string(), GOLD, s),
        button("RESUME", GREEN, 28.0, s, UiAction::Pause, sink, false),
        button("MENU", CORAL, 20.0, s, UiAction::Menu, sink, false),
    ));
    panel(vp, s, content)
}

fn panel(vp: [f32; 2], s: f32, content: View) -> View {
    let card = RBox(
        Modifier::new()
            .align_self_center()
            .background(CREAM)
            .border(Dp(4.0 * s), INK, Dp(24.0 * s))
            .shadow_with_color(Dp(0.0), Dp(6.0 * s), BUTTON_SHADOW)
            .padding_values(PaddingValues {
                left: Dp(44.0 * s),
                right: Dp(44.0 * s),
                top: Dp(30.0 * s),
                bottom: Dp(34.0 * s),
            }),
    )
    .child(content);
    let _ = vp;
    RBox(
        Modifier::new()
            .fill_max_size()
            .background(SCRIM)
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER)
            .input_blocker(),
    )
    .child(card)
}

fn centered(text: &str, size: f32, s: f32) -> View {
    label_plain(Modifier::new(), text, size, INK, s)
}

fn chip_row(label_text: &str, value: &str, bg: Color, s: f32) -> View {
    RBox(chip(bg, s)).child(label_plain(
        Modifier::new(),
        &format!("{label_text}: {value}"),
        22.0,
        INK,
        s,
    ))
}

fn button(
    text: &str,
    bg: Color,
    font_size: f32,
    s: f32,
    action: UiAction,
    sink: &Sink,
    disabled: bool,
) -> View {
    let mut m = chunky(bg, 24.0, 11.0, s);
    if disabled {
        m = m.disabled();
    } else {
        m = m.on_click({
            let sink = sink.clone();
            move || sink.lock().unwrap().push(action)
        });
    }
    RBox(m).child(label_plain(Modifier::new(), text, font_size, INK, s))
}
