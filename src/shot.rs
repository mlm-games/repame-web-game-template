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
                    150 => Some(crate::sim::Intent::ToGarage),
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
            if std::env::var("DD_AUDIT").is_ok() {
                // A run of the game that names its own anomalies, so a video hunt
                // does not depend on anyone squinting at thumbnails. Every check is
                // one this port has actually broken at some point.
                if let Some(session) = self.sim.world.get_resource::<crate::sim::race::Session>() {
                    let mut notes: Vec<String> = Vec::new();
                    let racing = matches!(
                        session.phase,
                        crate::sim::race::Phase::Countdown | crate::sim::race::Phase::Racing
                    );
                    // 1. Sky where the ground should be: a surface wound the wrong
                    //    way is culled and leaves the clear colour showing.
                    let sky = pixel_share(&rgba, request.width, request.height, |px| {
                        px[2] as i32 > px[0] as i32 + 14 && px[2] as i32 > px[1] as i32 + 8
                    });
                    let lower_sky =
                        region_sky_share(&rgba, request.width, request.height, 0.55, 1.0);
                    // 2. The car must be in frame while racing.
                    let car_visible = car_on_screen(self, request);
                    // 3. The car must be on the road, not under or over it.
                    let car = session.player.clone();
                    let height_ok = self
                        .sim
                        .world
                        .get_resource::<crate::sim::race::TrackWorld>()
                        .map(|world| {
                            let ground =
                                world.track.height(car.pos.z.clamp(0.0, world.track.length));
                            (car.pos.y - ground).abs() < 3.0
                        })
                        .unwrap_or(true);
                    // 4. An obstacle's corridor: its lateral offset must be on the
                    //    road, or the road has a hazard nobody can reach.
                    let stray_obstacles = self
                        .sim
                        .world
                        .get_resource::<crate::sim::race::TrackWorld>()
                        .map(|world| {
                            world
                                .obstacles
                                .iter()
                                .filter(|ob| {
                                    // `lateral` is the offset from the centre line.
                                    ob.lateral.abs() > world.track.half_width + 1.0
                                })
                                .count()
                        })
                        .unwrap_or(0);
                    if racing && !car_visible {
                        notes.push("car not on screen".into());
                    }
                    if racing && !height_ok {
                        let ground = self
                            .sim
                            .world
                            .get_resource::<crate::sim::race::TrackWorld>()
                            .map(|world| {
                                world.track.height(car.pos.z.clamp(0.0, world.track.length))
                            })
                            .unwrap_or(0.0);
                        notes.push(format!(
                            "car {:.1}m above the road (y {:.1}, road {:.1})",
                            car.pos.y - ground,
                            car.pos.y,
                            ground
                        ));
                    }
                    // Only a race is meant to be all surface: the garage and the
                    // map screens have their own sky, and the garage floor is
                    // blue-grey enough to read as sky to a colour test.
                    if racing && lower_sky > 0.35 {
                        notes.push(format!("{:.0}% sky below the horizon", lower_sky * 100.0));
                    }
                    if racing && sky > 0.97 {
                        notes.push("frame is almost entirely sky".into());
                    }
                    if stray_obstacles > 0 {
                        notes.push(format!("{stray_obstacles} obstacles off the road"));
                    }
                    if !notes.is_empty() && frame % 15 == 0 {
                        eprintln!(
                            "audit f{frame} level={} phase={:?} s={:.0}m v={:.0}km/h: {}",
                            session.level,
                            session.phase,
                            car.pos.z,
                            car.speed() * 3.6,
                            notes.join("; ")
                        );
                    }
                }
            }
            if std::env::var("DD_TRACE_CAR").is_ok() && frame % 30 == 0 {
                // Is the player's car actually painted into the frame? Count the
                // pixels that carry paint (warm, red-dominant) anywhere in view.
                let mut paint = 0usize;
                let mut dark = 0usize;
                for px in rgba.as_chunks::<4>().0 {
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

/// Share of pixels matching a predicate.
fn pixel_share(rgba: &[u8], width: u32, height: u32, predicate: impl Fn([u8; 4]) -> bool) -> f64 {
    let mut hits = 0usize;
    let mut total = 0usize;
    for (index, px) in rgba.as_chunks::<4>().0.iter().enumerate() {
        let y = index as u32 / width.max(1);
        if y >= height {
            continue;
        }
        total += 1;
        if predicate([px[0], px[1], px[2], px[3]]) {
            hits += 1;
        }
    }
    hits as f64 / total.max(1) as f64
}

/// Sky share of a vertical band of the frame, as fractions of the height.
fn region_sky_share(rgba: &[u8], width: u32, height: u32, from: f32, to: f32) -> f64 {
    let y0 = (from * height as f32) as usize;
    let y1 = (to * height as f32) as usize;
    let stride = width as usize * 4;
    let mut hits = 0usize;
    let mut total = 0usize;
    for y in y0..y1.min(height as usize) {
        for x in 0..width as usize {
            let i = y * stride + x * 4;
            let (r, g, b) = (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32);
            total += 1;
            if b > r + 14 && b > g + 8 {
                hits += 1;
            }
        }
    }
    hits as f64 / total.max(1) as f64
}

/// Is the player's car anywhere in the frame?
fn car_on_screen(app: &App, request: &CaptureRequest) -> bool {
    let session = match app.sim.world.get_resource::<crate::sim::race::Session>() {
        Some(session) => session,
        None => return true,
    };
    let aspect = request.width as f32 / request.height.max(1) as f32;
    let _ = aspect;
    let Some(camera) = app.sim.world.get_resource::<crate::scene::FrameCamera>() else {
        return true;
    };
    let clip = camera.view_proj * (session.player.pos + glam::Vec3::new(0.0, 1.0, 0.0)).extend(1.0);
    clip.w > 1e-4
}
