# Deadly Descent

A downhill survival racer ported to Repame. Drive a car down a mountain track,
survive the obstacles, reach the finish in one piece.

## Provenance

The reference is the shipped **Unity 2022.3.62f2 WebGL (IL2CPP)** build of Deadly
Descent (`github.com/ubg42/DeadlyDescent`, checkout `3954a35`, data file retrieved
from the Playgama CDN on 2026-10-18). What was read, and how, is recorded in
`refs/deadly-descent.toml` and the ledger
`~/Documents/Repos/assets/deadly-descent/REA-findings.md`; the analysis scripts and
tables live in `refs/DeadlyDescent/analysis/`.

In short: the IL2CPP metadata parses fully (v31; 8,274 types, 53,624 methods, 36,331
fields, all tables tiling exactly), and the serialized assets decode with a
reconstructed MonoBehaviour layout. That yielded the game's own tables — the 61-level
progression, the ten cars' physics and gearboxes, the damage and breakable constants,
the AI config and the segment inventory.

**Not recovered**: method bodies (no wasm decompiler is available here), so Unity's
WheelCollider integration and the track generator's placement algorithm are rewritten
rather than copied, and the coin prices for cars and upgrades do not appear in any
decoded asset. Those are the DESIGNED rows below.

### Art

Art is the original game's, exported to
`~/Documents/refs/DeadlyDescent/extracted/` (571 GLBs, 331 PNGs, audio, sprites) and
loaded at runtime. It is reference material and is **never committed or
redistributed**: set `DD_ASSETS` to that directory (or leave it in place) and the cars
and props appear; without it every actor falls back to a boxed stand-in, and the game
still runs. Textures are exported but not yet bound to the GLB materials.

## Run

```sh
cargo run                 # desktop (1280x720)
cargo test                # headless sim checks: tables, car model, a 90 s run
DD_ASSETS=~/Documents/refs/DeadlyDescent/extracted cargo run
```

Controls: **W/A/S/D** or arrows to drive, **Shift/N** nitro, **Space** handbrake,
**C** camera view (chase / cockpit), **Esc** pause, **R** restart, **T** (pause menu)
toggles the on-screen touch pad. Garage → map select → race → results.

### Playtests and captures

```sh
# Drive a level with the bot controller and print a once-a-second trace.
DD_AUTOPLAY=6 cargo run            # DD_AUTOPLAY_TRACE=0 silences the trace

# Headless frames (the whole game frame: 3D scene plus UI) straight to PNGs.
# Same compose path as the window, fixed frame time, no window and no display.
DD_CAPTURE=/tmp/dd-frames DD_AUTOPLAY=6 DD_CAPTURE_FRAMES=300 cargo run
DD_CAPTURE_FPS=30 DD_CAPTURE_W=960 DD_CAPTURE_H=540     # 30 fps, 960x540
DD_DEBUG_CAM=bird                                        # whole-layout camera
ffmpeg -framerate 30 -i /tmp/dd-frames/frame_%04d.png -pix_fmt yuv420p run.mp4

# The same thing against the live window, when a display is in use.
scripts/playtest-capture.sh 6 20 2 /tmp/dd-playtest
```

Isolation aids, all environment flags: `DD_TRACE_DRAW=1` names every car part
drawn, `DD_TRACE_CAMERA=1` prints the camera and car transforms,
`DD_TRACE_SCENE=1` prints group counts, triangle counts and world bounds,
`DD_TRACE_PROPS=1` prints each prop mesh as it loads, `DD_DEBUG_CUBE=1` renders a
single box at the camera target, `DD_DEBUG_KEEP=textured|untextured` and
`DD_DEBUG_FIRST=<n>` render one slice of the scene, `DD_DEBUG_CAM=bird|side`
picks a diagnostic camera, and `DD_HIDE_CAR_PARTS=1` / `DD_HIDE_RIDGES=1` /
`DD_HIDE_DECK=1` hide a layer.

