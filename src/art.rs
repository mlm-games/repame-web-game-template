//! Original art loading: meshes, textures and material tints.
//!
//! The shipped game's meshes and textures are exported under
//! `refs/DeadlyDescent/extracted/` by the extraction pass (see README). They are
//! reference material and are never committed: without the directory every actor
//! falls back to a boxed stand-in with no textures, and the game still runs.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use glam::{Vec3, vec3};
use repame_view3d::MeshGroup;
use repame_view3d::gltf;
use repame_view3d::render::SceneUpload;
use serde::Deserialize;

use crate::sim::car::CarSim;

/// Square edge of one texture page. Car liveries are authored at 1–2K; one page
/// per texture keeps them legible without packing.
pub const PAGE: u32 = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PartKind {
    Body,
    /// Cabin furniture and instruments: drawn only in cockpit view.
    Interior,
    Wing,
    Caliper,
    Wheel,
    /// Collision proxies and helpers: never drawn.
    Hidden,
    Other,
}

impl PartKind {
    /// Visible from the chase camera?
    pub fn visible_in_chase(self) -> bool {
        !matches!(self, PartKind::Interior | PartKind::Hidden)
    }
}

#[derive(Clone, Debug)]
pub struct ArtPart {
    /// Exported file the part came from, for tracing.
    pub source: String,
    pub groups: Vec<MeshGroup>,
    pub kind: PartKind,
    /// Wheel corner this part belongs to (front/rear, left/right), if any.
    pub corner: Option<usize>,
    /// Mirror factors applied to local x/z, used when one mesh serves several
    /// corners (the export keeps a shared mesh once).
    pub mirror: (f32, f32),
}

#[derive(Clone, Debug, Default)]
pub struct CarArt {
    pub parts: Vec<ArtPart>,
}

impl CarArt {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// Directory holding the extracted reference art.
pub fn art_root() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("DD_ASSETS") {
        let path = PathBuf::from(dir);
        if path.is_dir() {
            return Some(path);
        }
    }
    let home = std::env::var("HOME").ok()?;
    let candidate = Path::new(&home).join("Documents/refs/DeadlyDescent/extracted");
    candidate.is_dir().then_some(candidate)
}

// ---------------------------------------------------------------- materials

fn normalise(name: &str) -> String {
    name.trim().replace(' ', "_").to_ascii_lowercase()
}

/// `assets/dd1/car_materials.json`: mesh owner -> material, its base-colour
/// texture (the exported PNG stem) and the material's colour factor, read from
/// the bundle's MeshRenderers and Materials.
const CAR_MATERIALS_JSON: &str = include_str!("../assets/dd1/car_materials.json");

#[derive(Clone, Debug, Default, Deserialize)]
pub struct MaterialEntry {
    #[serde(default)]
    go: String,
    #[serde(default, rename = "material")]
    _material: Option<String>,
    #[serde(default)]
    texture: Option<String>,
    #[serde(default)]
    colors: Option<HashMap<String, Vec<f32>>>,
}

impl MaterialEntry {
    pub fn texture_name(&self) -> Option<&str> {
        self.texture.as_deref()
    }

    pub fn tint(&self) -> Option<[f32; 3]> {
        let colors = self.colors.as_ref()?;
        let c = colors.get("_Color").or_else(|| colors.get("_BaseColor"))?;
        if c.len() < 3 {
            return None;
        }
        Some([c[0], c[1], c[2]])
    }
}

/// General mesh-owner -> material table, covering every mesh the exporter named
/// by its owning GameObject (`Container`, `Barrier-02`, car bodies, ...).
const MESH_MATERIALS_JSON: &str = include_str!("../assets/dd1/mesh_materials.json");

fn mesh_material_table() -> &'static HashMap<String, MaterialEntry> {
    static TABLE: OnceLock<HashMap<String, MaterialEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        serde_json::from_str::<HashMap<String, MaterialEntry>>(MESH_MATERIALS_JSON)
            .unwrap_or_default()
    })
}

