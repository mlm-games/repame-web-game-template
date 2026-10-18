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
use crate::sim::race::{Phase, Session, TrackWorld};

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
const ROAD_EDGE: [f32; 3] = [0.24, 0.22, 0.21];
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

fn build_static(track_world: &TrackWorld, level: &data::Level) -> Vec<MeshGroup> {
    let racing = level.is_racing_track();
    let road_tone = if racing { ASPHALT } else { DIRT };
    let verge_tone = if racing { GRASS } else { SAND };
    let track = &track_world.track;
    let mut road = MeshGroup::default();
    let mut shoulder = MeshGroup::default();
    let steps = 140usize;
    let step = track.length / steps as f32;
    let half = track.half_width;
    for i in 0..steps {
        let s0 = i as f32 * step;
        let s1 = s0 + step;
        let c0 = track.centre(s0);
        let c1 = track.centre(s1);
        // road surface
        let quad = [
            vec3(c0.x - half, c0.y, c0.z),
            vec3(c0.x + half, c0.y, c0.z),
            vec3(c1.x + half, c1.y, c1.z),
            vec3(c1.x - half, c1.y, c1.z),
        ];
        let base = road.positions.len() as u32;
        for (k, v) in quad.iter().enumerate() {
            road.positions.push(v.to_array());
            road.colors.push(if (i + k) % 24 < 12 {
                road_tone
            } else {
                ROAD_EDGE
            });
        }
        road.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        // downhill skirt either side, so the road reads as cut into a slope
        for side in [-1.0f32, 1.0] {
            let a = vec3(c0.x + side * half, c0.y, c0.z);
            let b = vec3(c1.x + side * half, c1.y, c1.z);
            let c = vec3(c1.x + side * half, c1.y - 9.0, c1.z);
            let d = vec3(c0.x + side * half, c0.y - 9.0, c0.z);
            let base = shoulder.positions.len() as u32;
            for v in [a, b, c, d] {
                shoulder.positions.push(v.to_array());
                shoulder.colors.push(MOUNTAIN);
            }
            let flip = side > 0.0;
            if flip {
                shoulder.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            } else {
                shoulder.indices.extend_from_slice(&[
                    base + 1,
                    base,
                    base + 3,
                    base + 1,
                    base + 3,
                    base + 2,
                ]);
            }
        }
    }
    // grass verges
    let mut verge = MeshGroup::default();
    for i in 0..steps {
        let s0 = i as f32 * step;
        let s1 = (i + 1) as f32 * step;
        let c0 = track.centre(s0);
        let c1 = track.centre(s1);
        for side in [-1.0f32, 1.0] {
            let a = vec3(c0.x + side * half, c0.y - 0.05, c0.z);
            let b = vec3(c1.x + side * half, c1.y - 0.05, c1.z);
            let c = vec3(
                c1.x + side * (half + 26.0),
                c1.y - 5.0 - i as f32 * 0.05,
                c1.z,
            );
            let d = vec3(
                c0.x + side * (half + 26.0),
                c0.y - 5.0 - i as f32 * 0.05,
                c0.z,
            );
            let base = verge.positions.len() as u32;
            for v in [a, b, c, d] {
                verge.positions.push(v.to_array());
                verge.colors.push(verge_tone);
            }
            if side > 0.0 {
                verge.indices.extend_from_slice(&[
                    base,
                    base + 1,
                    base + 2,
                    base,
                    base + 2,
                    base + 3,
                ]);
            } else {
                verge.indices.extend_from_slice(&[
                    base + 1,
                    base,
                    base + 3,
                    base + 1,
                    base + 3,
                    base + 2,
                ]);
            }
        }
    }
    vec![road, shoulder, verge]
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
    if dst.normals.len() == dst.positions.len() - src.positions.len()
        && src.normals.len() == src.positions.len()
    {
        dst.normals.extend_from_slice(&src.normals);
    }
    dst.indices.extend(src.indices.iter().map(|i| i + base));
}

