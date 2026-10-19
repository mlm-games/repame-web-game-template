use super::UiAction;
use repose_core::prelude::{Color, Dp, Modifier, StateColors, View};
use repose_core::text::TextAlign;
use repose_core::{AlignItems, JustifyContent, PaddingValues, Sp};
use repose_ui::{Box as RBox, Spacer, TextStyle as _, ViewExt as _};

pub const INK: Color = Color(0x14, 0x18, 0x22, 0xFF);
pub const SUN: Color = Color(0xFF, 0xC9, 0x3C, 0xFF);
pub const DANGER: Color = Color(0xE8, 0x4C, 0x3C, 0xFF);
pub const OK: Color = Color(0x62, 0xC3, 0x70, 0xFF);
pub const CYAN: Color = Color(0x46, 0xC8, 0xF5, 0xFF);
pub const CREAM: Color = Color(0xFF, 0xF6, 0xE8, 0xFF);
pub const STEEL: Color = Color(0x2C, 0x35, 0x48, 0xFF);
pub const STEEL_DARK: Color = Color(0x1C, 0x22, 0x30, 0xFF);
pub const PANEL: Color = Color(0x14, 0x18, 0x22, 0xD8);
pub const LOCKED: Color = Color(0x33, 0x3A, 0x48, 0xFF);
pub const SHADOW: Color = Color(0x00, 0x00, 0x00, 0x55);

pub const FONT: &str = "Fredoka";

pub const BUTTON_W: f32 = 210.0;
pub const BUTTON_H: f32 = 46.0;

pub fn mix(a: Color, b: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color(
        (a.0 as f32 + (b.0 as f32 - a.0 as f32) * t) as u8,
        (a.1 as f32 + (b.1 as f32 - a.1 as f32) * t) as u8,
        (a.2 as f32 + (b.2 as f32 - a.2 as f32) * t) as u8,
        (a.3 as f32 + (b.3 as f32 - a.3 as f32) * t) as u8,
    )
}

pub fn lighten(c: Color, t: f32) -> Color {
    mix(c, Color(0xFF, 0xFF, 0xFF, 0xFF), t)
}

pub fn rect(vp: [f32; 2], s: f32, anchor: [f32; 4], offset: [f32; 4]) -> Modifier {
    let left = anchor[0] * vp[0] + offset[0] * s;
    let top = anchor[1] * vp[1] + offset[1] * s;
    let right = anchor[2] * vp[0] + offset[2] * s;
    let bottom = anchor[3] * vp[1] + offset[3] * s;
    Modifier::new()
        .absolute()
        .offset_left(Dp(left))
        .offset_top(Dp(top))
        .offset_right(Dp((vp[0] - right).max(0.0)))
        .offset_bottom(Dp((vp[1] - bottom).max(0.0)))
}

pub fn label(m: Modifier, text: &str, size: f32, fill: Color, s: f32) -> View {
    repose_ui::Text(text.to_string())
        .size(Sp(size * s))
        .text_align(TextAlign::Center)
        .font_family(FONT)
        .color(fill)
        .outlined(0.12, INK)
        .single_line()
        .overflow_visible()
        .modifier(m)
}

pub fn label_plain(m: Modifier, text: &str, size: f32, fill: Color, s: f32) -> View {
    repose_ui::Text(text.to_string())
        .size(Sp(size * s))
        .text_align(TextAlign::Center)
        .font_family(FONT)
        .color(fill)
        .single_line()
        .overflow_visible()
        .modifier(m)
}

pub fn button(
    text: &str,
    bg: Color,
    size: f32,
    s: f32,
    action: UiAction,
    sink: &super::Sink,
    disabled: bool,
) -> View {
    let mut m = Modifier::new()
        .width(Dp(BUTTON_W * s))
        .height(Dp(BUTTON_H * s))
        .background(bg)
        .state_colors(StateColors {
            default: bg,
            hovered: lighten(bg, 0.08),
            focused: lighten(bg, 0.04),
            pressed: mix(bg, INK, 0.18),
            disabled: mix(bg, INK, 0.35),
            dragged: mix(bg, INK, 0.18),
        })
        .border(Dp(2.5 * s), INK, Dp(13.0 * s))
        .shadow_with_color(Dp(0.0), Dp(4.0 * s), SHADOW)
        .align_items(AlignItems::CENTER)
        .justify_content(JustifyContent::CENTER)
        .focusable(true);
    if disabled {
        m = m.disabled();
    } else {
        m = m.on_click({
            let sink = sink.clone();
            move || sink.lock().unwrap().push(action)
        });
    }
    RBox(m).child(label(Modifier::new(), text, size, INK, s))
}

pub fn bar(frac: f32, fg: Color, s: f32, width: f32, height: f32) -> View {
    let frac = frac.clamp(0.0, 1.0);
    repose_ui::ZStack(
        Modifier::new()
            .width(Dp(width * s))
            .height(Dp(height * s))
            .background(STEEL_DARK)
            .clip_rounded(Dp(height * s * 0.5)),
    )
    .child((
        RBox(
            Modifier::new()
                .width(Dp(width * s * frac))
                .height(Dp(height * s))
                .background(fg)
                .clip_rounded(Dp(height * s * 0.5)),
        )
        .child(Spacer()),
        Spacer(),
    ))
}

pub fn panel(vp: [f32; 2], s: f32, content: View) -> View {
    padded_panel(vp, s, content, 20.0)
}

pub fn padded_panel(vp: [f32; 2], s: f32, content: View, pad: f32) -> View {
    RBox(
        Modifier::new()
            .fill_max_size()
            .justify_content(JustifyContent::CENTER)
            .align_items(AlignItems::CENTER),
    )
    .child(
        RBox(
            Modifier::new()
                .background(PANEL)
                .clip_rounded(Dp(20.0 * s))
                .padding_values(PaddingValues {
                    left: Dp(pad * s),
                    right: Dp(pad * s),
                    top: Dp(pad * s),
                    bottom: Dp(pad * s),
                })
                .max_width(Dp(vp[0] * 0.94)),
        )
        .child(content),
    )
}
