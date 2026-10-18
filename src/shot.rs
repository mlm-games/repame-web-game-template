//! Offscreen frame capture: renders the game's own frame (3D scene plus UI)
//! through the same host composition the window uses and writes PNGs, one per
//! frame.
//!
//! This is the port's video-export/playtest tool: `DD_CAPTURE=<dir>` renders a
//! fixed number of frames headless and exits, so a level can be inspected without
//! a display, a compositor or a screen recorder.
//!
//! The frames come from `ReposeRuntime::compose_frame_output`, the platform's
//! own one-shot host tick. Laying the view out by hand instead produces a scene
//! where every text run is painted once per retained node, which is what the
//! runtime's scheduler scope and frame cache exist to prevent.

use std::path::Path;

use repose_app::ReposeRuntime;
use repose_core::{RenderContext, Scheduler, View};
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
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
        let mut gpu = OffscreenRenderer::new_blocking(request.width, request.height, 1)?;
        let mut runtime = ReposeRuntime::new();
        runtime.set_viewport_and_scale(request.width, request.height, 1.0);
        let dt = Duration::from_secs_f64(1.0 / request.fps.max(1) as f64);
        // `DD_CAPTURE_FLOW=1` walks the game the way a player does: garage, map,
        // race. Fixed frame indices keep the recording reproducible.
        let flow = std::env::var("DD_CAPTURE_FLOW").is_ok();
        for frame in 0..request.frames {
            if flow {
                let intent = match frame {
                    45 => Some(crate::sim::Intent::ToMapSelect),
                    90 => Some(crate::sim::Intent::StartLevel(request.level.unwrap_or(6))),
                    _ => None,
                };
                if let Some(intent) = intent {
                    self.sim
                        .world
                        .resource_mut::<crate::sim::Intents>()
                        .0
                        .push(intent);
                }
            }
            let mut root =
                |sched: &mut Scheduler, ctx: &RenderContext| -> View { self.view(sched, ctx, dt) };
            let out = runtime.compose_frame_output(&mut root, &ctx);
            apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
            let rgba = gpu.render_rgba(&out.scene, Some([0.05, 0.06, 0.08, 1.0]))?;
            if std::env::var("DD_TRACE_CAR").is_ok() && frame % 30 == 0 {
                // Is the player's car actually painted into the frame? Count the
                // pixels that carry paint (warm, red-dominant) anywhere in view.
                let mut paint = 0usize;
                let mut dark = 0usize;
                for px in rgba.chunks_exact(4) {
                    let (r, g, b) = (px[0] as i32, px[1] as i32, px[2] as i32);
                    if r > g + 25 && g >= b {
                        paint += 1;
                    }
                    if r < 40 && g < 40 && b < 40 {
                        dark += 1;
                    }
                }
                let total = (request.width * request.height) as f64;
                eprintln!(
                    "car trace f{frame}: paint {:.2}% dark {:.2}%",
                    paint as f64 / total * 100.0,
                    dark as f64 / total * 100.0
                );
            }
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
