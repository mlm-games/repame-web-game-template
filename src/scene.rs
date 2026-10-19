//! 3D presentation: the downhill road, its obstacles, and the cars.
//!
//! Building the frame is a pure function of the sim snapshot: the renderer never
//! feeds back into the simulation. Vehicle bodies are boxed stand-ins until the
//! original meshes are wired in (see README, art path).

use bevy_ecs::prelude::*;
use glam::{Vec2, Vec3, vec3};
use repame_view3d::render::TonemapMethod;
use repame_view3d::{
    BatchDesc, Frame3d, GeomHandle, MeshGroup, OrbitCamera, SceneFilter, SceneLight, Viewport3d,
};
use repose_core::prelude::View;

use crate::sim::car::CarSim;
use crate::sim::data;
use crate::sim::obstacles::Kind;
use crate::sim::race::{Phase, Screen, Session, TrackWorld};

/// Batch shape: one page per car texture, so the desc depends on how many
/// pages the art cache has bound (fixed after the first frame).
pub fn scene_desc(layers: u32) -> BatchDesc {
    BatchDesc {
        layer_size: crate::art::PAGE,
        layers: layers.max(1),
        filter: SceneFilter::Linear,
        mips: 1,
    }
}

// Shipped material tones: `road2.001` (asphalt) 0.349, `Road.001` (concrete
// variant) 0.434, `Terrain`/`RoadGlide` 0.321, `Dunes` sand 0.434.
const ASPHALT: [f32; 3] = [0.349, 0.345, 0.34];
const DIRT: [f32; 3] = [0.30, 0.26, 0.22];
const GRASS: [f32; 3] = [0.27, 0.32, 0.20];
const MOUNTAIN: [f32; 3] = [0.40, 0.39, 0.375];
const SAND: [f32; 3] = [0.43, 0.40, 0.33];
const CONTAINER_A: [f32; 3] = [0.72, 0.26, 0.20];
const CRATE: [f32; 3] = [0.55, 0.40, 0.22];
const SPIKE: [f32; 3] = [0.55, 0.55, 0.60];
const BALL_C: [f32; 3] = [0.22, 0.22, 0.26];
const PRESS_C: [f32; 3] = [0.65, 0.55, 0.15];
const GATE_C: [f32; 3] = [0.80, 0.65, 0.25];
const TRAIN_C: [f32; 3] = [0.30, 0.30, 0.45];
const BUS_C: [f32; 3] = [0.85, 0.72, 0.20];
const WALL_C: [f32; 3] = [0.62, 0.60, 0.56];
const GLASS: [f32; 3] = [0.18, 0.26, 0.34];