/// Material entry for a mesh file stem: the exported names keep the owner chain,
/// so the longest owner name the stem ends with wins.
pub fn mesh_material(stem: &str) -> Option<&'static MaterialEntry> {
    let table = mesh_material_table();
    let lowered = stem.to_ascii_lowercase();
    let mut best: Option<(&'static MaterialEntry, usize)> = None;
    for (owner, entry) in table.iter() {
        let key = normalise(owner);
        if !key.is_empty() && lowered.ends_with(&key) && key.len() > best.map(|b| b.1).unwrap_or(0)
        {
            best = Some((entry, key.len()));
        }
    }
    best.map(|b| b.0)
}

fn material_table() -> &'static HashMap<String, MaterialEntry> {
    static TABLE: OnceLock<HashMap<String, MaterialEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: HashMap<String, Vec<MaterialEntry>> =
            serde_json::from_str(CAR_MATERIALS_JSON).unwrap_or_default();
        let mut out = HashMap::new();
        for (car, entries) in raw {
            for entry in entries {
                out.insert(format!("{}|{}", car, normalise(&entry.go)), entry);
            }
        }
        out
    })
}

/// Look up a mesh's material entry. Export file names keep the owner chain with
/// spaces turned into underscores (`Porsche_911_View_Body_body`), so the entry
/// is the longest owner name the stem ends with.
fn entry_for(car_id: &str, stem: &str) -> Option<&'static MaterialEntry> {
    let table = material_table();
    let lowered = stem.to_ascii_lowercase();
    let mut best: Option<(&'static MaterialEntry, usize)> = None;
    for (key, entry) in table.iter() {
        let Some((car, owner)) = key.split_once('|') else {
            continue;
        };
        if car != car_id {
            continue;
        }
        if lowered.ends_with(owner) && owner.len() > best.map(|b| b.1).unwrap_or(0) {
            best = Some((entry, owner.len()));
        }
    }
    best.map(|b| b.0)
}

// ---------------------------------------------------------------- textures

#[derive(Clone)]
pub struct TexturePage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub name: String,
}

/// Pages are allocated once and uploaded on the frames after allocation; the
/// batch keeps them, so nothing is re-uploaded per frame.
#[derive(Default)]
pub struct Textures {
    pages: Vec<TexturePage>,
    index: HashMap<String, u32>,
    pending: Vec<u32>,
    failed: Vec<String>,
    /// Layer count the pages were last uploaded against; a change means the
    /// batch was rebuilt and every page has to go up again.
    uploaded_layers: u32,
    preloaded: bool,
}

impl Textures {
    pub fn layer_count(&self) -> u32 {
        (self.pages.len() as u32).max(1)
    }

    /// Bind every car's material textures up front so the batch's layer count
    /// is final before the first frame; pages then upload once.
    pub fn preload_cars(&mut self, cars: &[crate::sim::data::Car]) {
        if self.preloaded {
            return;
        }
        self.preloaded = true;
        let table = material_table();
        let mut names: Vec<String> = Vec::new();
        for car in cars {
            for (key, entry) in table.iter() {
                let Some((owner_car, _)) = key.split_once('|') else {
                    continue;
                };
                if owner_car != car.id {
                    continue;
                }
                if let Some(texture) = entry.texture.as_deref()
                    && !names.iter().any(|n| n == texture)
                {
                    names.push(texture.to_string());
                }
            }
        }
        names.sort();
        for name in names {
            self.page(&name);
        }
    }

    pub fn uploads(&mut self) -> Vec<SceneUpload> {
        let layers = self.layer_count();
        if self.uploaded_layers != layers {
            self.uploaded_layers = layers;
            let all: Vec<u32> = (0..self.pages.len() as u32).collect();
            self.pending = all;
        }
        let pending = std::mem::take(&mut self.pending);
        pending
            .into_iter()
            .filter_map(|page| {
                let texture = self.pages.get(page as usize)?;
                Some(SceneUpload {
                    page,
                    x: 0,
                    y: 0,
                    w: texture.width,
                    h: texture.height,
                    rgba: texture.rgba.clone(),
                })
            })
            .collect()
    }