fn obstacle_groups_with_props(
    cache: &mut crate::props::PropCache,
    textures: &mut crate::art::Textures,
    track_world: &TrackWorld,
    horizon: Option<(f32, f32)>,
) -> Vec<MeshGroup> {
    let table = crate::props::prop_table();
    let track = &track_world.track;
    let mut opaque = MeshGroup::default();
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
            && let Some(groups) = crate::props::obstacle_groups(cache, textures, def, at, yaw)
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
            push_standin(&mut opaque, ob, at);
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
    let mut body = MeshGroup::default();
    let mut glass = MeshGroup::default();
    let (s, c) = sim.yaw.sin_cos();
    let rot = |v: Vec3| vec3(v.x * c - v.z * s, v.y, v.x * s + v.z * c);
    let base = sim.pos + vec3(0.0, wheel_r + 0.55, 0.0);
    let pitch = sim.pitch;
    let (sp, cp) = pitch.sin_cos();
    let tilt = |v: Vec3| vec3(v.x, v.y * cp - v.z * sp, v.y * sp + v.z * cp);
    // body
    let body_half = vec3(width * 0.5, 0.42, len * 0.5);
    let centre = base + rot(tilt(vec3(0.0, 0.15, 0.0)));
    let mut cube = MeshGroup::default();
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
    let mut cube = MeshGroup::default();
    push_box(&mut cube, Vec3::ZERO, cab_half, GLASS);
    for v in cube.positions.iter_mut() {
        let local = Vec3::from(*v);
        *v = (cab_centre + rot(tilt(local))).to_array();
    }
    glass.positions.extend(cube.positions);
    glass.colors.extend(cube.colors);
    glass.indices.extend(cube.indices);
    // wheels
    let mut wheels = MeshGroup::default();
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

pub fn frame_for(
    world: &mut World,
    _ctx: &repose_core::RenderContext,
    viewport_px: [f32; 2],
    geom: &GeomHandle,
) -> View {
    let Some(session) = world.get_resource::<Session>() else {
        return repose_ui::Spacer();
    };
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
    let mut groups: Vec<MeshGroup> = Vec::new();
    let mut geometry_ready = false;
    // `resource_scope` gives the cache a mutable borrow while the track stays in
    // the world: nothing can observe a world without a track mid-frame.
    world.resource_scope(|world, track_world: Mut<TrackWorld>| {
        let key = track_key(&level);
        {
            let mut cache = world.resource_mut::<SceneCache>();
            if cache.key != key || cache.static_groups.is_empty() {
                cache.key = key;
                let mut built = build_static(&track_world, &level);
                let SceneCache { props, art, .. } = &mut *cache;
                let dressing =
                    props.dressing(&track_world.track, level.segment(), &mut art.textures);
                built.extend(dressing);
                if level.is_racing_track() {
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
        let mut art_used = 0usize;
        for (sim, colour) in cars {
            let car_id = sim.spec.id.clone();
            let wheels = sim.spec.wheels.clone();
            let mut cache = world.resource_mut::<SceneCache>();
            let art = cache.art.get(car_id.as_str(), &wheels).cloned();
            match art {
                Some(art) if !art.is_empty() => {
                    for part in &art.parts {
                        groups.extend(crate::art::part_groups(part, &sim, colour));
                        art_used += 1;
                    }
                    groups.extend(crate::art::missing_wheel_groups(&sim, &art, colour));
                }
                _ => groups.extend(car_groups(&sim, colour)),
            }
        }
        let _ = art_used;
    }
    let (target, yaw, dist) = camera_for(&player, phase, viewport_px);
    let background = [0.384, 0.705, 1.0, 1.0];
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
            pitch: if matches!(phase, Phase::Garage | Phase::MapSelect) {
                0.35
            } else {
                0.28
            },
            dist,
            fov_y_deg: if matches!(phase, Phase::Garage | Phase::MapSelect) {
                38.0
            } else {
                65.0
            },
        },
        light: SceneLight {
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
        },
        ..Default::default()
    };
    for g in groups {
        if !g.is_empty() {
            frame.push(g);
        }
    }
    Viewport3d(frame, geom.clone(), "deadly-descent", desc, |_ev| {})
}

fn camera_for(car: &CarSim, phase: Phase, viewport_px: [f32; 2]) -> (Vec3, f32, f32) {
    if matches!(phase, Phase::Garage | Phase::MapSelect) {
        return (Vec3::new(0.0, 1.0, 0.0), 0.5, 9.5);
    }
    let speed = car.speed();
    let lead = 6.0 + speed * 0.22;
    let target = car.pos + car.forward() * lead + vec3(0.0, 1.6, 0.0);
    let portrait = viewport_px[1] > viewport_px[0];
    let dist = if portrait { 11.5 } else { 9.0 } + speed * 0.035;
    (target, car.yaw, dist)
}

pub fn viewport2d(ui: &crate::ui::SharedUi) -> Vec2 {
    ui.viewport
}
