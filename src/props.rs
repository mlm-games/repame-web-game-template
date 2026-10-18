//! World dressing from the shipped meshes.
//!
//! Every obstacle, barrier, tree, grandstand, footbridge and terrain ridge is one
//! of the game's own exported meshes, placed by the sim's track geometry: edge
//! barriers every 12 m (the shipped barrier is 12.1 m long), roadside scatter by
//! seeded choice, and the hazard meshes bound to their [`crate::sim::obstacles::Kind`].
//! Without the export the caller's procedural stand-ins stand in.

use std::collections::HashMap;

use glam::{Vec3, vec3};
use repame_view3d::MeshGroup;
use repame_view3d::gltf;
use serde::Deserialize;

use crate::art::art_root;
use crate::sim::obstacles::Kind;
use crate::sim::seed::unit;
use crate::sim::track::Track;

#[derive(Clone, Debug, Deserialize)]
pub struct PropDef {
    pub file: String,
    #[serde(default)]
    pub yaw_deg: f32,
    #[serde(default = "unit_scale")]
    pub scale: [f32; 3],
    #[serde(default)]
    pub lift: f32,
}

fn unit_scale() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Clone, Debug, Deserialize)]
pub struct EdgeDef {
    pub file: String,
    pub every: f32,
    pub offset: f32,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ScatterDef {
    pub file: String,
    pub prob: f32,
    pub min: f32,
    pub max: f32,
    #[serde(default)]
    pub spans: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct RidgeDef {
    pub file: String,
    pub scale: f32,
}

/// One placed piece of the shipped modular track (`GRID-R-*`, `C-*`), in
/// `StreamingAssets/glb` and imported at `scale = 0.01` exactly as the build's
/// `GlbTrackImporter` does.
#[derive(Clone, Debug, Deserialize)]
pub struct TrackPiece {
    pub file: String,
    pub scale: f32,
    pub lateral: f32,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct TrackPieces {
    #[serde(default)]
    pub start: Vec<TrackPiece>,
    #[serde(default)]
    pub gantry: Option<GantryDef>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct GantryDef {
    pub file: String,
    pub scale: f32,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct PropTable {
    #[serde(default)]
    pub track_pieces: TrackPieces,
    #[serde(default)]
    pub obstacle: HashMap<String, PropDef>,
    pub edge: Option<EdgeDef>,
    #[serde(default)]
    pub ridges: Vec<RidgeDef>,
    #[serde(default)]
    pub scatter: Vec<ScatterDef>,
}

impl PropTable {
    pub fn obstacle_for(&self, kind: Kind) -> Option<&PropDef> {
        let key = match kind {
            Kind::Container => "Container",
            Kind::Crate => "Crate",
            Kind::Spikes => "Spikes",
            Kind::Train => "Train",
            Kind::Ramp => "Ramp",
            Kind::Bus => "Bus",
            Kind::Wall => "Wall",
            // No shipped mesh for these: the caller's stand-ins cover them.
            Kind::Ball | Kind::Press | Kind::Gate | Kind::Hole => return None,
        };
        self.obstacle.get(key)
    }
}

/// Loaded reference meshes, cached by file stem.
#[derive(Default)]
pub struct PropCache {
    meshes: HashMap<String, Option<Vec<MeshGroup>>>,
}

impl PropCache {
    pub fn get(&mut self, file: &str, textures: &mut crate::art::Textures) -> Option<&[MeshGroup]> {
        let entry = self
            .meshes
            .entry(file.to_string())
            .or_insert_with(|| load_prop(file, textures));
        entry.as_deref()
    }

    /// Bind every texture the table references, so the batch's page count is
    /// final before the first frame draws.
    pub fn preload(&mut self, textures: &mut crate::art::Textures) {
        let keys: Vec<String> = {
            let _ = self;
            crate::props::touch_files()
        };
        for file in keys {
            let _ = self.get(&file, textures);
        }
    }

    /// Build the track's static dressing: edge barriers, ridges, the level's own
    /// props and the generic roadside scatter. Deterministic in the level's seed,
    /// so a track looks the same every run.
    pub fn dressing(
        &mut self,
        track: &Track,
        segment: &str,
        textures: &mut crate::art::Textures,
    ) -> Vec<MeshGroup> {
        let table = prop_table();
        let mut out: Vec<MeshGroup> = Vec::new();
        let seed = track.seed;
        // edge barriers both sides
        if let Some(edge) = &table.edge
            && let Some(groups) = self.get(&edge.file, textures).map(|g| g.to_vec())
        {
            let mut s = 8.0;
            let mut salt = 7u32;
            while s < track.length - 8.0 {
                for side in [-1.0f32, 1.0] {
                    let yaw = tangent_yaw(track, s);
                    let pos = track.road_pos(s, side * edge.offset);
                    for g in &groups {
                        out.push(place(
                            g,
                            pos,
                            yaw + std::f32::consts::FRAC_PI_2,
                            [1.0, 1.0, 1.0],
                        ));
                    }
                }
                salt = salt.wrapping_add(1);
                s += edge.every * (0.9 + unit(seed, salt) * 0.2);
            }
        }
        // background ridges along one side, offset well clear of the road
        if !table.ridges.is_empty() {
            let ridges: Vec<(String, f32)> = table
                .ridges
                .iter()
                .map(|r| (r.file.clone(), r.scale))
                .collect();
            let mut s = 40.0;
            let mut index = 0usize;
            while s < track.length + 200.0 {
                let (file, scale) = &ridges[index % ridges.len()];
                index += 1;
                if let Some(groups) = self.get(file, textures).map(|g| g.to_vec()) {
                    let side = if index.is_multiple_of(2) { 1.0 } else { -1.0 };
                    let distance = 60.0 + unit(seed, index as u32 + 40) * 60.0;
                    let pos = track.road_pos(s.min(track.length) - 12.0, side * distance);
                    let yaw = tangent_yaw(track, s.min(track.length));
                    for g in &groups {
                        out.push(place(g, pos, yaw, [*scale, *scale, *scale]));
                    }
                }
                s += 150.0;
            }
        }
        // the level's own props, taken from the shipped segment scene
        for file in level_palette(segment) {
            let Some(groups) = self.get(file, textures).map(|g| g.to_vec()) else {
                continue;
            };
            let mut s = 40.0;
            let mut salt = 31u32;
            while s < track.length - 20.0 {
                salt = salt.wrapping_add(1);
                if unit(seed, salt) > 0.45 {
                    s += 16.0;
                    continue;
                }
                let side = if unit(seed, salt ^ 0x77) < 0.5 {
                    -1.0
                } else {
                    1.0
                };
                let distance = 13.0 + unit(seed, salt ^ 0x99) * 34.0;
                let pos = track.road_pos(s, side * distance);
                let yaw = tangent_yaw(track, s) + (unit(seed, salt ^ 0xAB) - 0.5) * 1.2;
                for g in &groups {
                    out.push(place(g, pos, yaw, [1.0, 1.0, 1.0]));
                }
                s += 22.0;
            }
        }
        // roadside scatter
        let scatter: Vec<ScatterDef> = table.scatter.clone();
        let mut s = 30.0;
        let mut salt = 91u32;
        while s < track.length - 20.0 {
            for def in &scatter {
                salt = salt.wrapping_add(1);
                if unit(seed, salt) > def.prob {
                    continue;
                }
                let Some(groups) = self.get(&def.file, textures).map(|g| g.to_vec()) else {
                    continue;
                };
                let side = if unit(seed, salt ^ 0x5A5A) < 0.5 {
                    -1.0
                } else {
                    1.0
                };
                let lateral = if def.spans {
                    0.0
                } else {
                    side * (def.min + unit(seed, salt ^ 0x1234) * (def.max - def.min).max(0.0))
                };
                let pos = track.road_pos(s, lateral);
                let yaw = tangent_yaw(track, s);
                for g in &groups {
                    out.push(place(g, pos, yaw, [1.0, 1.0, 1.0]));
                }
            }
            salt = salt.wrapping_add(1);
            s += 18.0;
        }
        out
    }
}

/// The shipped modular deck (start grid rows) for the racing levels, placed with
/// the piece's own 0.01 import scale.
pub fn start_deck(
    cache: &mut PropCache,
    textures: &mut crate::art::Textures,
    track: &Track,
) -> Vec<MeshGroup> {
    let table = prop_table();
    let mut out = Vec::new();
    if table.track_pieces.start.is_empty() {
        return out;
    }
    for s in [4.0f32, 9.0] {
        for piece in &table.track_pieces.start {
            let Some(groups) = cache.get(&piece.file, textures).map(|g| g.to_vec()) else {
                continue;
            };
            let pos = track.road_pos(s, piece.lateral);
            let yaw = tangent_yaw(track, s);
            let scale = [piece.scale, piece.scale, piece.scale];
            for g in &groups {
                out.push(place(g, pos, yaw, scale));
            }
        }
    }
    if let Some(gantry) = &table.track_pieces.gantry
        && let Some(groups) = cache.get(&gantry.file, textures).map(|g| g.to_vec())
    {
        let s = (track.length - 8.0).max(0.0);
        let pos = track.road_pos(s, 0.0);
        let yaw = tangent_yaw(track, s);
        let scale = [gantry.scale, gantry.scale, gantry.scale];
        for g in &groups {
            out.push(place(g, pos, yaw, scale));
        }
    }
    out
}

/// Yaw of the road's direction of travel at a distance along the track.
pub fn tangent_yaw(track: &Track, s: f32) -> f32 {
    let a = track.centre((s - 2.0).max(0.0));
    let b = track.centre((s + 2.0).min(track.length));
    let d = b - a;
    d.x.atan2(d.z)
}

/// Rotate/scale/translate a cached group into world space.
fn place(group: &MeshGroup, at: Vec3, yaw: f32, scale: [f32; 3]) -> MeshGroup {
    let (sy, cy) = yaw.sin_cos();
    let mut out = group.clone();
    for p in out.positions.iter_mut() {
        let local = vec3(p[0] * scale[0], p[1] * scale[1], p[2] * scale[2]);
        let world = at
            + vec3(
                local.x * cy + local.z * sy,
                local.y,
                -local.x * sy + local.z * cy,
            );
        *p = world.to_array();
    }
    out
}

/// Place one prop instance for an obstacle, if the art exists.
pub fn obstacle_groups(
    cache: &mut PropCache,
    textures: &mut crate::art::Textures,
    def: &PropDef,
    at: Vec3,
    yaw: f32,
) -> Option<Vec<MeshGroup>> {
    let groups = cache.get(&def.file, textures)?.to_vec();
    let yaw = yaw + def.yaw_deg.to_radians();
    Some(
        groups
            .iter()
            .map(|g| place(g, at + vec3(0.0, def.lift, 0.0), yaw, def.scale))
            .collect(),
    )
}

const PROPS_JSON: &str = include_str!("../assets/dd1/props.json");

pub fn prop_table() -> &'static PropTable {
    static TABLE: std::sync::OnceLock<PropTable> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| match serde_json::from_str::<PropTable>(PROPS_JSON) {
        Ok(table) => table,
        Err(error) => {
            log::warn!("deadly-descent: props.json failed to parse: {error}");
            PropTable::default()
        }
    })
}

fn load_prop(file: &str, textures: &mut crate::art::Textures) -> Option<Vec<MeshGroup>> {
    let root = art_root()?;
    // Bundle meshes first, then the modular track pieces that ship as GLBs in
    // StreamingAssets (imported at 0.01 by the build's own importer).
    let packaged = root.join("meshes").join(format!("{file}.glb"));
    let streamed = root.parent().map(|parent| {
        parent
            .join("StreamingAssets")
            .join("glb")
            .join(format!("{file}.glb"))
    });
    let bytes = match std::fs::read(&packaged) {
        Ok(bytes) => bytes,
        Err(_) => {
            let path = streamed?;
            std::fs::read(&path).ok()?
        }
    };
    let meshes = gltf::import_slice(&bytes).ok()?;
    let entry = crate::art::mesh_material(file);
    let page = entry
        .and_then(|e| e.texture_name())
        .and_then(|name| textures.page_sized(name, crate::art::Textures::PROP_PAGE));
    let tint = entry.and_then(|e| e.tint());
    let mut groups: Vec<MeshGroup> = Vec::new();
    for mesh in meshes {
        for mut group in mesh.groups {
            if group.is_empty() {
                continue;
            }
            match page {
                Some(page) => {
                    group.texture_page = page;
                    if let Some(tint) = tint {
                        for c in group.colors.iter_mut() {
                            *c = [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]];
                        }
                    }
                }
                None => {
                    // No page: uvs would sample empty texels.
                    group.uvs.clear();
                }
            }
            groups.push(group);
        }
    }
    (!groups.is_empty()).then_some(groups)
}

/// Per-level prop palettes: the meshes each shipped segment scene actually
/// contains (recorded in `assets/dd1/level_props.json`). They dress that level
/// with its own art instead of the generic roadside set.
const LEVEL_PROPS_JSON: &str = include_str!("../assets/dd1/level_props.json");

fn level_palette(segment: &str) -> &'static [String] {
    static TABLE: std::sync::OnceLock<HashMap<String, Vec<String>>> = std::sync::OnceLock::new();
    let table = TABLE.get_or_init(|| {
        serde_json::from_str::<HashMap<String, Vec<String>>>(LEVEL_PROPS_JSON).unwrap_or_default()
    });
    table.get(segment).map(|v| v.as_slice()).unwrap_or(&[])
}

/// Every mesh file the prop table references.
pub fn touch_files() -> Vec<String> {
    let table = prop_table();
    let mut files: Vec<String> = table.obstacle.values().map(|d| d.file.clone()).collect();
    if let Some(edge) = &table.edge {
        files.push(edge.file.clone());
    }
    files.extend(table.ridges.iter().map(|r| r.file.clone()));
    files.extend(table.scatter.iter().map(|s| s.file.clone()));
    files.extend(table.track_pieces.start.iter().map(|p| p.file.clone()));
    if let Some(gantry) = &table.track_pieces.gantry {
        files.push(gantry.file.clone());
    }
    files.sort();
    files.dedup();
    files
}
