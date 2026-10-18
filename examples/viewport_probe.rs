//! Is a UI scene with a 3D viewport callback in it painted once?
//!
//! `cargo run --example viewport_probe -- /tmp/vp.png [groups]`
//!
//! `groups=0` keeps the callback with an empty batch; `groups=1` gives it a
//! single triangle. The label above the callback is the tell: it must appear
//! exactly once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use repame_view3d::mesh::MeshGroup;
use repame_view3d::render::{BatchDesc, SceneFilter};
use repame_view3d::{Frame3d, GeomHandle, Viewport3d};
use repose_core::RenderContext;
use repose_core::prelude::{AlignItems, Color, JustifyContent, Modifier};
use repose_core::{Dp, View};
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
        .unwrap_or_else(|| "/tmp/vp.png".into());
    let groups: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);

    let ctx = RenderContext::new();
    let geom = GeomHandle::new();
    let desc = BatchDesc {
        layer_size: 128,
        layers: 1,
        filter: SceneFilter::Linear,
        mips: 1,
    };
    let mut frame = Frame3d {
        desc,
        ..Frame3d::default()
    };
    for i in 0..groups {
        let mut g = MeshGroup::default();
        let x = i as f32;
        g.positions = vec![[x, 0.0, 0.0], [x + 1.0, 0.0, 0.0], [x, 1.0, 0.0]];
        g.colors = vec![[1.0, 0.0, 0.0]; 3];
        g.indices = vec![0, 1, 2];
        frame.push(g);
    }
    let viewport = Viewport3d(frame, geom.clone(), "probe", desc, |_| {});

    let view: View = ZStack(
        Modifier::new()
            .fill_max_size()
            .background(Color(0x20, 0x30, 0x40, 0xFF))
            .align_items(AlignItems::CENTER)
            .justify_content(JustifyContent::CENTER),
    )
    .child((
        viewport,
        Text("PROBE")
            .size(repose_core::Sp(20.0))
            .color(Color::WHITE),
        repose_ui::Box(
            Modifier::new()
                .width(Dp(20.0))
                .height(Dp(10.0))
                .background(Color(0xFF, 0x40, 0x40, 0xFF)),
        )
        .child(repose_ui::Spacer()),
    ));

    let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
    let (scene, hits, sems) =
        layout_and_paint(&view, (W, H), &states, &Interactions::default(), None);
    eprintln!("groups={groups} hits={} sems={}", hits.len(), sems.len());

    let mut gpu = OffscreenRenderer::new_blocking(W, H, 1)?;
    apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
    let rgba = gpu.render_rgba(&scene, Some([0.05, 0.06, 0.08, 1.0]))?;
    image::save_buffer(&path, &rgba, W, H, image::ColorType::Rgba8)?;
    eprintln!("wrote {path}");
    Ok(())
}
