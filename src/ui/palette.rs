//! Palette and widget styles. Every size is a design unit on the 960x540
//! grid; callers scale by `ui_scale`.

use super::UiAction;
use repose_core::prelude::{Color, Modifier, StateColors, View};
use repose_core::text::TextAlign;
use repose_core::{AlignItems, Dp, JustifyContent, PaddingValues, Sp};
use repose_ui::{Box as RBox, Spacer, TextStyle as _, ViewExt as _};

/// The outline color: every sprite, chip and button wears it, ~3 px at the
/// design grid. Blumgi art is literally this navy line plus flat fills.
pub const INK: Color = Color(0x1E, 0x2A, 0x4A, 0xFF);
/// The sky the demo plays on, and the page background.
pub const SKY: Color = Color(0x7F, 0xC9, 0xF2, 0xFF);
/// Sibling sky tone: rays, hills, anything behind the play field.
pub const SKY_DARK: Color = Color(0x5F, 0xAC, 0xDE, 0xFF);
pub const CORAL: Color = Color(0xF4, 0x71, 0x6A, 0xFF);
pub const GOLD: Color = Color(0xFF, 0xC9, 0x3C, 0xFF);
pub const GOLD_DARK: Color = Color(0xE0, 0x9B, 0x1F, 0xFF);
pub const CYAN: Color = Color(0x46, 0xC8, 0xF5, 0xFF);
pub const PINK: Color = Color(0xFF, 0x9E, 0xB1, 0xFF);
pub const GREEN: Color = Color(0x7B, 0xD8, 0x8F, 0xFF);
/// Cream: the text, the panels, the card fills.
pub const CREAM: Color = Color(0xFF, 0xF8, 0xEA, 0xFF);
/// Panel scrim: the ink at partial alpha, so the world stays readable.
pub const SCRIM: Color = Color(0x1E, 0x2A, 0x4A, 0x66);
pub const BUTTON_SHADOW: Color = Color(0x1E, 0x2A, 0x4A, 0x33);
pub const TOUCH_BG: Color = Color(0xFF, 0xF8, 0xEA, 0x99);
pub const TOUCH_RING: Color = Color(0x1E, 0x2A, 0x4A, 0xFF);
pub const TOUCH_SHADOW: Color = Color(0x1E, 0x2A, 0x4A, 0x22);
pub const TOUCH_KNOB: Color = Color(0xFF, 0xFF, 0xFF, 0xFF);
pub const TOUCH_KNOB_DARK: Color = Color(0xC9, 0xD8, 0xEE, 0xFF);
pub const TOUCH_CENTER: Color = Color(0x1E, 0x2A, 0x4A, 0xFF);

pub const FONT: &str = "Fredoka";

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

/// `anchor_*` on a full-rect parent plus the matching `offset_*`: an absolute
/// box stretched between the four edges.
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

/// Chunky cream pill with an ink hairline: the HUD readouts.
pub fn chip(bg: Color, s: f32) -> Modifier {
    Modifier::new()
        .background(bg)
        .border(Dp(2.5 * s), INK, Dp(14.0 * s))
        .padding_values(PaddingValues {
            left: Dp(14.0 * s),
            right: Dp(14.0 * s),
            top: Dp(6.0 * s),
            bottom: Dp(6.0 * s),
        })
}

/// The button face: solid fill, ink outline, heavy radius, offset shadow.
pub fn chunky(bg: Color, pad_x: f32, pad_y: f32, s: f32) -> Modifier {
    Modifier::new()
        .state_colors(StateColors {
            default: bg,
            hovered: lighten(bg, 0.08),
            focused: lighten(bg, 0.04),
            pressed: mix(bg, INK, 0.16),
            disabled: mix(bg, INK, 0.30),
            dragged: mix(bg, INK, 0.16),
        })
        .border(Dp(3.0 * s), INK, Dp(16.0 * s))
        .padding_values(PaddingValues {
            left: Dp(pad_x * s),
            right: Dp(pad_x * s),
            top: Dp(pad_y * s),
            bottom: Dp(pad_y * s),
        })
        .shadow_with_color(Dp(0.0), Dp(5.0 * s), BUTTON_SHADOW)
        .align_items(AlignItems::CENTER)
        .justify_content(JustifyContent::CENTER)
        .focusable(true)
}

/// A label in the game font: cream fill, ink stroke sitting on the glyph
/// outline, which is what makes the text read as part of the art. For text
/// that sits on the sky.
pub fn label(m: Modifier, text: &str, size: f32, fill: Color, s: f32) -> View {
    let outline = (size / 3.0).max(2.5);
    repose_ui::Text(text.to_string())
        .size(Sp(size * s))
        .text_align(TextAlign::Center)
        .font_family(FONT)
        .color(fill)
        // The stroke is centred on the glyph outline, so widen the em to grow
        // `outline` design units outward.
        .outlined(0.42 * outline / size, INK)
        .single_line()
        .overflow_visible()
        .modifier(m)
}

/// The same font with no stroke, for text on a cream card: there the ink is
/// the card's border, not the glyph's.
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

/// Disc of a solid colour, the same `draw_circle` stack laid out in dp.
pub fn disc(vp: [f32; 2], s: f32, at: [f32; 2], r: f32, color: Color) -> View {
    RBox(
        rect(
            vp,
            s,
            [0.0; 4],
            [at[0] - r, at[1] - r, at[0] + r, at[1] + r],
        )
        .background(color)
        .clip_rounded(Dp(r * s))
        .hit_passthrough(),
    )
    .child(Spacer())
}

/// Side of a corner icon button, in design units.
pub const ICON_BUTTON: f32 = 46.0;
/// The ligature em size inside it.
const ICON_GLYPH: f32 = 26.0;
/// The family name repose registers the bundled face under.
pub const MATERIAL_SYMBOLS: &str = "Material Symbols Outlined";

/// A Material Symbols ligature (`pause`, `volume_up`, `menu`, …). Repose
/// bundles the Outlined face and registers it, so no font ships with the
/// game. No glyph stroke: the ligature is already an outline drawing, and the
/// stroke would fill its counters.
pub fn icon(name: &str, size: f32, s: f32) -> View {
    repose_ui::Text(name.to_string())
        .size(Sp(size * s))
        .text_align(TextAlign::Center)
        .font_family(MATERIAL_SYMBOLS)
        .color(INK)
        .single_line()
        .overflow_visible()
        .modifier(Modifier::new())
}

/// The square icon button the video's corner cluster is made of: cream face,
/// ink outline, even gap to its neighbours.
pub fn icon_button(
    name: &str,
    s: f32,
    action: UiAction,
    sink: &super::Sink,
    disabled: bool,
) -> View {
    let mut m = Modifier::new()
        .size(Dp(ICON_BUTTON * s), Dp(ICON_BUTTON * s))
        .background(CREAM)
        .border(Dp(3.0 * s), INK, Dp(12.0 * s))
        .state_colors(StateColors {
            default: CREAM,
            hovered: lighten(CREAM, 0.06),
            focused: lighten(CREAM, 0.03),
            pressed: mix(CREAM, INK, 0.14),
            disabled: mix(CREAM, INK, 0.30),
            dragged: mix(CREAM, INK, 0.14),
        })
        .shadow_with_color(Dp(0.0), Dp(4.0 * s), BUTTON_SHADOW)
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
    RBox(m).child(icon(name, ICON_GLYPH, s))
}
