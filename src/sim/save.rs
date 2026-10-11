
use bevy_ecs::prelude::Resource;
use game_utils::save::SaveManager;
use game_utils::save_store::SaveStore;
use game_utils::storage::FsStorage;
use serde::{Deserialize, Serialize};

const APP: &str = "web_game.json";

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
    store: SaveStore<FsStorage>,
}

impl Default for SaveGame {
    fn default() -> Self {
        Self::new()
    }
}

impl SaveGame {
    pub fn new() -> Self {
        let manager = SaveManager::new("com", "mlm-games", "WebGame", APP, 1);
        let store = SaveStore::new(manager.data_dir(), APP);
        let mut data = SaveData::default();
        let loaded = store.load(&SaveStore::<FsStorage>::is_intact_json, &[]);
        if let Some(bytes) = loaded.data
            && let Ok(parsed) = serde_json::from_slice::<SaveData>(&bytes)
        {
            data = parsed;
        }
        Self {
            data,
            cloud_dirty: false,
            store,
        }
    }

    pub fn save_data(&mut self) {
        let Ok(bytes) = serde_json::to_vec(&self.data) else {
            return;
        };
        let _ = self.store.write(&bytes);
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
