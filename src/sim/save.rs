use bevy_ecs::prelude::Resource;
use game_utils::save::SaveManager;
use game_utils::save_store::SaveStore;
use game_utils::storage::FsStorage;
use repame_save::{AppId, SaveFile};
use serde::{Deserialize, Serialize};

const APP: AppId = AppId::new("com.mlm-games.web-game");

const SAVE_FILE: &str = "save.ron";

const SCHEMA: u32 = 1;

/// The identity every template game shared before they each got one of their
/// own, and the directory it stamped its saves into. Read once, while
/// [`SAVE_FILE`] does not exist yet; never written.
const LEGACY_APP: &str = "WebGame";

const LEGACY_FILE: &str = "web_game.json";

pub const CLOUD_KEY: &str = "web_game_save";

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct SaveData {
    #[serde(default)]
    pub best_score: i32,
    #[serde(default)]
    pub muted: bool,
}

#[derive(Resource, Clone, Debug)]
pub struct SaveGame {
    pub data: SaveData,
    pub cloud_dirty: bool,
    store: SaveFile,
}

impl Default for SaveGame {
    fn default() -> Self {
        Self::new()
    }
}

impl SaveGame {
    pub fn new() -> Self {
        Self::with_store(SaveFile::new(APP, SAVE_FILE, SCHEMA))
    }

    fn with_store(store: SaveFile) -> Self {
        let mut save = Self {
            data: SaveData::default(),
            cloud_dirty: false,
            store,
        };
        match save.store.load::<SaveData>() {
            Ok(Some(loaded)) => save.data = loaded,
            Ok(None) => save.adopt_legacy(),
            Err(error) => log::warn!("web-game: {error}"),
        }
        save
    }

    /// Carry a save written before the move onto `repame-save` across: raw JSON
    /// under [`LEGACY_APP`]'s data directory. The old file is left in place, so
    /// an older build still finds it.
    fn adopt_legacy(&mut self) {
        let dir = SaveManager::new("com", "mlm-games", LEGACY_APP, LEGACY_FILE, 1).data_dir();
        let Some(bytes) = SaveStore::new(dir, LEGACY_FILE)
            .load(&SaveStore::<FsStorage>::is_intact_json, &[])
            .data
        else {
            return;
        };
        match serde_json::from_slice::<SaveData>(&bytes) {
            Ok(data) => {
                self.data = data;
                self.save_data();
            }
            Err(error) => log::warn!("web-game: {LEGACY_FILE}: {error}"),
        }
    }

    pub fn save_data(&mut self) {
        if let Err(error) = self.store.save(&self.data) {
            log::warn!("web-game: {error}");
        }
        self.cloud_dirty = true;
    }

    pub fn apply_cloud(&mut self, cloud: &serde_json::Value) -> bool {
        if !cloud.is_object() {
            return false;
        }
        let before = serde_json::to_string(&self.data).unwrap_or_default();
        if let Some(best) = cloud.get("best_score").and_then(|v| v.as_i64())
            && best > self.data.best_score as i64
        {
            self.data.best_score = best as i32;
        }
        if let Some(muted) = cloud.get("muted").and_then(|v| v.as_bool()) {
            self.data.muted = muted;
        }
        let changed = serde_json::to_string(&self.data).unwrap_or_default() != before;
        if changed {
            self.save_data();
        }
        changed
    }
}