    /// Pages for props are bound smaller than car liveries: they are seen at
    /// distance and there are many of them.
    pub const PROP_PAGE: u32 = 256;

    pub fn page(&mut self, name: &str) -> Option<u32> {
        self.page_sized(name, PAGE)
    }

    /// Page for an exported texture name, loading and downscaling it on first
    /// use. Names that do not resolve to a file are remembered as failures.
    pub fn page_sized(&mut self, name: &str, max_edge: u32) -> Option<u32> {
        let key = name.trim().to_string();
        if let Some(page) = self.index.get(&key) {
            return Some(*page);
        }
        if self.failed.iter().any(|f| f == &key) {
            return None;
        }
        let root = art_root()?;
        let path = root.join("textures").join(format!("{key}.png"));
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => {
                // A name may have been sanitised differently; try a loose match.
                match loose_texture(&root, &key) {
                    Some(bytes) => bytes,
                    None => {
                        self.failed.push(key);
                        return None;
                    }
                }
            }
        };
        let image = match image::load_from_memory_with_format(&bytes, image::ImageFormat::Png) {
            Ok(image) => image.to_rgba8(),
            Err(error) => {
                log::warn!("deadly-descent: texture {key} failed to decode: {error}");
                self.failed.push(key);
                return None;
            }
        };
        let (mut width, mut height) = image.dimensions();
        let mut rgba = image.into_raw();
        // Fit inside the page, halving until it does.
        while width > max_edge || height > max_edge {
            let (w, h) = ((width / 2).max(1), (height / 2).max(1));
            rgba = half_scale(&rgba, width, height);
            width = w;
            height = h;
        }
        let page = self.pages.len() as u32;
        self.pages.push(TexturePage {
            width,
            height,
            rgba,
            name: key.clone(),
        });
        self.index.insert(key, page);
        self.pending.push(page);
        Some(page)
    }
}

fn loose_texture(root: &Path, key: &str) -> Option<Vec<u8>> {
    let dir = std::fs::read_dir(root.join("textures")).ok()?;
    let wanted = normalise(key);
    for entry in dir.flatten() {
        let path = entry.path();
        let stem = path.file_stem().and_then(|s| s.to_str())?;
        if normalise(stem) == wanted {
            return std::fs::read(&path).ok();
        }
    }
    None
}

fn half_scale(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let (w, h) = ((width / 2).max(1), (height / 2).max(1));
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            for c in 0..4 {
                let x0 = (x * 2).min(width.saturating_sub(1));
                let x1 = (x0 + 1).min(width.saturating_sub(1));
                let y0 = (y * 2).min(height.saturating_sub(1));
                let y1 = (y0 + 1).min(height.saturating_sub(1));
                let at = |px: u32, py: u32| rgba[((py * width + px) * 4 + c) as usize] as u32;
                let sum = at(x0, y0) + at(x1, y0) + at(x0, y1) + at(x1, y1);
                out[((y * w + x) * 4 + c) as usize] = (sum / 4) as u8;
            }
        }
    }
    out
}

// ---------------------------------------------------------------- meshes

