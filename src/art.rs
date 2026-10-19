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

/// How many frames re-send every texture page. See [`Textures::uploads`].
pub const UPLOAD_ROUNDS: u32 = 3;

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
    ///
    /// The interior carries the glass: skipping it left every car a hollow shell
    /// with its cabin open to the sky, which is not what the shipped build draws.
    pub fn visible_in_chase(self) -> bool {
        !matches!(self, PartKind::Hidden)
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
    /// Authored transform in the car, when the export's scene has one.
    pub transform: Option<PartTransform>,
}

#[derive(Clone, Debug, Default)]
pub struct CarArt {
    pub parts: Vec<ArtPart>,
}

/// A part's authored transform in its car, as the shipped Garage scene has it.
/// Most meshes are authored in car space, but wheels, spoilers and the interior
/// sit on pivots, and without these they all land on the car's origin.
#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct PartTransform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

const CAR_PARTS_JSON: &str = include_str!("../assets/dd1/car_parts.json");

fn car_parts() -> &'static HashMap<String, PartTransform> {
    static TABLE: OnceLock<HashMap<String, PartTransform>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(CAR_PARTS_JSON).unwrap_or_default())
}

/// Look up a part's transform: owner paths are `Cars/<car>/<path>` or `<car>/<path>`.
fn part_transform(car_id: &str, go_path: &str) -> Option<PartTransform> {
    let path = go_path.strip_prefix("Cars/").unwrap_or(go_path);
    let rest = path.strip_prefix(car_id)?.strip_prefix('/')?;
    car_parts().get(&format!("{car_id}/{rest}")).copied()
}

impl CarArt {
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// Convert an imported mesh to this renderer's conventions.
///
/// The export was written for Unity, whose front faces wind the opposite way
/// from this renderer's, so imported as-is a car body sits entirely back-facing
/// and back-face culling hides every panel: a car reads as a scattering of
/// loose parts while the room around it draws. The extraction also carries no
/// cull information (the .glb files have no materials) and its winding is not
/// even consistent between mesh families, so each triangle gets a mirrored
/// twin. The twin owns its vertices, because the scene shader shades with the
/// vertex normal and never reads `front_facing`: sharing them would light the
/// copy that faces the camera as if it faced away.
pub fn double_sided(group: &mut MeshGroup) {
    let base = group.positions.len() as u32;
    let positions = group.positions.clone();
    let colors = group.colors.clone();
    let uvs = group.uvs.clone();
    let normals: Vec<[f32; 3]> = group
        .normals
        .iter()
        .map(|n| [-n[0], -n[1], -n[2]])
        .collect();
    group.positions.extend_from_slice(&positions);
    group.colors.extend_from_slice(&colors);
    group.normals.extend_from_slice(&normals);
    group.uvs.extend_from_slice(&uvs);
    let mut mirrored = Vec::with_capacity(group.indices.len());
    for tri in group.indices.as_chunks::<3>().0 {
        mirrored.extend_from_slice(&[tri[0] + base, tri[2] + base, tri[1] + base]);
    }
    group.indices.extend_from_slice(&mirrored);
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
    /// Mesh the renderer drew, so a manifest part finds its material exactly.
    #[serde(default)]
    mesh: Option<u64>,
    /// Alpha test threshold for cutout materials (foliage, fences, decals).
    #[serde(default)]
    cutoff: Option<f32>,
    /// Blend instead of cut out.
    #[serde(default)]
    transparent: Option<bool>,
}

impl MaterialEntry {
    pub fn texture_name(&self) -> Option<&str> {
        self.texture.as_deref()
    }

    pub fn cutoff(&self) -> f32 {
        self.cutoff.unwrap_or(0.0)
    }

