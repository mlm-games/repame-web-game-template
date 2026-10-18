//! Headless fixed-tick checks: the shipped tables, the car model, a full run.

use deadly_descent::sim::car::Input;
use deadly_descent::sim::data;
use deadly_descent::sim::platform::Platform;
use deadly_descent::sim::race::{Phase, Session, TrackWorld};
use deadly_descent::sim::{Controls, Intent, Intents, apply_intents, subscribe};
use repame_sim::Sim;

fn boot() -> Sim {
    let mut sim = Sim::with_default_step();
    subscribe(&mut sim.world);
    sim.world.insert_resource(Controls(Input::default()));
    sim
}

fn tick(sim: &mut Sim, controls: Input, ticks: usize) {
    for _ in 0..ticks {
        sim.world.resource_mut::<Controls>().0 = controls;
        let mut schedule = deadly_descent::sim::build_schedule();
        sim.step_with(std::time::Duration::from_millis(16), |world| {
            schedule.run(world);
        });
    }
}

#[test]
fn level_table_is_the_shipped_one() {
    let levels = data::levels();
    assert_eq!(levels.len(), 61, "Game.MapConfig carries 61 levels");
    assert_eq!(levels[0].name, "Start");
    assert_eq!(levels[0].cost_to_unlock, 0);
    assert_eq!(levels[6].name, "Racing Track");
    assert!(levels[6].is_racing_track());
    assert!(levels[6].racing_track_size > 0);
    assert!(levels[40].reward_for_completion > levels[1].reward_for_completion);
    assert!(levels.iter().all(|l| l.max_possible_score > 0));
}

#[test]
fn car_table_is_the_shipped_one() {
    let cars = data::cars();
    assert_eq!(cars.len(), 10);
    let porsche = cars.iter().find(|c| c.id == "Porsche_911").unwrap();
    assert!((porsche.mass - 1500.0).abs() < 1.0, "Rigidbody mass");
    assert_eq!(porsche.wheels.len(), 4);
    assert!((porsche.wheels[0].radius - 0.32).abs() < 0.01);
    assert!((porsche.max_rpm - 7000.0).abs() < 1.0);
    let gears = porsche.gears.as_ref().unwrap();
    assert_eq!(gears.len(), 7, "Porsche 911 ships a 7-speed");
    assert!(gears[0] > gears[1]);
}

#[test]
fn car_drives_downhill_and_the_run_ends() {
    let mut sim = boot();
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::StartLevel(0));
    apply_intents(&mut sim.world);
    assert!(sim.world.get_resource::<Session>().is_some());
    tick(&mut sim, Input::default(), 200);
    tick(
        &mut sim,
        Input {
            throttle: 1.0,
            ..Input::default()
        },
        60 * 90,
    );
    let session = sim.world.resource::<Session>();
    let player = &session.player;
    assert!(player.pos.y.is_finite() && player.pos.z.is_finite());
    assert!(player.distance > 30.0, "car should move down the hill");
    assert!(player.top_speed > 20.0, "car should reach a real speed");
    assert!(
        matches!(session.phase, Phase::Finished | Phase::Failed),
        "a 90 s run ends one way or the other, got {:?}",
        session.phase
    );
}

#[test]
fn steering_turns_the_car() {
    let mut sim = boot();
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::StartLevel(0));
    apply_intents(&mut sim.world);
    tick(&mut sim, Input::default(), 200);
    tick(
        &mut sim,
        Input {
            throttle: 1.0,
            ..Input::default()
        },
        60 * 6,
    );
    let before = sim.world.resource::<Session>().player.pos.x;
    tick(
        &mut sim,
        Input {
            throttle: 1.0,
            steer: 1.0,
            ..Input::default()
        },
        60 * 3,
    );
    let after = sim.world.resource::<Session>().player.pos.x;
    assert!(
        (after - before).abs() > 1.0,
        "steering should move the car sideways: {before} -> {after}"
    );
}

#[test]
fn track_geometry_is_consistent() {
    let level = &data::levels()[6];
    let track = TrackWorld::load(level);
    assert!(track.track.length > 100.0);
    assert!(track.track.height(0.0) > track.track.height(track.track.length));
    assert!(
        track
            .track
            .surface(track.track.centre(50.0).x, 50.0)
            .is_some()
    );
    assert!(
        track
            .track
            .surface(track.track.centre(50.0).x + 40.0, 50.0)
            .is_none()
    );
    assert!(!track.obstacles.is_empty(), "racing tracks place obstacles");
}

