//! Minimal offscreen-render probe: one white background, one text run, one box.
//! `cargo run --example offscreen_probe -- /tmp/probe.png`

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use repose_core::Dp;
use repose_core::RenderContext;
use repose_core::prelude::{AlignItems, Color, JustifyContent, Modifier};
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
use repose_ui::{
    Interactions, Text, TextFieldState, TextStyle as _, ViewExt as _, ZStack, layout_and_paint,
};

const W: u32 = 320;
const H: u32 = 180;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/probe.png".into());
    let ctx = RenderContext::new();
    let view = ZStack(
        Modifier::new()
            .fill_max_size()
            .background(Color(0x20, 0x30, 0x40, 0xFF))
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER),
    )
    .child((
        repose_ui::Box(
            Modifier::new()
                .width(Dp(40.0))
                .height(Dp(20.0))
                .background(Color(0xFF, 0x40, 0x40, 0xFF)),
        )
        .child(repose_ui::Spacer()),
        Text("PROBE".to_string())
            .size(repose_core::Sp(24.0))
            .color(Color::WHITE),
    ));

    let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
    let (scene, hits, sems) =
        layout_and_paint(&view, (W, H), &states, &Interactions::default(), None);
    eprintln!("hit regions {} / sem nodes {}", hits.len(), sems.len());

    let mut gpu = OffscreenRenderer::new_blocking(W, H, 1)?;
    apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
    let rgba = gpu.render_rgba(&scene, Some([0.05, 0.06, 0.08, 1.0]))?;
    image::save_buffer(&path, &rgba, W, H, image::ColorType::Rgba8)?;
    eprintln!("wrote {path}");
    Ok(())
}
