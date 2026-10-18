pub mod app;
pub mod rigs;
pub mod sim;
pub mod ui;

pub use app::App;

#[cfg(all(not(target_arch = "wasm32"), not(target_os = "android")))]
pub fn run() {
    if let Err(error) = app::run_desktop() {
        eprintln!("web-game failed to start: {error:?}");
    }
}

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(start)]
pub fn run() {
    wasm_bindgen_futures::spawn_local(async {
        playgama_bridge::set_game_loading_progress(0.0);
        let bridge = match playgama_bridge::load(Default::default()).await {
            Ok(bridge) => Some(bridge),
            Err(error) => {
                log::warn!("web-game: {error}; running without a platform");
                None
            }
        };
        playgama_bridge::set_game_loading_progress(100.0);
        if let Err(error) = app::run_web(bridge) {
            eprintln!("web-game failed to start: {error:?}");
        }
    });
}