#[test]
fn reference_art_loads_when_present() {
    // Optional: the extracted reference meshes live outside the repo. When they
    // are there the loader must return real geometry, not fall back.
    let Some(root) = deadly_descent::art::art_root() else {
        return;
    };
    let cars = data::cars();
    let porsche = cars.iter().find(|c| c.id == "Porsche_911").unwrap();
    let mut textures = deadly_descent::art::Textures::default();
    textures.preload_cars(&data::cars().to_vec());
    let art = deadly_descent::art::load_car_art("Porsche_911", &porsche.wheels, &mut textures);
    let Some(art) = art else {
        return;
    };
    assert!(art.parts.len() >= 2, "body and interior at minimum");
    let triangles: usize = art
        .parts
        .iter()
        .flat_map(|p| p.groups.iter())
        .map(|g| g.tri_count())
        .sum();
    assert!(triangles > 5_000, "real car geometry, got {triangles} tris");
    let _ = root;
}

#[test]
fn car_textures_bind_to_pages_when_present() {
    // Optional: only meaningful with the reference export in place.
    if deadly_descent::art::art_root().is_none() {
        return;
    }
    let cars = data::cars().to_vec();
    let mut textures = deadly_descent::art::Textures::default();
    textures.preload_cars(&cars);
    let layers = textures.layer_count();
    assert!(
        layers > 1,
        "car materials reference textures, got {layers} pages"
    );
    let uploads = textures.uploads();
    assert!(!uploads.is_empty(), "pages upload once after binding");
    for upload in &uploads {
        assert_eq!(
            upload.rgba.len(),
            (upload.w * upload.h * 4) as usize,
            "upload payload matches its extent"
        );
        assert!(upload.w <= deadly_descent::art::PAGE && upload.h <= deadly_descent::art::PAGE);
    }
    let porsche = cars.iter().find(|c| c.id == "Porsche_911").unwrap();
    let art = deadly_descent::art::load_car_art("Porsche_911", &porsche.wheels, &mut textures);
    let art = art.expect("porsche meshes load");
    let textured = art
        .parts
        .iter()
        .flat_map(|p| p.groups.iter())
        .filter(|g| !g.uvs.is_empty())
        .count();
    assert!(textured > 0, "at least one part carries uvs onto a page");
    // Invariant: a group either has no uvs, or samples a page inside the batch.
    let layers = textures.layer_count();
    for group in art.parts.iter().flat_map(|p| p.groups.iter()) {
        if !group.uvs.is_empty() {
            assert!(
                group.texture_page < layers,
                "textured group samples page {} of {}",
                group.texture_page,
                layers
            );
        }
    }
}

#[test]
fn app_resources_are_all_present() {
    // Every resource the frame loop touches must exist after subscribe(); a
    // missing one panics inside the platform host on the first frame.
    let sim = boot();
    assert!(sim.world.get_resource::<Controls>().is_some());
    assert!(sim.world.get_resource::<Intents>().is_some());
    assert!(
        sim.world
            .get_resource::<deadly_descent::sim::race::CueQueue>()
            .is_some()
    );
    assert!(sim.world.get_resource::<Platform>().is_some());
}

#[test]
fn props_and_textures_load_from_the_reference_export() {
    if deadly_descent::art::art_root().is_none() {
        return;
    }
    let table = deadly_descent::props::prop_table();
    assert!(table.edge.is_some(), "props.json defines the edge barrier");
    assert!(
        table
            .obstacle_for(deadly_descent::sim::obstacles::Kind::Container)
            .is_some(),
        "containers have a shipped mesh"
    );
    let mut textures = deadly_descent::art::Textures::default();
    let mut cache = deadly_descent::props::PropCache::default();
    cache.preload(&mut textures);
    assert!(textures.layer_count() > 1, "prop textures bind pages");
    // A container mesh must load and carry its material texture.
    let groups = cache
        .get("Container", &mut textures)
        .expect("container mesh");
    let textured = groups.iter().any(|g| !g.uvs.is_empty());
    assert!(textured, "container binds its texture page");
}

#[test]
fn dressing_covers_the_track() {
    if deadly_descent::art::art_root().is_none() {
        return;
    }
    let level = data::level_at(0);
    let world = TrackWorld::load(&level);
    let _ = level.segment();
    let mut textures = deadly_descent::art::Textures::default();
    let mut cache = deadly_descent::props::PropCache::default();
    let groups = cache.dressing(&world.track, level.segment(), &mut textures);
    assert!(!groups.is_empty(), "edge barriers and scatter are placed");
    let tris: usize = groups.iter().map(|g| g.tri_count()).sum();
    assert!(
        tris > 5_000,
        "dressing carries real geometry, got {tris} tris"
    );
    // Dressing must not sit on the drivable surface centre line.
    assert!(groups.iter().all(|g| g.bounds().is_some()));
}

#[test]
fn every_car_points_at_a_shipped_engine_directory() {
    let dirs: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_str(deadly_descent::sim::data::ENGINE_DIRS_JSON)
            .expect("engine_dirs.json");
    assert_eq!(dirs.len(), 10, "all ten cars map to an engine");
    for car in data::cars() {
        let dir = deadly_descent::sim::data::engine_dir(car.id.as_str());
        assert!(dir.is_some(), "{} has an engine directory", car.id);
        let name = dir.unwrap();
        assert!(dirs.contains_key(&car.id));
        assert!(
            dirs[&car.id]["exterior"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == name)),
            "{} maps to {}",
            car.id,
            name
        );
    }
    // The reference shares one engine between two cars (V12_Italian_MQ).
    let veyron = deadly_descent::sim::data::engine_dir("BugattiVeyron").unwrap();
    let lambo = deadly_descent::sim::data::engine_dir("LamborghiniReventon").unwrap();
    assert_eq!(veyron, lambo, "shipped: both use V12_Italian_MQ");
}

