//! The shipped UI skin.
//!
//! The build's sprites (`iconCoin`, `speed-dial`, `nitro-dial`, `btnBasic`,
//! `Frame_ListFrame12_Yellow`, `Car_Button_*`, `PictoIcon_Lock_2`, `star`, the
//! shop coin packs) plus the car icons and map previews from `StreamingAssets`
//! are loaded from the reference export as encoded images and handed to the UI.
//! Without the export every screen falls back to flat colour blocks.

use std::collections::HashMap;
use std::path::PathBuf;

use repose_core::RenderContext;
use repose_core::prelude::ImageHandle;

use crate::art::art_root;

fn streaming_root() -> Option<PathBuf> {
    let root = art_root()?;
    let candidate = root.parent()?.join("StreamingAssets");
    candidate.is_dir().then_some(candidate)
}

#[derive(Clone, Default, Debug)]
pub struct UiImages {
    sprites: HashMap<String, ImageHandle>,
    car_icons: HashMap<String, ImageHandle>,
    map_icons: HashMap<String, ImageHandle>,
    map_previews: Vec<(i32, ImageHandle)>,
    attempted: bool,
}

impl UiImages {
    /// Bind every sprite once; later frames reuse the handles.
    pub fn ensure(&mut self, rc: &RenderContext) {
        if self.attempted {
            return;
        }
        self.attempted = true;
        let Some(root) = art_root() else {
            return;
        };
        let sprite_dir = root.join("sprites");
        if let Ok(entries) = std::fs::read_dir(&sprite_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !matches!(ext, "png" | "jpg" | "jpeg" | "webp") {
                    continue;
                }
                if let Ok(bytes) = std::fs::read(&path) {
                    let handle = rc.alloc_image_handle();
                    rc.set_image_encoded(handle, bytes, true);
                    self.sprites.insert(stem.to_string(), handle);
                }
            }
        }
        if let Some(streaming) = streaming_root() {
            let mut car_icons: HashMap<String, ImageHandle> = HashMap::new();
            let mut map_icons: HashMap<String, ImageHandle> = HashMap::new();
            for (dir, target) in [("CarIcons", &mut car_icons), ("MapIcons", &mut map_icons)] {
                let path = streaming.join(dir);
                if let Ok(entries) = std::fs::read_dir(&path) {
                    for entry in entries.flatten() {
                        let file = entry.path();
                        let Some(stem) = file.file_stem().and_then(|s| s.to_str()) else {
                            continue;
                        };
                        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("");
                        if !matches!(ext, "png" | "jpg" | "jpeg") {
                            continue;
                        }
                        if let Ok(bytes) = std::fs::read(&file) {
                            let handle = rc.alloc_image_handle();
                            rc.set_image_encoded(handle, bytes, true);
                            target.insert(stem.to_string(), handle);
                        }
                    }
                }
            }
            self.car_icons = car_icons;
            self.map_icons = map_icons;
            // Map previews: Map.<mode>.<n>.RacingTrack.png, ordered by (mode, n).
            let mut previews: Vec<(i32, i32, PathBuf)> = Vec::new();
            if let Ok(entries) = std::fs::read_dir(streaming.join("MapTrajectories")) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    let parts: Vec<&str> = stem.split('.').collect();
                    if parts.len() >= 3
                        && parts[0] == "Map"
                        && let (Ok(mode), Ok(n)) =
                            (parts[1].parse::<i32>(), parts[2].parse::<i32>())
                    {
                        previews.push((mode, n, path.clone()));
                    }
                }
            }
            previews.sort();
            for (mode, _, path) in previews {
                if let Ok(bytes) = std::fs::read(&path) {
                    let handle = rc.alloc_image_handle();
                    rc.set_image_encoded(handle, bytes, true);
                    self.map_previews.push((mode, handle));
                }
            }
        }
    }

    pub fn sprite(&self, stem: &str) -> Option<ImageHandle> {
        self.sprites.get(stem).copied()
    }

    pub fn car_icon(&self, stem: &str) -> Option<ImageHandle> {
        self.car_icons.get(stem).copied()
    }

    pub fn map_icon(&self, stem: &str) -> Option<ImageHandle> {
        self.map_icons.get(stem).copied()
    }

    /// Preview for the `ordinal`-th racing level (0-based among racing levels).
    pub fn racing_preview(&self, ordinal: usize) -> Option<ImageHandle> {
        self.map_previews.get(ordinal).map(|(_, handle)| *handle)
    }

    /// The world/mode the `ordinal`-th racing level belongs to, from its preview
    /// name (`Map.<mode>.<n>.RacingTrack.png`): the shipped grouping.
    pub fn racing_mode(&self, ordinal: usize) -> Option<i32> {
        self.map_previews.get(ordinal).map(|(mode, _)| *mode)
    }
}