    pub fn blends(&self) -> bool {
        self.transparent.unwrap_or(false)
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

/// Materials by the mesh they render (`car|mesh id`): a manifest part names its
/// mesh directly, so this beats guessing from the file name.
fn car_mesh_table() -> &'static HashMap<(String, u64), MaterialEntry> {
    static TABLE: OnceLock<HashMap<(String, u64), MaterialEntry>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let raw: HashMap<String, Vec<MaterialEntry>> =
            serde_json::from_str(CAR_MATERIALS_JSON).unwrap_or_default();
        let mut out = HashMap::new();
        for (car, entries) in raw {
            for entry in entries {
                if let Some(mesh) = entry.mesh {
                    out.insert((car.clone(), mesh), entry);
                }
            }
        }
        out
    })
}

// ------------------------------------------------------- export manifest

#[derive(Deserialize, Default)]
struct ManifestOwner {
    #[serde(default)]
    game_object_path: Option<String>,
}

#[derive(Deserialize, Default)]
struct ManifestMeshEntry {
    #[serde(default)]
    file_rel: Option<String>,
    #[serde(default)]
    path_id: Option<u64>,
    #[serde(default)]
    owners: Vec<ManifestOwner>,
}

#[derive(Deserialize, Default)]
struct Manifest {
    #[serde(default)]
    meshes: Vec<ManifestMeshEntry>,
}

/// The art export's own manifest, next to the meshes it names. Cars are built
/// from it: the exporter writes each mesh once, under whichever GameObject name
/// it found, so a part's *owner path* (`Ford_F150_Raptor/View/Body/roof rack`) is
/// what says which car it belongs to.
fn export_manifest() -> Option<&'static Manifest> {
    static MANIFEST: OnceLock<Option<Manifest>> = OnceLock::new();
    MANIFEST
        .get_or_init(|| {
            let root = art_root()?;
            let bytes = std::fs::read(root.join("MANIFEST.json")).ok()?;
            match serde_json::from_slice::<Manifest>(&bytes) {
                Ok(manifest) => Some(manifest),
                Err(error) => {
                    log::warn!("deadly-descent: MANIFEST.json failed to parse: {error}");
                    None
                }
            }
        })
        .as_ref()
}

/// Every exported mesh a car owns: the file to load, the GameObject path it came
/// from (which classifies it) and its mesh id (which finds its material).
fn manifest_parts(car_id: &str) -> Vec<(String, String, Option<u64>)> {
    let Some(manifest) = export_manifest() else {
        return Vec::new();
    };
    let mut out: Vec<(String, String, Option<u64>)> = Vec::new();
    for mesh in &manifest.meshes {
        let Some(file) = mesh.file_rel.as_deref() else {
            continue;
        };
        let mut best: Option<(bool, usize, &str)> = None;
        for owner in &mesh.owners {
            let Some(path) = owner.game_object_path.as_deref() else {
                continue;
            };
            if !path.split('/').any(|segment| segment == car_id) {
                continue;
            }
            // The driver mannequin hangs off the car too, and its body is a
            // torso: drawing it as car art puts a limb where the car should be.
            if path.split('/').any(|segment| segment == "Mannequin") {
                continue;
            }
            let prefab = path.starts_with("LevelPrefab");
            let key = (prefab, path.len());
            if best.map(|b| key < (b.0, b.1)).unwrap_or(true) {
                best = Some((prefab, path.len(), path));
            }
        }
        let Some((_, _, go_path)) = best else {
            continue;
        };
        if out.iter().any(|(f, path, _)| f == file && path == go_path) {
            continue;
        }
        out.push((file.to_string(), go_path.to_string(), mesh.path_id));
    }
    out.sort();
    out
}

/// `assets/dd1/materials.json`: the shipped materials, by name.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct PortMaterial {
    #[serde(default)]
    pub texture: Option<String>,
    #[serde(default)]
    pub color: Option<[f32; 3]>,
    #[serde(default)]
    pub cutoff: Option<f32>,
    #[serde(default)]
    pub transparent: Option<bool>,
}

