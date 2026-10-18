//! UI sprite + label probe: does one image from the shipped skin, drawn once,
//! paint once? `cargo run --example image_probe -- /tmp/img.png [sprite]`

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use deadly_descent::ui::SharedUi;
use repose_core::RenderContext;
use repose_core::prelude::{AlignItems, Color, ImageFit, JustifyContent, Modifier};
use repose_core::{Dp, View};
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
use repose_ui::{
    Image, ImageExt as _, Interactions, Text, TextFieldState, TextStyle as _, ViewExt as _, ZStack,
    layout_and_paint,
};

const W: u32 = 320;
const H: u32 = 180;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/img.png".into());
    let name = std::env::args().nth(2).unwrap_or_else(|| "Star".into());

    let ctx = RenderContext::new();
    let mut ui = SharedUi {
        viewport: glam::Vec2::new(W as f32, H as f32),
        ..SharedUi::default()
    };
    ui.images.ensure(&ctx);
    let handle = ui.images.sprite(&name);
    eprintln!(
        "sprite {name}: {}",
        if handle.is_some() { "found" } else { "missing" }
    );

    let art: View = match handle {
        Some(handle) => Image(Modifier::new().width(Dp(48.0)).height(Dp(48.0)), handle)
            .image_fit(ImageFit::Contain),
        None => repose_ui::Box(
            Modifier::new()
                .width(Dp(48.0))
                .height(Dp(48.0))
                .background(Color(0xFF, 0x80, 0x00, 0xFF)),
        )
        .child(repose_ui::Spacer()),
    };

    let view = ZStack(
        Modifier::new()
            .fill_max_size()
            .background(Color(0x20, 0x30, 0x40, 0xFF))
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER),
    )
    .child((
        art,
        Text("PROBE".to_string())
            .size(repose_core::Sp(20.0))
            .color(Color::WHITE),
    ));

    let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
    let (scene, _hits, _sems) =
        layout_and_paint(&view, (W, H), &states, &Interactions::default(), None);

    let mut gpu = OffscreenRenderer::new_blocking(W, H, 1)?;
    apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
    let rgba = gpu.render_rgba(&scene, Some([0.05, 0.06, 0.08, 1.0]))?;
    image::save_buffer(&path, &rgba, W, H, image::ColorType::Rgba8)?;
    eprintln!("wrote {path}");
    Ok(())
}