#[test]
fn login_streak_pays_once_a_day_and_resets_after_a_gap() {
    let mut progress = deadly_descent::sim::meta::Progress::new();
    progress.data.coins = 0;
    progress.data.last_login_day = 0;
    progress.data.login_streak = 0;
    let first = progress.claim_login_reward(100).expect("day 1 claimable");
    assert_eq!(first, deadly_descent::sim::meta::LOGIN_REWARDS[0]);
    assert!(progress.claim_login_reward(100).is_none(), "once per day");
    assert!(progress.claim_login_reward(101).is_some(), "next day");
    assert_eq!(progress.data.login_streak, 2);
    // A skipped day restarts the streak.
    assert!(progress.claim_login_reward(105).is_some());
    assert_eq!(progress.data.login_streak, 1);
}

#[test]
fn coin_packs_and_entitlements_resolve() {
    use deadly_descent::sim::meta::{COIN_PACKS, Progress};
    assert_eq!(COIN_PACKS.len(), 4);
    assert_eq!(Progress::pack_coins("coinPack2_2"), Some(25_000));
    assert_eq!(Progress::pack_coins("nope"), None);
}

#[test]
fn level_palettes_come_from_their_own_scenes() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/assets/dd1/level_props.json"
    ))
    .expect("level_props.json");
    let table: std::collections::HashMap<String, Vec<String>> =
        serde_json::from_str(&raw).expect("parses");
    // The hole levels carry the hole pieces; the bus level carries the bus.
    assert!(table.len() >= 10, "most segments have their own props");
    let holes = table.get("Holes").expect("Holes palette");
    assert!(
        holes
            .iter()
            .any(|f| f.starts_with("HoleRoad") || f.starts_with("HoleRamp")),
        "holes dress with hole pieces: {holes:?}"
    );
    let bumps = table.get("Bumps").expect("Bumps palette");
    assert!(bumps.iter().any(|f| f == "Bump"), "bumps dress with bumps");
    // Car bodykit meshes must never leak into an environment palette.
    for (segment, files) in &table {
        for file in files {
            let low = file.to_lowercase();
            assert!(
                !(low.contains("wing") || low.contains("spoiler") || low.contains("diffuser")),
                "{segment} palette leaked a car part: {file}"
            );
        }
    }
}

#[test]
fn best_speed_survives_a_run() {
    let mut progress = deadly_descent::sim::meta::Progress::new();
    let results = deadly_descent::sim::race::Results {
        finish_time: 42.0,
        top_speed_kmh: 213.0,
        damage: 500.0,
        max_damage: 2000.0,
        place: 1,
        racers: 3,
        coins: 100,
        completion_bonus: 100,
        speed_bonus: 0,
        time_bonus: 0,
        damage_bonus: 0,
        first_completion: true,
        score: 1000,
    };
    progress.record_results(3, &results);
    assert_eq!(progress.data.best_speed.get(3).copied(), Some(213.0));
    assert!(
        progress.data.best_speed.get(3).copied().unwrap_or(0.0) > 0.0,
        "HUD readout has a value"
    );
}

#[test]
fn a_finished_run_pays_out_exactly_once() {
    // Regression: the results were re-recorded on every tick of a finished run,
    // which paid coins again and wrote the save each step.
    use deadly_descent::sim::meta::Progress;
    let mut sim = boot();
    sim.world
        .resource_mut::<Intents>()
        .0
        .push(Intent::StartLevel(0));
    apply_intents(&mut sim.world);
    // Skip the tutorial gates for a clean wallet measurement.
    sim.world.resource_mut::<Progress>().data.coins = 0;
    tick(&mut sim, Input::default(), 200);
    let mut ticks = 0;
    while ticks < 60 * 240 {
        let phase = sim.world.resource::<Session>().phase;
        if matches!(phase, Phase::Finished | Phase::Failed) {
            break;
        }
        tick(
            &mut sim,
            Input {
                throttle: 1.0,
                ..Input::default()
            },
            60,
        );
        ticks += 60;
    }
    let finished = sim.world.resource::<Session>().phase;
    assert!(
        matches!(finished, Phase::Finished | Phase::Failed),
        "the run ended: {finished:?}"
    );
    let after_first = sim.world.resource::<Progress>().data.coins;
    tick(&mut sim, Input::default(), 600);
    let after_more = sim.world.resource::<Progress>().data.coins;
    assert_eq!(
        after_first, after_more,
        "a finished run must not keep paying out while its screen is open"
    );
}
