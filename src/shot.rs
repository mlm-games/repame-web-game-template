//! Offscreen frame capture: renders the game's own frame (3D scene plus UI)
//! through the same compose path the window uses and writes PNGs, one per frame.
//!
//! This is the port's video-export/playtest tool: `DD_CAPTURE=<dir>` renders a
//! fixed number of frames headless and exits, so a level can be inspected without
//! a display, a compositor or a screen recorder.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use repose_core::RenderContext;
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
use repose_ui::{Interactions, TextFieldState, layout_and_paint};
use web_time::Duration;

use crate::app::App;

/// Capture settings read from the environment.
pub struct CaptureRequest {
    pub dir: String,
    pub frames: u32,
    pub fps: u32,
    pub level: Option<usize>,
    pub width: u32,
    pub height: u32,
}

impl CaptureRequest {
    pub fn from_env() -> Option<Self> {
        let dir = std::env::var("DD_CAPTURE").ok()?;
        let frames = std::env::var("DD_CAPTURE_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(180);
        let fps = std::env::var("DD_CAPTURE_FPS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let level = std::env::var("DD_AUTOPLAY")
            .ok()
            .and_then(|v| v.parse().ok());
        let width = std::env::var("DD_CAPTURE_W")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(960);
        let height = std::env::var("DD_CAPTURE_H")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(540);
        Some(Self {
            dir,
            frames,
            fps,
            level,
            width,
            height,
        })
    }
}

impl App {
    /// Render `frames` frames headless at `fps`, writing `frame_0000.png` … into
    /// `dir`. The simulation is stepped with a fixed frame time, so a capture is
    /// reproducible.
    pub fn run_capture(&mut self, request: &CaptureRequest) -> anyhow::Result<()> {
        let dir = Path::new(&request.dir);
        std::fs::create_dir_all(dir)?;
        let ctx = RenderContext::new();
        let viewport = [request.width as f32, request.height as f32];
        self.ui.viewport = glam::Vec2::new(viewport[0], viewport[1]);
        let mut gpu = OffscreenRenderer::new_blocking(request.width, request.height, 1)?;
        let states: HashMap<u64, Rc<RefCell<TextFieldState>>> = HashMap::new();
        let dt = Duration::from_secs_f64(1.0 / request.fps.max(1) as f64);
        for frame in 0..request.frames {
            // Same order as the window loop: input, sim, UI sync, compose.
            crate::sim::apply_intents(&mut self.sim.world);
            self.feed_input();
            self.autoplay_drive();
            let _ = self.advance(dt);
            self.sync_audio(dt);
            crate::ui::sync_shared_ui(&mut self.sim.world, &mut self.ui, &ctx);
            let view = self.compose(&ctx, viewport);
            let (scene, _, _) = layout_and_paint(
                &view,
                (request.width, request.height),
                &states,
                &Interactions::default(),
                None,
            );
            apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
            let rgba = gpu.render_rgba(&scene, Some([0.05, 0.06, 0.08, 1.0]))?;
            if frame == 0 {
                eprintln!(
                    "capture buffer len={} expected={} ({w}x{h})",
                    rgba.len(),
                    request.width as usize * request.height as usize * 4,
                    w = request.width,
                    h = request.height
                );
            }
            // The renderer's readback is already top-down; save as-is.
            let path = dir.join(format!("frame_{frame:04}.png"));
            image::save_buffer(
                &path,
                &rgba,
                request.width,
                request.height,
                image::ColorType::Rgba8,
            )?;
            if frame % 30 == 0 {
                eprintln!("capture {frame}/{} -> {}", request.frames, path.display());
            }
        }
        Ok(())
    }
}
