//! Offscreen-render probe for the game's own UI: composes the garage through
//! `ui::compose_root`, reports how many copies of a label the tree holds, and
//! writes the render.
//!
//! `cargo run --example ui_probe -- /tmp/ui.png [phase]`

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use deadly_descent::sim::car::Input;
use deadly_descent::sim::race::Phase;
use deadly_descent::sim::{Controls, subscribe};
use deadly_descent::ui::{self, SharedUi, Sink};
use repame_sim::Sim;
use repose_core::RenderContext;
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
use repose_ui::{Interactions, TextFieldState, layout_and_paint};

const W: u32 = 640;
const H: u32 = 360;

fn main() -> anyhow::Result<()> {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/ui.png".into());
    let phase = match std::env::args().nth(2).as_deref() {
        Some("map") => Phase::MapSelect,
        Some("race") => Phase::Racing,
        _ => Phase::Garage,
    };

    let mut sim = Sim::with_default_step();
    subscribe(&mut sim.world);
    sim.world.insert_resource(Controls(Input::default()));
    sim.world
        .resource_mut::<deadly_descent::sim::Intents>()
        .0
        .push(deadly_descent::sim::Intent::StartLevel(6));
    deadly_descent::sim::apply_intents(&mut sim.world);

    let ctx = RenderContext::new();
    let mut ui = SharedUi {
        viewport: glam::Vec2::new(W as f32, H as f32),
        ..SharedUi::default()
    };
    ui::sync_shared_ui(&mut sim.world, &mut ui, &ctx);
    ui.phase = phase;

    let sink: Sink = Arc::new(Mutex::new(Vec::new()));
    let view = ui::compose_root(&ui, sink);
    let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
    let (scene, hits, sems) =
        layout_and_paint(&view, (W, H), &states, &Interactions::default(), None);
    let count = |label: &str| {
        sems.iter()
            .filter(|n| n.label.as_deref() == Some(label))
            .count()
    };
    eprintln!(
        "hits={} sems={} RACE={} GARAGE={} SELECT LOCATION={}",
        hits.len(),
        sems.len(),
        count("RACE"),
        count("GARAGE"),
        count("SELECT LOCATION")
    );

    let mut gpu = OffscreenRenderer::new_blocking(W, H, 1)?;
    apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
    let rgba = gpu.render_rgba(&scene, Some([0.05, 0.06, 0.08, 1.0]))?;
    image::save_buffer(&path, &rgba, W, H, image::ColorType::Rgba8)?;
    eprintln!("wrote {path}");
    Ok(())
}