fn classify(name: &str, wheels: &[crate::sim::data::WheelSpec]) -> (PartKind, Option<usize>) {
    let lower = name.to_ascii_lowercase();
    let corner = wheels.iter().position(|w| {
        let tag = w.name.to_ascii_lowercase();
        let (f, r) = (tag.starts_with('f'), tag.starts_with('r'));
        let (l, rt) = (tag.ends_with('l'), tag.ends_with('r'));
        let low = name.to_ascii_lowercase();
        ((f && low.contains("_f")) || (r && low.contains("_r")))
            && ((l && (low.contains("_l") || low.contains("fl") || low.contains("rl")))
                || (rt && (low.contains("_r") || low.contains("fr") || low.contains("rr"))))
    });
    if lower.contains("collider") {
        return (PartKind::Hidden, corner);
    }
    let kind = if lower.contains("interior")
        || lower.contains("steering_wheel")
        || lower.contains("steer_wheel")
        || lower.contains("dial")
        || lower.contains("gauge")
        || lower.contains("panel")
        || lower.contains("pedal")
        || lower.contains("seat")
        || lower.contains("mirror")
    {
        PartKind::Interior
    } else if lower.contains("wheel") || lower.contains("whl") || lower.contains("tire") {
        PartKind::Wheel
    } else if lower.contains("caliper") {
        PartKind::Caliper
    } else if lower.contains("wing") || lower.contains("spoiler") {
        PartKind::Wing
    } else if lower.contains("body") {
        PartKind::Body
    } else {
        PartKind::Other
    };
    (kind, corner)
}

/// Load every exported mesh whose file name starts with the car's id, binding
/// each mesh's material texture page and tint.
pub fn load_car_art(
    car_id: &str,
    wheels: &[crate::sim::data::WheelSpec],
    textures: &mut Textures,
) -> Option<CarArt> {
    let root = art_root()?;
    let dir = root.join("meshes");
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut art = CarArt::default();
    let prefix = format!("{car_id}_");
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".glb"))
                .unwrap_or(false)
        })
        .collect();
    files.sort();
    for path in files {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let meshes = match gltf::import_slice(&bytes) {
            Ok(meshes) => meshes,
            Err(error) => {
                log::warn!(
                    "deadly-descent: {} failed to import: {error}",
                    path.display()
                );
                continue;
            }
        };
        let stem = name.trim_end_matches(".glb").to_string();
        let (kind, corner) = classify(&stem, wheels);
        let entry = entry_for(car_id, &stem);
        let page = entry
            .and_then(|e| e.texture.as_deref())
            .and_then(|t| textures.page(t));
        let tint = entry.and_then(|e| e.tint());
        for mesh in meshes {
            if mesh.groups.iter().all(|g| g.is_empty()) {
                continue;
            }
            let groups: Vec<MeshGroup> = mesh
                .groups
                .into_iter()
                .map(|mut group| {
                    match page {
                        Some(page) => {
                            // Textured: keep uvs, sample the page.
                            group.texture_page = page;
                            if let Some(tint) = tint {
                                for c in group.colors.iter_mut() {
                                    *c = [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]];
                                }
                            }
                        }
                        None => {
                            // Untextured: uvs without a page sample empty texels.
                            group.uvs.clear();
                        }
                    }
                    group
                })
                .collect();
            art.parts.push(ArtPart {
                source: stem.clone(),
                groups,
                kind,
                corner,
                mirror: (1.0, 1.0),
            });
        }
    }
    replicate_corners(&mut art, wheels);
    (!art.is_empty()).then_some(art)
}

/// The shipped meshes are shared between corners; the export keeps one copy, so
/// copy it to the corners that have no mesh of their own.
fn replicate_corners(art: &mut CarArt, wheels: &[crate::sim::data::WheelSpec]) {
    let has = |art: &CarArt, kind: PartKind, corner: usize| {
        art.parts
            .iter()
            .any(|p| p.kind == kind && p.corner == Some(corner))
    };
    for kind in [PartKind::Wheel, PartKind::Caliper] {
        let Some(source) = art
            .parts
            .iter()
            .find(|p| p.kind == kind && p.corner.is_some())
            .cloned()
        else {
            continue;
        };
        let source_corner = source.corner.unwrap();
        for target in 0..wheels.len().min(4) {
            if has(art, kind, target) {
                continue;
            }
            let same_side = wheels[source_corner].pos[0].signum() == wheels[target].pos[0].signum();
            let same_end = wheels[source_corner].pos[2].signum() == wheels[target].pos[2].signum();
            let mut part = source.clone();
            part.corner = Some(target);
            part.mirror = (
                if same_side { 1.0 } else { -1.0 },
                if same_end { 1.0 } else { -1.0 },
            );
            art.parts.push(part);
        }
    }
}

