//! Dump the 3D scene's texture pages as PNGs, plus one prop rendered alone.
//!
//! `cargo run --example page_probe -- /tmp/pages [prop ...]`
//!
//! The pages are exactly the RGBA the scene uploads to the renderer, so this
//! shows what a mesh samples, independent of the layout or the UI.

use repame_view3d::render::{BatchDesc, SceneFilter};
use repame_view3d::{Frame3d, GeomHandle, MeshGroup, SceneLight, Viewport3d};
use repose_core::RenderContext;
use repose_render_wgpu::apply_render_commands;
use repose_render_wgpu::offscreen::OffscreenRenderer;
use repose_ui::{Interactions, layout_and_paint};

const PROBE_W: u32 = 480;
const PROBE_H: u32 = 320;

fn main() -> anyhow::Result<()> {
    let dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp/pages".into());
    std::fs::create_dir_all(&dir)?;
    let props: Vec<String> = std::env::args().skip(2).collect();
    let props = if props.is_empty() {
        vec!["TerrainPiece".to_string(), "GRID-R-A".to_string()]
    } else {
        props
    };

    let mut textures = deadly_descent::art::Textures::default();
    let mut meshes = deadly_descent::props::PropCache::default();
    let mut groups: Vec<MeshGroup> = Vec::new();
    for file in &props {
        match meshes.get(file, &mut textures) {
            Some(mesh) => {
                eprintln!(
                    "{}: {} groups, ground_offset {:.2}",
                    file,
                    mesh.groups.len(),
                    mesh.ground_offset
                );
                let cloned = mesh.groups.clone();
                groups.extend(cloned);
            }
            None => eprintln!("{file}: missing"),
        }
    }

    // The pages the scene binds, straight from the upload queue.
    let uploads = textures.uploads();
    eprintln!("pages: {}", uploads.len());
    for upload in &uploads {
        let path = format!(
            "{dir}/page_{:02}_{}x{}.png",
            upload.page, upload.w, upload.h
        );
        image::save_buffer(
            &path,
            &upload.rgba,
            upload.w,
            upload.h,
            image::ColorType::Rgba8,
        )?;
    }

    // And one render of the props alone, centred on their bounds.
    let mut frame = Frame3d::default();
    let desc = BatchDesc {
        layer_size: deadly_descent::art::PAGE,
        layers: textures.layer_count().max(1),
        filter: SceneFilter::Linear,
        mips: 1,
    };
    frame.desc = desc;
    frame.viewport_px = [PROBE_W as f32, PROBE_H as f32];
    frame.background = Some([0.35, 0.55, 0.75, 1.0]);
    let (min, max) = groups
        .iter()
        .filter_map(|g| g.bounds())
        .fold(None::<([f32; 3], [f32; 3])>, |acc, (lo, hi)| {
            Some(match acc {
                None => (lo, hi),
                Some((alo, ahi)) => (
                    [alo[0].min(lo[0]), alo[1].min(lo[1]), alo[2].min(lo[2])],
                    [ahi[0].max(hi[0]), ahi[1].max(hi[1]), ahi[2].max(hi[2])],
                ),
            })
        })
        .unwrap_or(([0.0; 3], [1.0; 3]));
    let centre = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
        (min[2] + max[2]) * 0.5,
    ];
    let radius =
        ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2) + (max[2] - min[2]).powi(2))
            .sqrt()
            .max(1.0);
    let eye = [
        centre[0] + radius * 0.7,
        centre[1] + radius * 0.5,
        centre[2] + radius * 0.7,
    ];
    frame.cam = repame_view3d::OrbitCamera {
        target: glam::Vec3::from(centre),
        yaw: std::f32::consts::FRAC_PI_4,
        pitch: 0.5,
        dist: radius * 1.15,
        fov_y_deg: 50.0,
    };
    let _ = eye;
    frame.light = SceneLight {
        direction: [-0.4, -0.8, 0.3],
        color: [1.0, 0.97, 0.9],
        diffuse: 1.0,
        ambient: [0.35, 0.38, 0.42],
        fog: 0.0,
        fog_color: [0.35, 0.55, 0.75],
        fog_start: 100.0,
        fog_end: 400.0,
        exposure: 1.0,
        tonemap: repame_view3d::render::TonemapMethod::Reinhard,
    };
    let mut upload_group = MeshGroup::default();
    let _ = &mut upload_group;
    for g in groups {
        frame.push(g);
    }
    frame.uploads = uploads;

    let ctx = RenderContext::new();
    let geom = GeomHandle::new();
    let view = Viewport3d(frame, geom, "pages", desc, |_| {});
    let (scene, _hits, _sems) = layout_and_paint(
        &view,
        (PROBE_W, PROBE_H),
        &std::collections::HashMap::new(),
        &Interactions::default(),
        None,
    );
    let mut gpu = OffscreenRenderer::new_blocking(PROBE_W, PROBE_H, 1)?;
    apply_render_commands(gpu.renderer_mut(), ctx.drain()).log_failures();
    let rgba = gpu.render_rgba(&scene, Some([0.35, 0.55, 0.75, 1.0]))?;
    image::save_buffer(
        format!("{dir}/props.png"),
        &rgba,
        PROBE_W,
        PROBE_H,
        image::ColorType::Rgba8,
    )?;
    eprintln!("wrote {dir}");
    Ok(())
}