const MATERIALS_JSON: &str = include_str!("../assets/dd1/materials.json");
/// `assets/dd1/mesh_submaterials.json`: mesh id -> materials, one per submesh.
const MESH_SUBMATERIALS_JSON: &str = include_str!("../assets/dd1/mesh_submaterials.json");

fn materials() -> &'static HashMap<String, PortMaterial> {
    static TABLE: OnceLock<HashMap<String, PortMaterial>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(MATERIALS_JSON).unwrap_or_default())
}

fn submesh_materials() -> &'static HashMap<String, Vec<String>> {
    static TABLE: OnceLock<HashMap<String, Vec<String>>> = OnceLock::new();
    TABLE.get_or_init(|| serde_json::from_str(MESH_SUBMATERIALS_JSON).unwrap_or_default())
}

/// Which of a mesh's submeshes to draw for a part, as a per-submesh keep mask.
///
/// The part's own material names the submesh that reads as the part (a body's
/// paint); when the tables name nothing, the largest submesh is the best guess.
fn submesh_selection(
    mesh_id: Option<u64>,
    entry: Option<&MaterialEntry>,
    groups: &[MeshGroup],
) -> Vec<bool> {
    let empty = vec![false; groups.len()];
    let dominant = || {
        let Some(best) = groups
            .iter()
            .enumerate()
            .max_by_key(|(_, g)| g.tri_count())
            .map(|(i, _)| i)
        else {
            return empty.clone();
        };
        let mut mask = empty.clone();
        mask[best] = true;
        mask
    };
    let Some(names) = mesh_id.and_then(|id| submesh_materials().get(&id.to_string())) else {
        return dominant();
    };
    if names.len() != groups.len() {
        return dominant();
    }
    match entry.and_then(|e| e._material.as_deref()) {
        Some(wanted) => {
            // Glass rides along with the part's material even though it has a
            // texture of its own: left out, the cabin is an open hole.
            let mask: Vec<bool> = names
                .iter()
                .map(|name| name == wanted || name.contains("glass") || name.contains("window"))
                .collect();
            if mask.iter().any(|k| *k) {
                mask
            } else {
                dominant()
            }
        }
        None => dominant(),
    }
}

/// The material of one submesh (they are listed in submesh order).
pub fn submesh_material(mesh_id: u64, submesh: usize) -> Option<&'static PortMaterial> {
    let names = submesh_materials().get(&mesh_id.to_string())?;
    materials().get(names.get(submesh)?)
}

/// Whether the mesh's submesh materials are usable: the import writes one group
/// per submesh, so the lists line up or not at all.
pub fn submesh_count(mesh_id: u64) -> Option<usize> {
    submesh_materials().get(&mesh_id.to_string()).map(Vec::len)
}

/// Material for one exported mesh file, found through the manifest's owner names.
///
/// The export writes a mesh once, under one of the GameObjects that use it, so
/// `Tree-A-01.glb` may be the mesh of a GameObject called `Trees` — and that is
/// the name the material table is keyed by.
pub fn manifest_material(file_rel: &str) -> Option<&'static MaterialEntry> {
    let manifest = export_manifest()?;
    let entry = manifest
        .meshes
        .iter()
        .find(|mesh| mesh.file_rel.as_deref() == Some(file_rel))?;
    for owner in &entry.owners {
        let Some(path) = owner.game_object_path.as_deref() else {
            continue;
        };
        let name = path.rsplit('/').next().unwrap_or("");
        if let Some(hit) = mesh_material_table().get(name) {
            return Some(hit);
        }
    }
    None
}