`cargo run` is desktop-only by default; web (`trunk build --release`) and Android
(`cargo rapk`) are not built or verified in this pass.

## Parity matrix

Basis: **MEASURED** = read from the shipped build (value or table); **STRUCTURE** =
shape read off the build's metadata/names; **DESIGNED** = not recoverable, chosen
here and flagged; **VERIFIED** = covered by a test in `tests/`.

| Behaviour | Reference | Port | Basis |
| --- | --- | --- | --- |
| 61 levels, names, scenes, icons, unlock costs | `Game.MapConfig` entry list | `assets/dd1/levels.json` → `sim::data::Level` | MEASURED + VERIFIED |
| Rewards per level (completion / speed / time / damage) and thresholds | `rewardForCompletion`, `speedToGetReward` (km/h), `timeToGetReward` (s), `rewardForMaxDamage` | `race::build_results` | MEASURED |
| Score ceiling and completion score | `maxPossibleScore`, `scoreForCompletion` | `race::build_results` | MEASURED for the ceiling; speed/time score split DESIGNED |
| Second completion pays a reduced reward | `secondCompletionRewardMultiplier` (1.0 in the shipped table) | `meta::record_results` (half base + score delta) | DESIGNED (field is 1.0 in every level) |
| Ten cars with their names, order and shop kind | 10 car prefabs; `CarShopItemConfig` purchase kinds | `assets/dd1/cars.json` | MEASURED + VERIFIED |
| Car mass and drag | `Rigidbody` per prefab (1200–2788 kg) | `sim::data::Car` | MEASURED + VERIFIED |
| Wheel radius, suspension travel, spring, damper, target | four `WheelCollider`s per prefab | `sim::car` suspension | MEASURED |
| Tyre friction (forward/sideways slip, value, asymptote, stiffness) | `WheelCollider` curves | `data::friction_at` | MEASURED |
| Engine: torque curve, rpm limits, cut-off pair and time, nitro fields, shift rpms | inline `EngineConfig` in each car | `sim::car::CarSim::step` | MEASURED (curve and scalars) |
| Gearbox ratios and final drive per car | inline gearbox config | `Car.ratio`, `final_drive` | MEASURED + VERIFIED |
| Steering limit falls with speed (full lock low, ~20% at 40 m/s) | `SteerConfig.SteerLimitCurve` shape | `steer_authority` | MEASURED shape |
| ABS present, TCS absent | `SteerConfig.ABS` = 1.0, `TCS` = 0.0 in the car's inline steer config | `car::ABS_ENABLED`, brake modulation past the friction peak | MEASURED flags (Porsche payload); modulation DESIGNED |
| Damage accumulates to MaxDamage 2000, then the car is destroyed | `PG.VehicleDamageController` | `CarSim::max_damage`, `resolve_collisions` | MEASURED constants, DESIGNED accumulation |
| Impulse thresholds: breakables break at 10 (later levels 12), radius 5.0, multiplier 1.2 | `Game.Breakable` | `sim::obstacles` | MEASURED + VERIFIED (structure) |
| Obstacle kinds and their levels | 22 segment scenes + `Game.Breakable` pieces | `obstacles::mix_for` | STRUCTURE (kinds) |
| Per-level dressing from the level's own scene | segment scene contents (`SingleSpike`, `HoleSpikes`, `SchoolBus`, `molot`, `Bump`, `HoleRoad`, `Tube`, `Pillar`, buildings…) | `assets/dd1/level_props.json` → `props::dressing` | MEASURED (scene contents), placement DESIGNED |
| Best-speed readout beside the target speed | the HUD's own `cur/max km/h` and `BestSpeed` labels | `meta::best_speed`, HUD row | MEASURED shape, storage DESIGNED |
| Steering limit: 40° with limiting enabled on every car | each car's inline `SteerConfig` (`MaxSteerAngle`, `EnableSteerLimit`) | `car::steer_authority` + spec | MEASURED (all ten cars) |
| Obstacle placement within a level | `Track_Generator` + seeds | `obstacles::place` | DESIGNED |
| Track length, drop and road shape per level | `racingTrackSize`, `racingTrackSeed` | `track::Track::build` | DESIGNED (fields MEASURED, geometry designed) |
| Checkpoints required on flagged levels | `requireCheckpoints` | `race::step_world` | MEASURED |
| AI opponents per level, their cars and skill tiers, bot speed cap | `botConfigs`, `maxBotSpeed` | `race::ai_controls` | MEASURED inputs, DESIGNED controller |
| AI driver tuning: profile caps, target/turn-prediction offsets, look angle, obstacle hit distance and offsets, hit delays, reverse recovery | two `RaceAIConfigAsset`s (BaseAIConfig + RaceAIConfig, 20 fields) | `assets/dd1/ai.json` → `sim::ai` | MEASURED values, controller DESIGNED (PID shape from `RaceAIControl.P/I/D`) |
| Map previews (40 `MapTrajectories` + 24 `MapIcons`) and car icons | shipped PNG/JPGs | `ui::images`, cards show the real preview per level | MEASURED art, grid grouping DESIGNED |
| World grouping of the racing levels | `Map.<mode>.<n>.RacingTrack.png` preview names | `ui::images::racing_mode` → "WORLD n" on the selector | MEASURED from the shipped preview names |
| Coins, wallet, unlock fees paid from coins | `costToUnlock` | `meta::Progress` | MEASURED costs |
| Car coin prices | not in any decoded asset | `meta::car_price` | DESIGNED |
| Upgrade levels and prices | not in any decoded asset | `meta::upgrade_price`, `Upgrades` | DESIGNED |
| Save: coins, unlocks, upgrades, best time/score | — | `meta::SaveData` (RON, cloud key) | DESIGNED |
| Ads / IAP hooks | `MultiplatformKit` + Playgama bridge config; the shop's coin packs and the "Get Coins" row | `app::platform` + rewarded-ad coin grant (`free_coins`) | PARTIAL (rewarded wired, interstitial/IAP not) |
| Daily login reward with a streak | `LogInRewardTracker`, `Game.RewardsController` | `meta::login_reward_available` / `claim_login_reward` (7-day table) | STRUCTURE (streak); amounts DESIGNED |
| Coin packs and the ad-free entitlement | `coinPack0_2`…`coinPack3_2`, `no_ads` in the bridge config; `Game.NoAdsBtn` | `meta::COIN_PACKS`, `app::purchase`, `ui` garage rows | STRUCTURE (ids MEASURED, coin amounts DESIGNED) |
| Between-runs interstitial | CrazyGames copy: "each round offers new obstacles" flow; bridge config `advertisement.interstitial` | `app::handle_run_end` (`game_over` placement, skipped once ads are removed) | STRUCTURE |
| Leaderboard submit and popup | `Game.LeaderboardUI`, `Playgama.Modules.Leaderboards`, literal "Max score is {0} at the leaderboard" | `Platform::submit_score` / `show_leaderboard`, results button | STRUCTURE (per-level board `level_N`, DESIGNED id scheme) |
| Tutorial steps | `Game.Scripts.Tutorial.*` (`ClickOnLevelTutorialStep`, `ClickOnlyOneButtonStep`, `NitroTutorial`, `PulsationAnimation`) | `meta::advance_tutorial` + pulsing hints in the garage, selector and HUD | STRUCTURE (three shipped steps, pulse animation) |
| Engine audio: per-car engine directory, rpm-banded beds with on/off throttle variants and a wind bed | 42 `StreamingAssets/CarEngines/*` sets; per-car loader mapping read off the car prefabs | `data::engine_dir`, `app::audio` (Vorbis decode, crossfaded by rpm/throttle) | MEASURED mapping; crossfade DESIGNED |
| UI skin: coin icon, speed dial with needle, nitro dial, car cards, lock/star, buttons, lose panel | 171 UI sprites in the build (`iconCoin`, `speed-dial`, `S_TachometerArrow`, `nitro-dial`, `Car_Button_*`, `PictoIcon_Lock_2`, `YouLoseBack`, …) | `ui::images` + `ui::mod` | MEASURED skin |
| UI text and readouts | the build's own strings ("Select Location", "Earned <coin> N", "Total reward", "Try again!", "NITRO", "BestSpeed", "Continue", "Back") and the `cur/max km/h` shape | `ui::mod` | MEASURED (strings read out of the scenes) |
| Obstacle art and roadside dressing | shipped meshes: `Container`, `Wooden_Crate_02`, `HoleSpikes_2`, `Ramp`, `Industrial_Train_Dr14`, `SchoolBus_2`, `Barrier-02`, `Tree-A-01`, grandstands, footbridges, terrain ridges | `assets/dd1/props.json` → `props::PropCache` | MEASURED art, placement DESIGNED |
| Car and prop textures | `materials.json` + exported PNGs (`mesh_materials.json` records mesh → material → base texture) | `art::Textures` page array | MEASURED binding |
| UI text and localisation | `LeanLocalization`, 169 phrases | English strings inline | PARTIAL (English only, strings from the build where decoded) |
| Touch controls: a draggable steering wheel that returns to centre plus accel/brake/handbrake/nitro buttons | `PG.UI.SteerWheelControls` / `SteerWheelUI` (`MaxSteerWheelAngle`, `SteerWheelToDefaultSpeed`, `WheelIsPressed`, `PrevTouchPos`) and its button set; `MobileButtons_*` / `teerWheel` art | `ui::touch` (pointer events, so a mouse drives it too), merged with the keyboard each frame in `app::feed_input` | MEASURED structure and art; thresholds DESIGNED |
| Art | exported meshes + textures | GLB load, texture pages, boxed fallback when the export is absent | MEASURED art (car liveries, props, UI) |