fn push_box(m: &mut MeshGroup, centre: Vec3, half: Vec3, colour: [f32; 3]) {
    let c = centre;
    let h = half;
    let v = [
        c + vec3(-h.x, -h.y, -h.z),
        c + vec3(h.x, -h.y, -h.z),
        c + vec3(h.x, h.y, -h.z),
        c + vec3(-h.x, h.y, -h.z),
        c + vec3(-h.x, -h.y, h.z),
        c + vec3(h.x, -h.y, h.z),
        c + vec3(h.x, h.y, h.z),
        c + vec3(-h.x, h.y, h.z),
    ];
    let faces: [[usize; 4]; 6] = [
        [0, 1, 2, 3],
        [5, 4, 7, 6],
        [4, 0, 3, 7],
        [1, 5, 6, 2],
        [3, 2, 6, 7],
        [4, 5, 1, 0],
    ];
    for f in faces {
        let base = m.positions.len() as u32;
        for i in f {
            m.positions.push(v[i].to_array());
            m.colors.push(colour);
        }
        m.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Two mirror-image right-triangle prisms: a spike strip and a ramp.
fn push_wedge(m: &mut MeshGroup, centre: Vec3, half: Vec3, colour: [f32; 3], ramp: bool) {
    let c = centre;
    let h = half;
    let (z0, z1) = (c.z - h.z, c.z + h.z);
    let y0 = c.y - h.y;
    let y1 = if ramp { c.y + h.y } else { c.y + h.y * 0.2 };
    let a = vec3(c.x - h.x, y0, z0);
    let b = vec3(c.x + h.x, y0, z0);
    let d = vec3(c.x - h.x, y0, z1);
    let e = vec3(c.x + h.x, y0, z1);
    let t0 = vec3(c.x - h.x, y1, z1);
    let t1 = vec3(c.x + h.x, y1, z1);
    let tri = |m: &mut MeshGroup, p: [Vec3; 3]| {
        let base = m.positions.len() as u32;
        for v in p {
            m.positions.push(v.to_array());
            m.colors.push(colour);
        }
        m.indices.extend_from_slice(&[base, base + 1, base + 2]);
    };
    tri(m, [a, b, t1]);
    tri(m, [a, t1, t0]);
    tri(m, [d, t0, t1]);
    tri(m, [d, t1, e]);
    tri(m, [a, d, e]);
    tri(m, [a, e, b]);
    let _ = y1;
}

fn push_cylinder(
    m: &mut MeshGroup,
    centre: Vec3,
    radius: f32,
    half_len: f32,
    axis_x: bool,
    colour: [f32; 3],
    segments: usize,
) {
    for i in 0..segments {
        let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p = |s: f32, c: f32, off: f32| {
            if axis_x {
                centre + vec3(off, c * radius, s * radius)
            } else {
                centre + vec3(c * radius, s * radius, off)
            }
        };
        let quad = [
            p(s0, c0, -half_len),
            p(s1, c1, -half_len),
            p(s1, c1, half_len),
            p(s0, c0, half_len),
        ];
        let base = m.positions.len() as u32;
        for v in quad {
            m.positions.push(v.to_array());
            m.colors.push(colour);
        }
        m.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Which camera preset the app is showing (`C` switches, as the shipped game's
/// "change camera view" control does).
#[derive(Resource, Clone, Copy, Debug)]
pub struct CameraChoice {
    pub mode: CameraMode,
}

impl Default for CameraChoice {
    fn default() -> Self {
        Self {
            mode: CameraMode::Chase,
        }
    }
}

#[derive(Resource, Default)]
pub struct SceneCache {
    pub key: u64,
    pub static_groups: Vec<MeshGroup>,
    pub art: crate::art::ArtCache,
    pub props: crate::props::PropCache,
}

fn track_key(level: &data::Level) -> u64 {
    // Everything `build_static` varies on: the seed, the size, and the segment
    // (which picks the dressing palette and whether a start deck is placed).
    let segment = crate::sim::seed::hash(level.segment().bytes().fold(2166136261u32, |acc, b| {
        (acc ^ b as u32).wrapping_mul(16777619)
    })) as u64;
    (level.racing_track_seed.max(0) as u64) << 32
        | ((level.racing_track_size.max(0) as u64) << 20)
        | (segment & 0xFFFFF)
}

/// Emit a quad as two triangles wound so the surface faces *up*: the renderer
/// culls back faces, and a ground surface wound the other way is invisible from
/// every camera above it.
fn push_quad(g: &mut MeshGroup, verts: [Vec3; 4], colour: [f32; 3], uv: [[f32; 2]; 4]) {
    let base = g.positions.len() as u32;
    for (v, uv) in verts.into_iter().zip(uv) {
        g.positions.push(v.to_array());
        g.colors.push(colour);
        g.uvs.push(uv);
    }
    g.indices
        .extend_from_slice(&[base + 2, base + 1, base, base + 3, base + 2, base]);
}

/// Run-off ribbon past both ends of the track, in metres. The chase camera starts
/// behind the line, so without it the bottom of the frame looks into empty space.
const RUN_OFF: f32 = 60.0;

/// Bind the surfacing textures the shipped materials name, when the reference
/// export is present. Without it the groups keep flat colours, which is the
/// fallback every art path in this port follows.
fn ground_pages(textures: &mut crate::art::Textures) -> [Option<u32>; 4] {
    let page = |textures: &mut crate::art::Textures, name: &str| {
        textures.page_sized(name, crate::art::PAGE)
    };
    let road = page(textures, "CurveRoad");
    let shoulder = page(textures, "CementNM");
    let verge = page(textures, "UnityGrass");
    // The apron is the ground out to the horizon: a flat tone reads better than
    // a 300 m wide stretch of a 256 px texture, which bands as it repeats.
    let apron = None;
    [road, shoulder, verge, apron]
}

/// Give a group the page its surface wants, or leave it untextured.
fn bind_page(
    mut group: MeshGroup,
    page: Option<u32>,
    textures: &crate::art::Textures,
) -> MeshGroup {
    match page {
        Some(page) => {
            group.texture_page = page;
            textures.fit_page(&mut group);
        }
        None => group.uvs.clear(),
    }
    group
}

/// Scene geometry is depth-tested: `MeshGroup::default()` is the editor's
/// overlay default (always draw, no test), which paints procedural ground over
/// the imported cars and props submitted before it.
pub fn mesh() -> MeshGroup {
    MeshGroup {
        depth_test: true,
        ..MeshGroup::default()
    }
}

fn build_static(
    track_world: &TrackWorld,
    level: &data::Level,
    textures: &mut crate::art::Textures,
) -> Vec<MeshGroup> {
    let racing = level.is_racing_track();
    let road_tone = if racing { ASPHALT } else { DIRT };
    let verge_tone = if racing { GRASS } else { SAND };
    let track = &track_world.track;
    let mut road = mesh();
    let mut shoulder = mesh();
    let steps = 150usize;
    let span = track.length + RUN_OFF * 2.0;
    let step = span / steps as f32;
    let half = track.half_width;
    // The ribbon runs `RUN_OFF` metres past both ends. The camera starts behind
    // the line, and the width and height there come from the clamped ends, so the
    // run-off is the same surface continued rather than a new one.
    let along = |i: usize| i as f32 * step - RUN_OFF;
    let point = |s: f32| track.centre(s.clamp(0.0, track.length));
    for i in 0..steps {
        let (s0, s1) = (along(i), along(i + 1));
        let (c0, c1) = (point(s0), point(s1));
        // road surface
        let quad = [
            vec3(c0.x - half, c0.y, s0),
            vec3(c0.x + half, c0.y, s0),
            vec3(c1.x + half, c1.y, s1),
            vec3(c1.x - half, c1.y, s1),
        ];
        // The shipped `CurveRoad` sheet is a road piece, not a tiling surface:
        // asphalt fills the top left of the page, with a kerb band to the right
        // and a skirt below. Sampling the whole page per quad painted those
        // bands across the road every `step` metres, so the quads take the
        // asphalt rect only.
        const ROAD_U: [f32; 2] = [0.005, 0.75];
        const ROAD_V: [f32; 2] = [0.005, 0.61];
        // A hole is a real gap in the road here, so the quads it covers are not
        // built at all; the dark pit mesh under it reads as the drop.
        let gap = track_world.track.features.iter().any(|f| {
            matches!(f.kind, crate::sim::track::SurfaceKind::Hole)
                && s1 >= f.s - f.half_length
                && s0 <= f.s + f.half_length
        });
        if !gap {
            push_quad(
                &mut road,
                quad,
                road_tone,
                [
                    [ROAD_U[0], ROAD_V[0]],
                    [ROAD_U[1], ROAD_V[0]],
                    [ROAD_U[1], ROAD_V[1]],
                    [ROAD_U[0], ROAD_V[1]],
                ],
            );
        }
        // downhill skirt either side, so the road reads as cut into a slope
        for side in [-1.0f32, 1.0] {
            let a = vec3(c0.x + side * half, c0.y, s0);
            let b = vec3(c1.x + side * half, c1.y, s1);
            let c = vec3(c1.x + side * half, c1.y - 9.0, s1);
            let d = vec3(c0.x + side * half, c0.y - 9.0, s0);
            // Winding follows the corner order, and the lateral extent changes
            // sign with `side`: without this the left-hand ground faces down and
            // is culled, leaving sky under the road.
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            if side > 0.0 {
                push_quad(&mut shoulder, [a, d, c, b], MOUNTAIN, uv);
            } else {
                push_quad(&mut shoulder, [b, c, d, a], MOUNTAIN, uv);
            }
        }
    }
    // grass verges
    let mut verge = mesh();
    for i in 0..steps {
        let (s0, s1) = (along(i), along(i + 1));
        let (c0, c1) = (point(s0), point(s1));
        for side in [-1.0f32, 1.0] {
            let a = vec3(c0.x + side * half, c0.y - 0.05, s0);
            let b = vec3(c1.x + side * half, c1.y - 0.05, s1);
            let c = vec3(
                c1.x + side * (half + 26.0),
                c1.y - 5.0 - i as f32 * 0.05,
                s1,
            );
            let d = vec3(
                c0.x + side * (half + 26.0),
                c0.y - 5.0 - i as f32 * 0.05,
                s0,
            );
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            if side > 0.0 {
                push_quad(&mut verge, [a, d, c, b], verge_tone, uv);
            } else {
                push_quad(&mut verge, [b, c, d, a], verge_tone, uv);
            }
        }
    }
    // The apron carries the ground out to the horizon on both sides. Without it
    // the road reads as a ribbon floating in the sky, because the shoulder skirt
    // only reaches 26 m out.
    let mut apron = mesh();
    for i in 0..steps {
        let (s0, s1) = (along(i), along(i + 1));
        let (c0, c1) = (point(s0), point(s1));
        let drop0 = 5.0 + i as f32 * 0.05;
        for side in [-1.0f32, 1.0] {
            let inner = half + 26.0;
            let outer = half + 320.0;
            let a = vec3(c0.x + side * inner, c0.y - drop0, s0);
            let b = vec3(c1.x + side * inner, c1.y - drop0, s1);
            let c = vec3(c1.x + side * outer, c1.y - drop0 - 34.0, s1);
            let d = vec3(c0.x + side * outer, c0.y - drop0 - 34.0, s0);
            let uv = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
            if side > 0.0 {
                push_quad(&mut apron, [a, d, c, b], SAND, uv);
            } else {
                push_quad(&mut apron, [b, c, d, a], SAND, uv);
            }
        }
    }
    let [road_page, shoulder_page, verge_page, apron_page] = ground_pages(textures);
    if std::env::var("DD_TRACE_SCENE").is_ok() {
        eprintln!(
            "ground pages: road={road_page:?} shoulder={shoulder_page:?} verge={verge_page:?} \
             apron={apron_page:?} (layers {})",
            textures.layer_count()
        );
    }
    vec![
        bind_page(road, road_page, textures),
        bind_page(shoulder, shoulder_page, textures),
        bind_page(verge, verge_page, textures),
        bind_page(apron, apron_page, textures),
    ]
}

/// Procedural stand-in for hazards the export has no mesh for.
fn push_standin(opaque: &mut MeshGroup, ob: &crate::sim::obstacles::Obstacle, at: Vec3) {
    let half = ob.kind.half_extents();
    match ob.kind {
        Kind::Ball => {
            push_cylinder(
                opaque,
                at + vec3(0.0, half.y, 0.0),
                half.x,
                0.55,
                true,
                BALL_C,
                12,
            );
            push_wedge(
                opaque,
                at + vec3(0.0, half.y, 0.0),
                Vec3::new(half.x * 1.2, 0.35, 0.0),
                SPIKE,
                false,
            );
        }
        Kind::Press => push_box(opaque, at + vec3(0.0, 3.2 + half.y, 0.0), half, PRESS_C),
        Kind::Gate => push_box(opaque, at + vec3(0.0, half.y, 0.0), half, GATE_C),
        Kind::Hole => push_box(
            opaque,
            at + vec3(0.0, -1.2, 0.0),
            Vec3::new(half.x, 0.6, half.z),
            [0.03, 0.03, 0.04],
        ),
        Kind::Container => push_box(opaque, at + vec3(0.0, half.y, 0.0), half, CONTAINER_A),
        Kind::Crate => push_box(opaque, at + vec3(0.0, half.y, 0.0), half, CRATE),
        Kind::Spikes => push_wedge(opaque, at, half, SPIKE, false),
        Kind::Ramp => push_wedge(opaque, at + vec3(0.0, half.y, 0.0), half, WALL_C, true),
        Kind::Bus => push_box(opaque, at + vec3(0.0, half.y + 0.4, 0.0), half, BUS_C),
        Kind::Wall => push_box(opaque, at + vec3(0.0, half.y, 0.0), half, WALL_C),
        Kind::Train => push_box(opaque, at + vec3(0.0, half.y + 0.6, 0.0), half, TRAIN_C),
    }
}

fn append_group(dst: &mut MeshGroup, src: &MeshGroup) {
    let base = dst.positions.len() as u32;
    dst.positions.extend_from_slice(&src.positions);
    dst.colors.extend_from_slice(&src.colors);
    // Normals have to stay all-or-nothing: the renderer drops a group whose
    // normals do not match its vertices. `with_normals` rebuilds them for the
    // whole group afterwards when either side cannot supply them.
    if dst.normals.len() == dst.positions.len() - src.positions.len()
        && src.normals.len() == src.positions.len()
    {
        dst.normals.extend_from_slice(&src.normals);
    } else {
        dst.normals.clear();
    }
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

/// Give a group normals that match its vertices exactly.
///
/// The shipped meshes arrive with normals of their own; the procedural road,
/// verges and hazard stand-ins do not, and appending one kind to the other used
/// to leave a group whose normal count was short of its vertex count, which the
/// renderer drops without drawing anything. Normals are accumulated per vertex
/// from the triangles, so a rebuilt group is shaded rather than flat.
fn with_normals(mut group: MeshGroup) -> MeshGroup {
    if std::env::var("DD_DEBUG_FLIP_WINDING").is_ok() {
        // Isolation aid: the same geometry with every triangle's winding reversed.
        for tri in group.indices.as_chunks_mut::<3>().0 {
            tri.swap(0, 2);
        }
    }
    if group.normals.len() == group.positions.len() {
        return group;
    }
    let mut acc = vec![vec3(0.0, 0.0, 0.0); group.positions.len()];
    for tri in group.indices.as_chunks::<3>().0 {
        let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
        if a >= acc.len() || b >= acc.len() || c >= acc.len() {
            continue;
        }
        let face = (Vec3::from(group.positions[b]) - Vec3::from(group.positions[a]))
            .cross(Vec3::from(group.positions[c]) - Vec3::from(group.positions[a]));
        acc[a] += face;
        acc[b] += face;
        acc[c] += face;
    }
    group.normals = acc
        .into_iter()
        .map(|n| {
            if n.length_squared() > 1e-12 {
                n.normalize().to_array()
            } else {
                [0.0, 1.0, 0.0]
            }
        })
        .collect();
    group
}

fn obstacle_groups_with_props(
    cache: &mut crate::props::PropCache,
    textures: &mut crate::art::Textures,
    track_world: &TrackWorld,
    horizon: Option<(f32, f32)>,
) -> Vec<MeshGroup> {
    let table = crate::props::prop_table();
    let track = &track_world.track;
    let mut opaque = mesh();
    for ob in &track_world.obstacles {
        if let Some((from, to)) = horizon
            && (ob.s < from || ob.s > to)
        {
            continue;
        }
        if ob.broken && !ob.kind.is_breakable() {
            continue;
        }
        let centre = track.centre(ob.s) + ob.offset;
        let at = vec3(centre.x + ob.lateral, centre.y, centre.z);
        let yaw = crate::props::tangent_yaw(track, ob.s);
        let wrecked = ob.broken && ob.kind.is_breakable();
        if let Some(def) = table.obstacle_for(ob.kind)
            && let Some(groups) = crate::props::obstacle_groups(cache, textures, def, at, yaw, {
                let half = ob.kind.half_extents();
                Vec3::new(half.x * 2.0, half.y * 2.0, half.z * 2.0)
            })
        {
            for mut g in groups {
                if wrecked {
                    // Flatten the wreck onto the road where it broke.
                    for p in g.positions.iter_mut() {
                        p[1] = centre.y + 0.08;
                    }
                    for c in g.colors.iter_mut() {
                        *c = [c[0] * 0.7, c[1] * 0.7, c[2] * 0.7];
                    }
                }
                append_group(&mut opaque, &g);
            }
            continue;
        }
        if !wrecked {
            if ob.kind == Kind::Hole {
                // The gap spans the road, so the pit behind it does too: a
                // narrower box left pale ground showing at its edges.
                push_box(
                    &mut opaque,
                    at + vec3(0.0, -3.0, 0.0),
                    Vec3::new(track.half_width, 3.0, ob.kind.half_extents().z),
                    [0.03, 0.03, 0.04],
                );
            } else {
                push_standin(&mut opaque, ob, at);
            }
        }
    }
    vec![opaque]
}

/// A low-poly stand-in body matching the car's real proportions.
fn car_groups(sim: &CarSim, livery: [f32; 3]) -> Vec<MeshGroup> {
    let spec = sim.spec;
    let len = spec.wheelbase() + spec.wheel_radius() * 2.6;
    let width = spec.track_width() + 0.25;
    let wheel_r = spec.wheel_radius();
    let mut body = mesh();
    let mut glass = mesh();
    let (s, c) = sim.yaw.sin_cos();
    let rot = |v: Vec3| vec3(v.x * c - v.z * s, v.y, v.x * s + v.z * c);
    let base = sim.pos + vec3(0.0, wheel_r + 0.55, 0.0);
    let pitch = sim.pitch;
    let (sp, cp) = pitch.sin_cos();
    let tilt = |v: Vec3| vec3(v.x, v.y * cp - v.z * sp, v.y * sp + v.z * cp);
    // body
    let body_half = vec3(width * 0.5, 0.42, len * 0.5);
    let centre = base + rot(tilt(vec3(0.0, 0.15, 0.0)));
    let mut cube = mesh();
    push_box(&mut cube, Vec3::ZERO, body_half, livery);
    for v in cube.positions.iter_mut() {
        let local = Vec3::from(*v);
        *v = (centre + rot(tilt(local))).to_array();
    }
    body.positions.extend(cube.positions);
    body.colors.extend(cube.colors);
    body.indices.extend(cube.indices);
    // cabin
    let cab_half = vec3(width * 0.42, 0.34, len * 0.22);
    let cab_centre = base + rot(tilt(vec3(0.0, 0.72, -len * 0.06)));
    let mut cube = mesh();
    push_box(&mut cube, Vec3::ZERO, cab_half, GLASS);
    for v in cube.positions.iter_mut() {
        let local = Vec3::from(*v);
        *v = (cab_centre + rot(tilt(local))).to_array();
    }
    glass.positions.extend(cube.positions);
    glass.colors.extend(cube.colors);
    glass.indices.extend(cube.indices);
    // wheels
    let mut wheels = mesh();
    for (i, w) in spec.wheels.iter().enumerate().take(4) {
        let steer = if w.side == "front" { sim.yaw } else { 0.0 };
        let local = vec3(w.pos[0], w.pos[1], w.pos[2]);
        let world = sim.pos + rot(local) + vec3(0.0, 0.0, 0.0) * 0.0;
        let _ = steer;
        push_cylinder(
            &mut wheels,
            world,
            wheel_r,
            spec.track_width() * 0.06,
            true,
            [0.08, 0.08, 0.09],
            10,
        );
        let _ = i;
    }
    let _ = len;
    vec![body, glass, wheels]
}

pub fn car_colour(index: usize) -> [f32; 3] {
    const LIVERY: [[f32; 3]; 10] = [
        [0.85, 0.20, 0.16],
        [0.16, 0.42, 0.85],
        [0.90, 0.78, 0.18],
        [0.20, 0.70, 0.35],
        [0.62, 0.24, 0.78],
        [0.92, 0.52, 0.12],
        [0.90, 0.90, 0.92],
        [0.14, 0.62, 0.66],
        [0.36, 0.30, 0.26],
        [0.70, 0.14, 0.40],
    ];
    LIVERY[index % LIVERY.len()]
}

/// One car's art, placed for this frame. The menu shows a car with the race's
/// own loader, so an upgraded car looks the same in the garage as on the road.
fn push_car(
    art: &mut crate::art::ArtCache,
    groups: &mut Vec<MeshGroup>,
    sim: &CarSim,
    livery: [f32; 3],
) {
    let wheels = sim.spec.wheels.clone();
    let car = art.get(sim.spec.id.as_str(), &wheels).cloned();
    match car {
        Some(art) if !art.is_empty() => {
            for part in &art.parts {
                if !part.kind.visible_in_chase() || part.kind == crate::art::PartKind::Hidden {
                    continue;
                }
                if std::env::var("DD_TRACE_DRAW").is_ok() {
                    let placed = crate::art::part_groups(part, sim, livery);
                    let bounds = placed.iter().filter_map(|g| g.bounds()).fold(
                        None::<([f32; 3], [f32; 3])>,
                        |acc, (lo, hi)| {
                            Some(match acc {
                                None => (lo, hi),
                                Some((alo, ahi)) => (
                                    [alo[0].min(lo[0]), alo[1].min(lo[1]), alo[2].min(lo[2])],
                                    [ahi[0].max(hi[0]), ahi[1].max(hi[1]), ahi[2].max(hi[2])],
                                ),
                            })
                        },
                    );
                    let materials: Vec<(usize, usize, usize, usize, usize, u32, usize)> = placed
                        .iter()
                        .map(|g| {
                            (
                                g.positions.len(),
                                g.normals.len(),
                                g.uvs.len(),
                                g.colors.len(),
                                g.indices.len(),
                                g.texture_page,
                                g.tri_count(),
                            )
                        })
                        .collect();
                    eprintln!(
                        "menu car={} part={} kind={:?} corner={:?} groups={} materials={materials:?} bounds={:?}",
                        sim.spec.id,
                        part.source,
                        part.kind,
                        part.corner,
                        placed.len(),
                        bounds
                    );
                    groups.extend(placed);
                    continue;
                }
                groups.extend(crate::art::part_groups(part, sim, livery));
            }
            groups.extend(crate::art::missing_wheel_groups(sim, &art));
        }
        _ => groups.extend(car_groups(sim, livery)),
    }
}

/// The light the game's scenes are lit with.
fn scene_light() -> SceneLight {
    SceneLight {
        direction: [-0.45, -0.82, 0.35],
        color: [1.0, 0.97, 0.9],
        diffuse: 1.05,
        ambient: [0.32, 0.34, 0.38],
        fog: 0.18,
        fog_color: [0.62, 0.72, 0.86],
        fog_start: 90.0,
        fog_end: 340.0,
        exposure: 1.0,
        tonemap: TonemapMethod::Reinhard,
    }
}

/// The Garage scene: the shipped room with the selected car in it, on the menu
/// camera. The reference keeps the room in the Garage scene itself and stacks all
/// ten cars at the origin with only the selected one enabled, which is what this
/// rebuilds — a garage screen with no car in it was the largest menu gap.
fn menu_frame(world: &mut World, viewport_px: [f32; 2], geom: &GeomHandle) -> View {
    let phase = world
        .get_resource::<Screen>()
        .map(|screen| screen.0)
        .unwrap_or(Phase::Garage);
    if !matches!(phase, Phase::Garage | Phase::MapSelect) {
        return repose_ui::Spacer();
    }
    let (car_index, upgrades) = {
        let progress = world.get_resource::<crate::sim::meta::Progress>();
        let index = progress.map(|p| p.selected_car()).unwrap_or(0);
        let up = progress
            .map(|p| p.upgrades_for(data::car_at(index as i32).id.as_str()))
            .unwrap_or_default();
        (index, up)
    };
    let only_car = std::env::var("DD_DEBUG_ONLY_CAR").is_ok();
    let hide_car = std::env::var("DD_HIDE_CAR_PARTS").is_ok();
    let mut groups: Vec<MeshGroup> = Vec::new();
    world.resource_scope(|_world, mut cache: Mut<SceneCache>| {
        let SceneCache { props, art, .. } = &mut *cache;
        for piece in crate::props::garage_pieces().iter().filter(|_| !only_car) {
            if let Some(placed) = crate::props::placed_groups(
                props,
                &mut art.textures,
                &piece.file,
                Vec3::from(piece.position),
                piece.rotation,
                piece.scale,
            ) {
                groups.extend(placed);
            }
        }
        if !hide_car {
            let sim = CarSim::new(data::car_at(car_index as i32), Vec3::ZERO, 0.0, upgrades);
            push_car(art, &mut groups, &sim, car_colour(car_index));
        }
    });
    let (desc, uploads) = {
        let mut cache = world.resource_mut::<SceneCache>();
        cache.art.textures.preload_cars(data::cars());
        let SceneCache { props, art, .. } = &mut *cache;
        props.preload(&mut art.textures);
        let desc = scene_desc(cache.art.textures.layer_count());
        let uploads = cache.art.textures.uploads();
        (desc, uploads)
    };
    let (mut target, mut yaw, mut dist, mut pitch) = (vec3(0.0, 0.85, 0.0), 2.4, 7.0, 0.16);
    if let Ok(shape) = std::env::var("DD_DEBUG_CAM") {
        // Isolation aid, the same one the race path offers.
        match shape.as_str() {
            "bird" => {
                target = vec3(0.0, 0.0, 0.0);
                yaw = 0.0;
                dist = 45.0;
                pitch = 0.9;
            }
            "side" => {
                target = vec3(0.0, 1.0, 0.0);
                yaw = std::f32::consts::FRAC_PI_2;
                dist = 25.0;
                pitch = 0.25;
            }
            other => {
                // A bare angle aims the menu camera there: the garage's props
                // are fixed, so finding an angle clear of them is a sweep.
                if let Ok(angle) = other.parse::<f32>() {
                    yaw = angle;
                }
            }
        }
    }
    // The shipped garage is a dim interior: sky behind its walls read as a
    // building floating in the air, so the menu clears to the room's own dark
    // tone and the light keeps the industrial mood.
    let mut light = scene_light();
    light.ambient = [0.42, 0.44, 0.50];
    light.diffuse = 0.95;
    let mut frame = Frame3d {
        background: Some([0.16, 0.155, 0.15, 1.0]),
        viewport_px,
        desc,
        uploads,
        cam: OrbitCamera {
            target,
            yaw,
            pitch,
            dist,
            fov_y_deg: 38.0,
        },
        light,
        ..Default::default()
    };
    world.insert_resource(FrameCamera {
        view_proj: frame
            .cam
            .view_proj(viewport_px[0].max(1.0) / viewport_px[1].max(1.0)),
    });
    if let Ok(keep) = std::env::var("DD_DEBUG_KEEP") {
        groups.retain(|g| match keep.as_str() {
            "untextured" => g.uvs.is_empty(),
            "textured" => !g.uvs.is_empty(),
            _ => true,
        });
    }
    if let Ok(n) = std::env::var("DD_DEBUG_FIRST")
        && let Ok(n) = n.parse::<usize>()
    {
        groups.truncate(n);
    }
    if std::env::var("DD_DEBUG_CUBE").is_ok() {
        let mut m = mesh();
        push_box(
            &mut m,
            frame.cam.target,
            vec3(0.6, 0.6, 0.6),
            [0.9, 0.15, 0.15],
        );
        groups.clear();
        groups.push(m);
    }
    for g in groups {
        let g = with_normals(g);
        if !g.is_empty() {
            frame.push(g);
        }
    }
    Viewport3d(frame, geom.clone(), "deadly-descent", desc, |_ev| {})
}

pub fn frame_for(
    world: &mut World,
    _ctx: &repose_core::RenderContext,
    viewport_px: [f32; 2],
    geom: &GeomHandle,
) -> View {
    let phase = world
        .get_resource::<Screen>()
        .map(|screen| screen.0)
        .unwrap_or(Phase::Garage);
    let Some(session) = world.get_resource::<Session>() else {
        return menu_frame(world, viewport_px, geom);
    };
    // The garage is its own scene in the shipped build, and a finished race
    // leaves its session behind: without this the menu would show the track.
    if matches!(phase, Phase::Garage | Phase::MapSelect) {
        return menu_frame(world, viewport_px, geom);
    }
    let phase = session.phase;
    let level = session.level();
    let car_index = session.car;
    let player = session.player.clone();
    let ai_snapshot: Vec<(usize, crate::sim::car::CarSim)> = session
        .ai
        .iter()
        .map(|ai| {
            (
                data::cars()
                    .iter()
                    .position(|c| c.id == ai.sim.spec.id)
                    .unwrap_or(0),
                ai.sim.clone(),
            )
        })
        .collect();
    let mode = world
        .get_resource::<CameraChoice>()
        .map(|c| c.mode)
        .unwrap_or(CameraMode::Chase);
    let cockpit = mode == CameraMode::Cockpit;
    // The camera aims at the road itself, not just at the car: on a descending
    // track a level camera loses the road out of the bottom of the frame.
    let mut road_ahead = player.pos;
    let mut groups: Vec<MeshGroup> = Vec::new();
    let mut geometry_ready = false;
    // `resource_scope` gives the cache a mutable borrow while the track stays in
    // the world: nothing can observe a world without a track mid-frame.
    world.resource_scope(|world, track_world: Mut<TrackWorld>| {
        road_ahead = track_world.track.road_pos(
            (player.pos.z + CAMERA_LOOK_AHEAD).min(track_world.track.length),
            0.0,
        );
        let key = track_key(&level);
        {
            let mut cache = world.resource_mut::<SceneCache>();
            if cache.key != key || cache.static_groups.is_empty() {
                cache.key = key;
                let mut built = {
                    let SceneCache { art, .. } = &mut *cache;
                    build_static(&track_world, &level, &mut art.textures)
                };
                let SceneCache { props, art, .. } = &mut *cache;
                let dressing = props.dressing(&track_world.track, &mut art.textures);
                built.extend(dressing);
                if level.is_racing_track() && std::env::var("DD_HIDE_DECK").is_err() {
                    // The shipped modular deck marks the start grid and finish.
                    built.extend(crate::props::start_deck(
                        props,
                        &mut art.textures,
                        &track_world.track,
                    ));
                }
                cache.static_groups = built;
                // apply the level's own background tint to the road so modes differ
                let tint = if level.scene.contains("RacingTrack") {
                    0.06
                } else {
                    0.0
                };
                for g in cache.static_groups.iter_mut() {
                    for c in g.colors.iter_mut() {
                        c[0] = (c[0] + tint).min(1.0);
                    }
                }
            }
            groups.extend(cache.static_groups.iter().cloned());
        }
        if !matches!(phase, Phase::Garage | Phase::MapSelect) {
            let horizon = (player.pos.z - 120.0, player.pos.z + 420.0);
            let mut cache = world.resource_mut::<SceneCache>();
            let SceneCache { props, art, .. } = &mut *cache;
            let drawn =
                obstacle_groups_with_props(props, &mut art.textures, &track_world, Some(horizon));
            groups.extend(drawn);
        }
        geometry_ready = true;
    });
    // Bisect the world without losing the car: the car is pushed after this, so
    // truncating the static list is what finds a group that hides it.
    if let Ok(n) = std::env::var("DD_DEBUG_STATIC_MAX")
        && let Ok(n) = n.parse::<usize>()
    {
        groups.truncate(n);
    }
    if std::env::var("DD_DEBUG_ONLY_CAR").is_ok() {
        // Isolation aid: the car alone, on the background.
        groups.clear();
    }
    if geometry_ready {
        // cars: original meshes when the reference export is present, stand-ins
        // otherwise (the stand-ins always cover whatever art is missing).
        let mut cars: Vec<(CarSim, [f32; 3])> = Vec::new();
        if !matches!(phase, Phase::Garage | Phase::MapSelect) {
            cars.push((player.clone(), car_colour(car_index)));
            for (spec_index, sim) in &ai_snapshot {
                cars.push((sim.clone(), car_colour(*spec_index)));
            }
        } else {
            let car = data::car_at(car_index as i32);
            let upgrades = crate::sim::meta::Progress::default().upgrades_for(car.id.as_str());
            let mut show =
                crate::sim::car::CarSim::new(car, Vec3::new(0.0, 0.5, 0.0), 0.6, upgrades);
            show.yaw = 0.6 + viewport_px[0] * 0.0001;
            cars.push((show, car_colour(car_index)));
        }
        if std::env::var("DD_DEBUG_CAR_BOX").is_ok() {
            // Isolation aid: a hand-built box where the car is, so an art problem
            // can be told from a position or depth problem.
            let mut m = mesh();
            push_box(
                &mut m,
                player.pos + vec3(0.0, 1.0, 0.0),
                vec3(1.2, 1.0, 2.6),
                [0.85, 0.2, 0.16],
            );
            groups.push(m);
        }
        let mut art_used = 0usize;
        for (sim, colour) in cars {
            let car_id = sim.spec.id.clone();
            let wheels = sim.spec.wheels.clone();
            let mut cache = world.resource_mut::<SceneCache>();
            let art = cache.art.get(car_id.as_str(), &wheels).cloned();
            match art {
                Some(art) if !art.is_empty() && std::env::var("DD_HIDE_CAR_PARTS").is_err() => {
                    for part in &art.parts {
                        if !cockpit && !part.kind.visible_in_chase() {
                            continue;
                        }
                        if part.kind == crate::art::PartKind::Hidden {
                            continue;
                        }
                        if std::env::var("DD_TRACE_DRAW").is_ok() {
                            let bounds = part.groups.iter().filter_map(|g| g.bounds()).fold(
                                None::<([f32; 3], [f32; 3])>,
                                |acc, (min, max)| {
                                    Some(match acc {
                                        None => (min, max),
                                        Some((amin, amax)) => (
                                            [
                                                amin[0].min(min[0]),
                                                amin[1].min(min[1]),
                                                amin[2].min(min[2]),
                                            ],
                                            [
                                                amax[0].max(max[0]),
                                                amax[1].max(max[1]),
                                                amax[2].max(max[2]),
                                            ],
                                        ),
                                    })
                                },
                            );
                            let pages: Vec<u32> =
                                part.groups.iter().map(|g| g.texture_page).collect();
                            eprintln!(
                                "draw car={} part={} pages={pages:?} layers={} kind={:?} corner={:?} mirror={:?} local_bounds={:?} car_pos=({:.1},{:.1},{:.1}) yaw={:.2}",
                                sim.spec.id,
                                part.source,
                                cache.art.textures.layer_count(),
                                part.kind,
                                part.corner,
                                part.mirror,
                                bounds,
                                sim.pos.x,
                                sim.pos.y,
                                sim.pos.z,
                                sim.yaw
                            );
                        }
                        let placed = crate::art::part_groups(part, &sim, colour);
                        if std::env::var("DD_TRACE_DRAW").is_ok()
                            && part.kind == crate::art::PartKind::Body
                        {
                            let c = placed.iter().flat_map(|g| g.colors.iter()).fold(
                                None::<([f32; 3], [f32; 3])>,
                                |acc, c| {
                                    Some(match acc {
                                        None => (*c, *c),
                                        Some((lo, hi)) => (
                                            [lo[0].min(c[0]), lo[1].min(c[1]), lo[2].min(c[2])],
                                            [hi[0].max(c[0]), hi[1].max(c[1]), hi[2].max(c[2])],
                                        ),
                                    })
                                },
                            );
                            eprintln!(
                                "car paint colour range for {}: {c:?} (livery {:?})",
                                sim.spec.id, colour
                            );
                        }
                        if std::env::var("DD_TRACE_DRAW").is_ok() {
                            // Where the part actually lands, not where its mesh sits.
                            let world = placed.iter().filter_map(|g| g.bounds()).fold(
                                None::<([f32; 3], [f32; 3])>,
                                |acc, (lo, hi)| {
                                    Some(match acc {
                                        None => (lo, hi),
                                        Some((alo, ahi)) => (
                                            [
                                                alo[0].min(lo[0]),
                                                alo[1].min(lo[1]),
                                                alo[2].min(lo[2]),
                                            ],
                                            [
                                                ahi[0].max(hi[0]),
                                                ahi[1].max(hi[1]),
                                                ahi[2].max(hi[2]),
                                            ],
                                        ),
                                    })
                                },
                            );
                            if let Some((lo, hi)) = world {
                                eprintln!(
                                    "placed car={} part={} corner={:?} world_x=[{:.1},{:.1}] \
                                     world_y=[{:.1},{:.1}] world_z=[{:.1},{:.1}]",
                                    sim.spec.id,
                                    part.source,
                                    part.corner,
                                    lo[0],
                                    hi[0],
                                    lo[1],
                                    hi[1],
                                    lo[2],
                                    hi[2]
                                );
                            }
                        }
                        if std::env::var("DD_TRACE_DRAW").is_ok()
                            && matches!(
                                part.kind,
                                crate::art::PartKind::Body | crate::art::PartKind::Wheel
                            )
                        {
                            for (i, g) in placed.iter().enumerate() {
                                let max_index =
                                    g.indices.iter().copied().max().unwrap_or(0) as usize;
                                eprintln!(
                                    "group {}-{i}: pos={} uvs={} normals={} idx={} max_idx={} page={} \
                                     alpha={:.3} transparent={} depth_test={} finite={}",
                                    part.source,
                                    g.positions.len(),
                                    g.uvs.len(),
                                    g.normals.len(),
                                    g.indices.len(),
                                    max_index,
                                    g.texture_page,
                                    g.alpha,
                                    g.transparent,
                                    g.depth_test,
                                    g.positions.iter().all(|p| p.iter().all(|v| v.is_finite()))
                                        && g.normals
                                            .iter()
                                            .all(|n| n.iter().all(|v| v.is_finite()))
                                );
                            }
                        }
                        groups.extend(placed);
                        art_used += 1;
                    }
                    groups.extend(crate::art::missing_wheel_groups(&sim, &art));
                }
                _ => groups.extend(car_groups(&sim, colour)),
            }
        }
        let _ = art_used;
    }
    let (mut target, mut yaw, mut dist, mut pitch) =
        camera_for(&player, phase, viewport_px, mode, road_ahead);
    if let Ok(shape) = std::env::var("DD_DEBUG_CAM") {
        // Isolation aid: a fixed camera that shows the whole layout.
        match shape.as_str() {
            "bird" => {
                target = player.pos;
                yaw = 0.0;
                dist = 70.0;
                pitch = 0.85;
            }
            "side" => {
                target = player.pos + vec3(0.0, 3.0, 20.0);
                yaw = std::f32::consts::FRAC_PI_2;
                dist = 45.0;
                pitch = 0.25;
            }
            _ => {}
        }
    }
    let background = if std::env::var("DD_DEBUG_BG").is_ok() {
        // Isolation aid: an unmistakable colour for the empty regions.
        [1.0, 0.0, 1.0, 1.0]
    } else {
        [0.384, 0.705, 1.0, 1.0]
    };
    let (desc, uploads) = {
        let mut cache = world.resource_mut::<SceneCache>();
        cache.art.textures.preload_cars(data::cars());
        let SceneCache { props, art, .. } = &mut *cache;
        props.preload(&mut art.textures);
        let desc = scene_desc(cache.art.textures.layer_count());
        let uploads = cache.art.textures.uploads();
        (desc, uploads)
    };
    let mut frame = Frame3d {
        background: Some(background),
        viewport_px,
        desc,
        uploads,
        cam: OrbitCamera {
            target,
            yaw,
            pitch,
            dist,
            fov_y_deg: if matches!(phase, Phase::Garage | Phase::MapSelect) {
                38.0
            } else {
                65.0
            },
        },
        light: scene_light(),
        ..Default::default()
    };
    world.insert_resource(FrameCamera {
        view_proj: frame
            .cam
            .view_proj(viewport_px[0].max(1.0) / viewport_px[1].max(1.0)),
    });
    if std::env::var("DD_TRACE_FLOATERS").is_ok() {
        // Anything whose lowest vertex is well above the road at its own z is
        // floating, which reads in motion as debris in the sky.
        if let Some(track_world) = world.get_resource::<TrackWorld>() {
            let track = &track_world.track;
            let mut floating = 0usize;
            for g in &groups {
                let Some((min, max)) = g.bounds() else {
                    continue;
                };
                let z = (min[2] + max[2]) * 0.5;
                if !(0.0..=track.length).contains(&z) {
                    continue;
                }
                let height = track.height(z.clamp(0.0, track.length));
                if min[1] - height > 3.0 {
                    floating += 1;
                    if floating <= 12 {
                        eprintln!(
                            "floater: min_y {:.1} road {:.1} above by {:.1} size ({:.1},{:.1},{:.1})",
                            min[1],
                            height,
                            min[1] - height,
                            max[0] - min[0],
                            max[1] - min[1],
                            max[2] - min[2]
                        );
                    }
                }
            }
            eprintln!("floating groups: {floating}/{}", groups.len());
        }
    }
    if std::env::var("DD_TRACE_SCENE").is_ok() {
        let bad = groups
            .iter()
            .filter(|g| {
                g.positions
                    .iter()
                    .any(|p| !p[0].is_finite() || !p[1].is_finite() || !p[2].is_finite())
            })
            .count();
        let bounds = groups.iter().filter_map(|g| g.bounds()).fold(
            None::<([f32; 3], [f32; 3])>,
            |acc, (min, max)| {
                Some(match acc {
                    None => (min, max),
                    Some((amin, amax)) => (
                        [
                            amin[0].min(min[0]),
                            amin[1].min(min[1]),
                            amin[2].min(min[2]),
                        ],
                        [
                            amax[0].max(max[0]),
                            amax[1].max(max[1]),
                            amax[2].max(max[2]),
                        ],
                    ),
                })
            },
        );
        let tris: usize = groups.iter().map(|g| g.tri_count()).sum();
        let transparent: Vec<_> = groups
            .iter()
            .filter(|g| g.transparent || g.alpha < 1.0)
            .map(|g| (g.alpha, g.bounds()))
            .take(8)
            .collect();
        eprintln!("  transparent groups: {transparent:?}");
        for (i, g) in groups.iter().enumerate().take(4) {
            eprintln!(
                "  group[{i}] bounds={:?} tris={} depth_test={} transparent={} page={}",
                g.bounds(),
                g.tri_count(),
                g.depth_test,
                g.transparent,
                g.texture_page
            );
        }
        eprintln!(
            "scene-bounds groups={} tris={} bad={} bounds={:?}",
            groups.len(),
            tris,
            bad,
            bounds
        );
    }
    if std::env::var("DD_TRACE_CAMERA").is_ok() {
        // Where the car lands on screen: a camera that loses it is the failure
        // the trace needs to show, not just describe.
        let aspect = viewport_px[0].max(1.0) / viewport_px[1].max(1.0);
        let clip = frame.cam.view_proj(aspect) * (player.pos + vec3(0.0, 1.0, 0.0)).extend(1.0);
        if clip.w > 1e-4 {
            let ndc = clip.truncate() / clip.w;
            eprintln!(
                "car-on-screen x={:.0}/{} y={:.0}/{} (ndc {:.2},{:.2})",
                (ndc.x * 0.5 + 0.5) * viewport_px[0],
                viewport_px[0] as i32,
                (1.0 - (ndc.y * 0.5 + 0.5)) * viewport_px[1],
                viewport_px[1] as i32,
                ndc.x,
                ndc.y
            );
        } else {
            eprintln!("car-on-screen: behind the camera");
        }
        {
            let clip = frame.cam.view_proj(aspect) * (road_ahead + vec3(0.0, 1.2, 0.0)).extend(1.0);
            if clip.w > 1e-4 {
                let ndc = clip.truncate() / clip.w;
                eprintln!(
                    "road-on-screen x={:.0}/{} ({}% from centre) forward=({:.2},{:.2},{:.2})",
                    (ndc.x * 0.5 + 0.5) * viewport_px[0],
                    viewport_px[0] as i32,
                    ((ndc.x * 0.5 + 0.5) * viewport_px[0] - viewport_px[0] * 0.5) / viewport_px[0]
                        * 100.0,
                    frame.cam.forward().x,
                    frame.cam.forward().y,
                    frame.cam.forward().z
                );
            }
        }
    }
    if std::env::var("DD_TRACE_CAMERA").is_ok() {
        let eye = frame.cam.eye();
        let fwd = frame.cam.forward();
        eprintln!(
            "camera eye=({:.1},{:.1},{:.1}) target=({:.1},{:.1},{:.1}) dist={:.1} yaw={:.2} forward=({:.2},{:.2},{:.2}) car=({:.1},{:.1},{:.1}) car_yaw={:.2}",
            eye.x,
            eye.y,
            eye.z,
            frame.cam.target.x,
            frame.cam.target.y,
            frame.cam.target.z,
            frame.cam.dist,
            frame.cam.yaw,
            fwd.x,
            fwd.y,
            fwd.z,
            player.pos.x,
            player.pos.y,
            player.pos.z,
            player.yaw
        );
    }
    if let Ok(keep) = std::env::var("DD_DEBUG_KEEP") {
        // Isolation aid: render one slice of the scene.
        groups.retain(|g| match keep.as_str() {
            "untextured" => g.uvs.is_empty(),
            "textured" => !g.uvs.is_empty(),
            _ => true,
        });
    }
    if let Ok(n) = std::env::var("DD_DEBUG_FIRST")
        && let Ok(n) = n.parse::<usize>()
    {
        groups.truncate(n);
    }
    if std::env::var("DD_DEBUG_CUBE").is_ok() {
        // Isolation aid: one hand-built box at the camera target. If this shows,
        // the pipeline works and the scene art is at fault, and vice versa.
        let mut m = mesh();
        push_box(
            &mut m,
            frame.cam.target,
            vec3(1.5, 1.5, 1.5),
            [0.9, 0.15, 0.15],
        );
        groups.clear();
        groups.push(m);
    }
    for g in groups {
        let g = with_normals(g);
        if !g.is_empty() {
            frame.push(g);
        }
    }
    Viewport3d(frame, geom.clone(), "deadly-descent", desc, |_ev| {})
}

/// `OrbitCamera::eye` places the eye along `(cos yaw, sin yaw)` in XZ, while the
/// car's forward is `(sin yaw, cos yaw)`: the camera yaw that puts the eye behind
/// the car is therefore `-car.yaw - pi/2`.
/// Camera presets, as the build's `CameraPreset` names them: a chase view and a
/// first-person cockpit view (`C` switches, the shipped "change camera view" key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    Chase,
    Cockpit,
}

/// The camera the last frame was rendered with, so a diagnostic can project
/// world points exactly as the frame did.
#[derive(bevy_ecs::prelude::Resource, Clone, Copy, Debug)]
pub struct FrameCamera {
    pub view_proj: glam::Mat4,
}

/// How far down the track the camera aims, in metres. Short enough that the car
/// stays above the bottom of the frame at speed: aiming far ahead drops it out.
const CAMERA_LOOK_AHEAD: f32 = 12.0;

fn camera_for(
    car: &CarSim,
    phase: Phase,
    viewport_px: [f32; 2],
    mode: CameraMode,
    road_ahead: Vec3,
) -> (Vec3, f32, f32, f32) {
    if matches!(phase, Phase::Garage | Phase::MapSelect) {
        return (Vec3::new(0.0, 1.0, 0.0), 0.5, 9.5, 0.35);
    }
    let speed = car.speed();
    let portrait = viewport_px[1] > viewport_px[0];
    let (back, up, aim_height) = match mode {
        CameraMode::Chase => {
            let back = if portrait { 10.5 } else { 8.5 } + speed * 0.05;
            (back, 3.4 + speed * 0.010, 1.2)
        }
        CameraMode::Cockpit => (0.0, 1.35, 0.6),
    };
    let flat = vec3(car.forward().x, 0.0, car.forward().z).normalize_or_zero();
    let mut eye = car.pos + vec3(0.0, up, 0.0) - flat * back;
    let mut target = road_ahead + vec3(0.0, aim_height, 0.0);
    // A fatal crash can drop the car through the track. The screen that reports
    // it should still look along the road, not up at its underside.
    if matches!(phase, Phase::Failed) {
        let floor = road_ahead.y + 1.2;
        if eye.y < floor {
            let lift = floor - eye.y;
            eye.y += lift;
            target.y += lift;
        }
    }
    // `OrbitCamera` places the eye at `target + dist * (yaw, pitch)`, so both are
    // read straight off the eye-to-target vector.
    let to_eye = eye - target;
    let dist = to_eye.length().max(0.1);
    (
        target,
        to_eye.z.atan2(to_eye.x),
        dist,
        (to_eye.y / dist).clamp(-1.0, 1.0).asin(),
    )
}

pub fn viewport2d(ui: &crate::ui::SharedUi) -> Vec2 {
    ui.viewport
}