/// Material for one of a car's own GameObjects, by the object's name.
fn entry_owner(car_id: &str, go: &str) -> Option<&'static MaterialEntry> {
    let owner = normalise(go);
    if owner.is_empty() {
        return None;
    }
    material_table().get(&format!("{car_id}|{owner}"))
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
    /// Frames uploaded so far: the first few rounds are repeated on purpose.
    upload_rounds: u32,
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
        // Pages go up whenever the batch may have been rebuilt: the layer count
        // changing means it was, and the first frames cover the case where the
        // batch is built after the uploads that were meant to fill it. A page the
        // renderer never received draws untextured — the flat-colour look this
        // once produced on every road and verge.
        if self.uploaded_layers != layers || self.upload_rounds < UPLOAD_ROUNDS {
            self.uploaded_layers = layers;
            self.upload_rounds += 1;
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

    /// Scale uvs onto a page's texels.
    ///
    /// A page is one array layer of `PAGE` texels square while most exported
    /// textures are smaller and fill only a corner of that layer. The renderer
    /// samples uvs over the whole layer and never rescales them, so a mesh
    /// whose uvs run 0..1 would sample mostly the empty part of the layer: its
    /// panels came out half transparent, which read as a car with pieces
    /// missing. Scaling by the texture's share keeps every sample on texels.
    pub fn fit_page(&self, group: &mut MeshGroup) {
        let Some(texture) = self.pages.get(group.texture_page as usize) else {
            return;
        };
        let (su, sv) = (
            texture.width as f32 / PAGE as f32,
            texture.height as f32 / PAGE as f32,
        );
        if su == 1.0 && sv == 1.0 {
            return;
        }
        for uv in group.uvs.iter_mut() {
            uv[0] *= su;
            uv[1] *= sv;
        }
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

/// Tokens of a scene-object name, lowercased: `Tire FR` is `["tire", "fr"]`.
fn tokens(name: &str) -> Vec<String> {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(|t| t.to_ascii_lowercase())
        .collect()
}

/// Which wheel a part belongs to, by whole token.
///
/// Substring matching is not safe here: `Ford_F150_Raptor_View_Body_body`
/// contains `_f` (in `_Ford`) and `_r` (in `_Raptor`), which used to tag the
/// whole body as the front-right wheel — and then it was drawn at that wheel,
/// once per corner, with no body left on the car.
fn wheel_corner(name: &str, wheels: &[crate::sim::data::WheelSpec]) -> Option<usize> {
    let part = tokens(name);
    wheels.iter().position(|w| {
        let wheel = tokens(&w.name);
        // The tag is the last token of the wheel's name: "Tire FR" -> "fr".
        let Some(tag) = wheel.last() else {
            return false;
        };
        tag.len() <= 2 && part.iter().any(|t| t == tag)
    })
}

fn classify(name: &str, wheels: &[crate::sim::data::WheelSpec]) -> (PartKind, Option<usize>) {
    // Names arrive as file stems (`Cars_BMW_M3_steering_wheel`) or object names
    // (`steering wheel`); matching on one spelling missed the other.
    let lower = name.to_ascii_lowercase().replace('_', " ");
    let corner = wheel_corner(name, wheels);
    if lower.contains("collider") {
        return (PartKind::Hidden, corner);
    }
    // `W_FL` is the shipped wheel mesh for cars whose wheels are not separate
    // Tire/Whl objects: a lone `w` token is a wheel, not a wing.
    let single_w = tokens(name).first().map(String::as_str) == Some("w");
    let kind = if lower.contains("interior")
        || lower.contains("steering")
        || lower.contains("dial")
        || lower.contains("gauge")
        || lower.contains("panel")
        || lower.contains("pedal")
        || lower.contains("seat")
        || lower.contains("mirror")
    {
        PartKind::Interior
    } else if lower.contains("wheel") || lower.contains("whl") || lower.contains("tire") || single_w
    {
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
    // Only wheel-corner parts are moved to a wheel: everything else is drawn in
    // the car's own frame, and a corner on it would teleport it.
    let corner = match kind {
        PartKind::Wheel | PartKind::Caliper => corner,
        _ => None,
    };
    (kind, corner)
}

/// Load every exported mesh the car owns, binding each mesh's material texture
/// page and tint.
///
/// The export names most meshes after their GameObject, not their car, so the
/// manifest's owner paths decide what belongs to a car. Without the manifest the
/// loader falls back to the car-prefixed file names.
pub fn load_car_art(
    car_id: &str,
    wheels: &[crate::sim::data::WheelSpec],
    textures: &mut Textures,
) -> Option<CarArt> {
    let root = art_root()?;
    let mut art = CarArt::default();
    let mut files: Vec<(PathBuf, String, Option<u64>)> = Vec::new();
    for (file, go_path, mesh_id) in manifest_parts(car_id) {
        let path = root.join(&file);
        if path.is_file() {
            files.push((path, go_path, mesh_id));
        }
    }
    if files.is_empty() {
        let dir = root.join("meshes");
        let prefix = format!("{car_id}_");
        for path in std::fs::read_dir(&dir).ok()?.flatten().map(|e| e.path()) {
            let named = path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with(&prefix) && n.ends_with(".glb"))
                .unwrap_or(false);
            if named {
                files.push((path, String::new(), None));
            }
        }
        files.sort();
    }
    for (path, go_path, mesh_id) in files {
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
        let meshes: Vec<_> = meshes
            .into_iter()
            .map(|mut mesh| {
                for group in &mut mesh.groups {
                    double_sided(group);
                }
                mesh
            })
            .collect();
        let stem = name.trim_end_matches(".glb").to_string();
        // The owner path carries the tokens that name a part ("Wheels/FR/View/
        // Tire FR"), so it classifies better than the file name ever could —
        // but only its last segment: everything under `View/Body/` is car body
        // area, and a diffuser is not what the paint goes on.
        let key = if go_path.is_empty() {
            stem.clone()
        } else {
            go_path.rsplit('/').next().unwrap_or("").to_string()
        };
        let (kind, corner) = classify(&key, wheels);
        let entry = mesh_id
            .and_then(|id| car_mesh_table().get(&(car_id.to_string(), id)))
            .or_else(|| {
                let go = go_path.rsplit('/').next().unwrap_or("");
                entry_owner(car_id, go)
            })
            .or_else(|| entry_for(car_id, &stem));
        let page = entry
            .and_then(|e| e.texture.as_deref())
            .and_then(|t| textures.page(t));
        let tint = entry.and_then(|e| e.tint());
        let cutoff = entry.map(|e| e.cutoff()).unwrap_or(0.0);
        let blends = entry.map(|e| e.blends()).unwrap_or(false);
        for mesh in meshes {
            if mesh.groups.iter().all(|g| g.is_empty()) {
                continue;
            }
            // A car mesh is split per material (a body is paint, parts, glass
            // and chrome at once). Each submesh keeps its own material: the
            // part's entry names the one that reads as the part (a body's
            // paint) and the glass rides along, because left out the cabin is
            // an open hole. A submesh whose material has no texture of its own
            // is meant to be flat — glass is authored as black paint — so it is
            // drawn with its colour instead of falling back to the part's page.
            let keep = submesh_selection(mesh_id, entry, &mesh.groups);
            let groups: Vec<MeshGroup> = mesh
                .groups
                .into_iter()
                .enumerate()
                .filter(|(index, _)| keep.get(*index).copied().unwrap_or(true))
                .map(|(index, mut group)| {
                    let (bound_page, bound_tint, bound_cutoff, bound_blends) =
                        match mesh_id.and_then(|id| submesh_material(id, index)) {
                            Some(material) => (
                                material.texture.as_deref().and_then(|t| textures.page(t)),
                                material.color.or(tint),
                                material.cutoff.unwrap_or(cutoff),
                                material.transparent.unwrap_or(blends),
                            ),
                            None => (page, tint, cutoff, blends),
                        };
                    // The tint applies either way: an untextured submesh is
                    // drawn as its colour, which is how glass reads as dark
                    // glazing rather than as the white vertex colour the GLB
                    // carries.
                    if let Some(tint) = bound_tint {
                        for c in group.colors.iter_mut() {
                            *c = [c[0] * tint[0], c[1] * tint[1], c[2] * tint[2]];
                        }
                    }
                    match bound_page {
                        Some(bound_page) => {
                            // Textured: keep uvs, sample the page.
                            group.texture_page = bound_page;
                            textures.fit_page(&mut group);
                            group.alpha_cutoff = bound_cutoff;
                            group.transparent |= bound_blends;
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
                transform: part_transform(car_id, &go_path),
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
fn rotate_quat([x, y, z, w]: [f32; 4], v: Vec3) -> Vec3 {
    let (xx, yy, zz) = (x * x, y * y, z * z);
    let (xy, xz, yz) = (x * y, x * z, y * z);
    let (wx, wy, wz) = (w * x, w * y, w * z);
    vec3(
        (1.0 - 2.0 * (yy + zz)) * v.x + 2.0 * (xy - wz) * v.y + 2.0 * (xz + wy) * v.z,
        2.0 * (xy + wz) * v.x + (1.0 - 2.0 * (xx + zz)) * v.y + 2.0 * (yz - wx) * v.z,
        2.0 * (xz - wy) * v.x + 2.0 * (yz + wx) * v.y + (1.0 - 2.0 * (xx + yy)) * v.z,
    )
}

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
    // Wheels sit where the simulation puts them; everything else follows the
    // transform its object carries in the shipped Garage scene.
    let authored = match (art.kind, art.corner) {
        (PartKind::Wheel | PartKind::Caliper, Some(_)) => None,
        _ => art.transform,
    };
    let mut out = Vec::with_capacity(art.groups.len());
    for group in &art.groups {
        let mut g = group.clone();
        for p in g.positions.iter_mut() {
            let raw = Vec3::from(*p);
            let mut local = vec3(raw.x * art.mirror.0, raw.y, raw.z * art.mirror.1);
            if let Some(transform) = authored {
                local = vec3(
                    local.x * transform.scale[0],
                    local.y * transform.scale[1],
                    local.z * transform.scale[2],
                );
                local = rotate_quat(transform.rotation, local) + Vec3::from(transform.position);
            }
            local += offset;
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
        // Untextured parts carry the paint. Multiplying keeps the material colour
        // as authored: an added floor desaturated every car toward tan, which is
        // how an orange truck dissolved into the road.
        if g.uvs.is_empty() && matches!(art.kind, PartKind::Body | PartKind::Wing) {
            for c in g.colors.iter_mut() {
                *c = [
                    (c[0] * colour[0]).clamp(0.0, 1.0),
                    (c[1] * colour[1]).clamp(0.0, 1.0),
                    (c[2] * colour[2]).clamp(0.0, 1.0),
                ];
            }
        }
        out.push(g);
    }
    out
}

/// Stand-in wheels for corners the export has no mesh for.
pub fn missing_wheel_groups(sim: &CarSim, art: &CarArt) -> Vec<MeshGroup> {
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
        // Tyres are rubber, whatever paint the car wears: tinting these with the
        // livery turned a yellow car's wheels yellow, which is what "partly
        // disassembled" looked like at distance. The rim keeps the car's colour.
        let mut g = crate::scene::mesh();
        push_wheel(
            &mut g,
            centre,
            w.radius,
            w.radius * 0.55,
            [0.06, 0.06, 0.07],
        );
        // Rims are neutral on the shipped cars; carrying the livery here is what
        // made a fallback wheel read as a pale disc rather than a wheel.
        push_disc(
            &mut g,
            centre + vec3(0.0, 0.0, 0.0),
            w.radius * 0.55,
            w.radius * 0.56,
            [0.30, 0.30, 0.32],
        );
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

/// A rim: a stubby cylinder on the wheel's axis, centred where the wheel is.
fn push_disc(m: &mut MeshGroup, centre: Vec3, radius: f32, half_len: f32, colour: [f32; 3]) {
    push_wheel(m, centre, radius, half_len, colour);
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