## Layout

```
src/
  sim/
    data.rs        level and car tables (embedded JSON, from the build)
    car.rs         four-wheel vehicle model, engine, damage accumulation
    track.rs       descending road geometry shared by sim and view
    obstacles.rs   hazard kinds, per-level mix, moving hazards, break rules
    race.rs        session lifecycle, AI opponents, checkpoints, rewards
    meta.rs        wallet, unlocks, upgrades, save file
    mod.rs         intents, resources, schedule
  art.rs           original meshes -> MeshGroups, stand-in fallback
  scene.rs         Frame3d composition (road, obstacles, cars, camera)
  ui/              garage, map select, HUD, results (Repose views)
  app/             input, platform host, audio, the frame loop
assets/dd1/        tables extracted from the build (+ segment layouts)
tests/sim_smoke.rs headless checks
```

## Next

1. Racing-track levels from the generator's piece catalogue: the shipped racing
   tracks are grids of `GRID-R-*` / `R-*` / `C-*` / `DC-*` pieces (1.8 × 5 m tiles and
   4.45 m connectors, imported at `scale = 0.01` from `StreamingAssets/glb`). The port
   still drives a designed descending profile with the shipped art dressing it.
2. Shop and upgrade coin prices (`Game.PricesConfig` carries only `CarPrices`; the
   price curve is in code, so it is not recoverable here — measure it from the live
   build or keep the designed curve).
3. Touch controls: the shipped `MobileButtons_*` / `MobileInput_*` / `teerWheel`
   sprites and the mobile UI scenes are exported; the port is keyboard only (no
   mobile target has been built, so this is deliberately last).
4. Per-car steer scalars beyond ABS/TCS (`MaxSteerAngle`, drift limits) as the
   inline configs are only partly mapped.