/// Transform an art part into world space for a car at its current pose.
pub fn part_groups(art: &ArtPart, sim: &CarSim, colour: [f32; 3]) -> Vec<MeshGroup> {
    let (sy, cy) = sim.yaw.sin_cos();
    let (sp, cp) = sim.pitch.sin_cos();
    let offset = match (art.kind, art.corner) {
        (PartKind::Wheel | PartKind::Caliper, Some(index)) => {
            let w = &sim.spec.wheels[index];
            vec3(w.pos[0], w.pos[1], w.pos[2])
        }
        _ => Vec3::ZERO,
    };
    let mut out = Vec::with_capacity(art.groups.len());
    for group in &art.groups {
        let mut g = group.clone();
        for p in g.positions.iter_mut() {
            let raw = Vec3::from(*p);
            let local = vec3(raw.x * art.mirror.0, raw.y, raw.z * art.mirror.1) + offset;
            let pitched = vec3(
                local.x,
                local.y * cp - local.z * sp,
                local.y * sp + local.z * cp,
            );
            let world = sim.pos
                + vec3(
                    pitched.x * cy - pitched.z * sy,
                    pitched.y,
                    pitched.x * sy + pitched.z * cy,
                );
            *p = world.to_array();
        }
        // Untextured parts still carry the livery so cars stay distinguishable.
        if g.uvs.is_empty() && matches!(art.kind, PartKind::Body | PartKind::Wing) {
            for c in g.colors.iter_mut() {
                *c = [
                    c[0] * (0.35 + colour[0] * 0.65),
                    c[1] * (0.35 + colour[1] * 0.65),
                    c[2] * (0.35 + colour[2] * 0.65),
                ];
            }
        }
        out.push(g);
    }
    out
}

/// Stand-in wheels for corners the export has no mesh for.
pub fn missing_wheel_groups(sim: &CarSim, art: &CarArt, colour: [f32; 3]) -> Vec<MeshGroup> {
    let mut out = Vec::new();
    for (i, w) in sim.spec.wheels.iter().enumerate().take(4) {
        let covered = art
            .parts
            .iter()
            .any(|p| p.kind == PartKind::Wheel && p.corner == Some(i));
        if covered {
            continue;
        }
        let centre = sim.wheel_world(i);
        let mut g = MeshGroup::default();
        push_wheel(
            &mut g,
            centre,
            w.radius,
            sim.spec.track_width() * 0.06,
            [0.08, 0.08, 0.09],
        );
        for c in g.colors.iter_mut() {
            *c = [
                c[0] * (0.4 + colour[0] * 0.6),
                c[1] * (0.4 + colour[1] * 0.6),
                c[2] * (0.4 + colour[2] * 0.6),
            ];
        }
        out.push(g);
    }
    out
}

fn push_wheel(m: &mut MeshGroup, centre: Vec3, radius: f32, half_len: f32, colour: [f32; 3]) {
    let segments = 12;
    for i in 0..segments {
        let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        let (s0, c0) = a0.sin_cos();
        let (s1, c1) = a1.sin_cos();
        let p = |s: f32, c: f32, off: f32| centre + vec3(off, c * radius, s * radius);
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

/// Cache of loaded car art, and the texture pages it binds.
#[derive(Default)]
pub struct ArtCache {
    map: HashMap<String, Option<CarArt>>,
    pub textures: Textures,
}

impl ArtCache {
    pub fn get(&mut self, car_id: &str, wheels: &[crate::sim::data::WheelSpec]) -> Option<&CarArt> {
        let textures = &mut self.textures;
        self.map
            .entry(car_id.to_string())
            .or_insert_with(|| load_car_art(car_id, wheels, textures))
            .as_ref()
    }
}
