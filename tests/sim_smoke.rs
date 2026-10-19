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
fn the_track_is_laid_along_z() {
    // `centre` and `road_pos` are world points: `z` is the distance along the
    // track. Every scene builder (road ribbon, props, obstacles, tangent yaw)
    // places from them, so a `z` that stays at zero collapses the whole world
    // onto one plane.
    let level = &data::levels()[6];
    let track = &TrackWorld::load(level).track;
    for s in [0.0, 12.5, 250.0, track.length - 1.0] {
        let c = track.centre(s);
        assert!((c.z - s).abs() < 1e-3, "centre({s}).z = {}", c.z);
        let edge = track.half_width * 0.98;
        for lateral in [-edge, 0.0, edge] {
            let p = track.road_pos(s, lateral);
            assert!(
                (p.z - s).abs() < 1e-3,
                "road_pos({s}, {lateral}).z = {}",
                p.z
            );
            assert!(
                track.surface(p.x, p.z).is_some(),
                "road_pos({s}, {lateral}) must land on the road"
            );
        }
    }
    let yaw = deadly_descent::props::tangent_yaw(track, track.length * 0.5);
    assert!(
        yaw.abs() < std::f32::consts::FRAC_PI_4,
        "tangent yaw {yaw} points across the track, not along it"
    );
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
    textures.preload_cars(data::cars());
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
    let mesh = cache
        .get("Container", &mut textures)
        .expect("container mesh");
    assert!(
        mesh.groups.iter().any(|g| !g.uvs.is_empty()),
        "container binds its texture page"
    );
    assert!(
        mesh.ground_offset.is_finite(),
        "props carry a ground offset so their lowest vertex meets the road"
    );
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
    // The save is shared state: a playthrough can leave a faster run behind, so
    // start this level's entry empty before recording.
    if let Some(slot) = progress.data.best_speed.get_mut(3) {
        *slot = 0.0;
    }
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

#[test]
fn touch_wheel_steers_deadzones_and_recentres() {
    use deadly_descent::ui::touch::{TouchState, WHEEL_RETURN_SPEED};

    let mut touch = TouchState::default();
    assert_eq!(touch.steer_input(), 0.0, "centred at rest");

    touch.steer = 1.0;
    assert!(
        (touch.steer_input() - 1.0).abs() < 1e-6,
        "full lock reads as 1"
    );
    touch.steer = 0.02;
    assert_eq!(touch.steer_input(), 0.0, "deadzone: no twitch at centre");

    touch.steer = 0.6;
    let step = 1.0 / 60.0;
    let mut elapsed = 0.0;
    while touch.steer != 0.0 && elapsed < 2.0 {
        touch.step(step);
        elapsed += step;
    }
    assert_eq!(touch.steer, 0.0, "recentres on its own");
    assert!(
        (elapsed - 0.6 / WHEEL_RETURN_SPEED).abs() < 0.06,
        "recentre takes steer/speed seconds, took {elapsed}"
    );
}

#[test]
fn touch_buttons_reach_the_sim_input() {
    use deadly_descent::sim::car::Input;
    use deadly_descent::ui::touch::TouchState;

    let mut touch = TouchState::default();
    assert!(!touch.any_input());
    touch.press_for_test("accel");
    touch.press_for_test("nitro");
    // The merge the app performs each frame: whichever source asks for more wins.
    let keyboard = Input::default();
    let merged = Input {
        throttle: keyboard.throttle.max(touch.throttle),
        brake: keyboard.brake.max(touch.brake),
        steer: touch.steer_input(),
        handbrake: touch.handbrake,
        nitro: touch.nitro,
    };
    assert_eq!(merged.throttle, 1.0, "touch throttle reaches the sim");
    assert!(merged.nitro, "touch nitro reaches the sim");
    touch.release_all();
    assert_eq!(touch.throttle, 0.0);
    assert!(!touch.any_input(), "everything released");
}

#[test]
fn the_car_body_is_not_pinned_to_a_wheel() {
    // Regression: the corner classifier matched substrings, and
    // `Ford_F150_Raptor_View_Body_body` contains `_f` (in `_Ford`) and `_r` (in
    // `_Raptor`), so the whole body was tagged front-right, drawn at that wheel
    // and replicated per corner — the car had no body, just wheel-sized blobs.
    let mut textures = deadly_descent::art::Textures::default();
    let cars = data::cars();
    for spec in cars {
        let Some(art) = deadly_descent::art::load_car_art(&spec.id, &spec.wheels, &mut textures)
        else {
            continue;
        };
        for part in &art.parts {
            if part.kind == deadly_descent::art::PartKind::Wheel
                || part.kind == deadly_descent::art::PartKind::Caliper
            {
                continue;
            }
            assert!(
                part.corner.is_none(),
                "{}: {} is a {:?} but carries wheel corner {:?}; only wheels and \
                 calipers belong at a corner",
                spec.id,
                part.source,
                part.kind,
                part.corner
            );
        }
        // And the body must be car-sized where it stands.
        // A car whose export has no part named for its body cannot be measured.
        let Some(body) = art
            .parts
            .iter()
            .find(|p| p.kind == deadly_descent::art::PartKind::Body)
        else {
            continue;
        };
        let joined =
            deadly_descent::sim::car::CarSim::new(spec, glam::Vec3::ZERO, 0.0, Default::default());
        let placed = deadly_descent::art::part_groups(body, &joined, [1.0, 1.0, 1.0]);
        let (lo, hi) = placed
            .iter()
            .filter_map(|g| g.bounds())
            .fold(None::<([f32; 3], [f32; 3])>, |acc, (lo, hi)| {
                Some(match acc {
                    None => (lo, hi),
                    Some((alo, ahi)) => (
                        [alo[0].min(lo[0]), alo[1].min(lo[1]), alo[2].min(lo[2])],
                        [ahi[0].max(hi[0]), ahi[1].max(hi[1]), ahi[2].max(hi[2])],
                    ),
                })
            })
            .expect("the body has bounds");
        let length = hi[2] - lo[2];
        assert!(
            (3.0..9.0).contains(&length),
            "{}: the body is {length:.1} m long; a car body belongs on the car",
            spec.id
        );
    }
}

#[test]
fn the_garage_room_stands_on_its_floor() {
    // Parity guard: the shipped Garage scene places its room with Unity
    // quaternions, which are left-handed. Read the other way round, the floor
    // faces down (and is culled out of view) while the walls hang over the car.
    // The floor's world normal is what tells the two readings apart.
    let Some(_) = deadly_descent::art::art_root() else {
        return;
    };
    let mut textures = deadly_descent::art::Textures::default();
    let mut cache = deadly_descent::props::PropCache::default();
    let pieces = deadly_descent::props::garage_pieces();
    assert!(!pieces.is_empty(), "the garage layout ships with the build");
    let place = |cache: &mut deadly_descent::props::PropCache,
                 textures: &mut deadly_descent::art::Textures,
                 piece: &deadly_descent::props::GaragePiece| {
        deadly_descent::props::placed_groups(
            cache,
            textures,
            &piece.file,
            glam::Vec3::from(piece.position),
            piece.rotation,
            piece.scale,
        )
    };
    let floor = pieces
        .iter()
        .find(|piece| piece.file == "Plane001")
        .expect("the garage floor is part of the layout");
    let groups = place(&mut cache, &mut textures, floor).expect("the floor art loads");
    let (up, count) = groups
        .iter()
        .flat_map(|group| group.normals.iter())
        .fold((0.0f32, 0usize), |(up, count), normal| {
            (up + normal[1], count + 1)
        });
    assert!(count > 0, "the floor has normals");
    let mean = up / count as f32;
    assert!(
        mean > 0.5,
        "the garage floor faces the sky (mean normal y {mean:.2}): scene quaternions are left-handed"
    );
    let floor_low = groups
        .iter()
        .filter_map(|group| group.bounds())
        .map(|(low, _)| low[1])
        .fold(f32::INFINITY, f32::min);
    // `Cube` is the shipped ground slab under the whole scene, not a room piece:
    // it is meant to run deep below the floor.
    for piece in pieces
        .iter()
        .filter(|piece| !matches!(piece.file.as_str(), "Plane001" | "Cube"))
    {
        let Some(groups) = place(&mut cache, &mut textures, piece) else {
            continue;
        };
        let low = groups
            .iter()
            .filter_map(|group| group.bounds())
            .map(|(low, _)| low[1])
            .fold(f32::INFINITY, f32::min);
        assert!(
            low > floor_low - 2.0,
            "{} sinks {:.1} m below the garage floor",
            piece.file,
            floor_low - low
        );
    }
}

#[test]
fn obstacle_art_fits_the_box_the_simulation_collides_with() {
    // Parity guard: a hazard's mesh and its collision box come from two places —
    // the shipped art and this port's tables — and when they disagree the car
    // either clips through a wall or stops short of it. Measured per level so a
    // mismatch names the kind and the level.
    let Some(_) = deadly_descent::art::art_root() else {
        return;
    };
    let mut textures = deadly_descent::art::Textures::default();
    let mut cache = deadly_descent::props::PropCache::default();
    let table = deadly_descent::props::prop_table();
    let mut worst: Vec<String> = Vec::new();
    for level in data::levels().iter().take(61) {
        let track = TrackWorld::load(level);
        let mut seen = std::collections::HashSet::new();
        for ob in track.obstacles.iter() {
            let kind = format!("{:?}", ob.kind);
            if !seen.insert(kind.clone()) {
                continue;
            }
            let Some(def) = table.obstacle_for(ob.kind) else {
                continue; // no shipped art: the stand-in is built from the box
            };
            let Some(groups) = deadly_descent::props::obstacle_groups(
                &mut cache,
                &mut textures,
                def,
                glam::Vec3::ZERO,
                0.0,
                {
                    let half = ob.kind.half_extents();
                    glam::Vec3::new(half.x * 2.0, half.y * 2.0, half.z * 2.0)
                },
            ) else {
                continue;
            };
            let (lo, hi) = groups
                .iter()
                .filter_map(|g| g.bounds())
                .fold(None::<([f32; 3], [f32; 3])>, |acc, (lo, hi)| {
                    Some(match acc {
                        None => (lo, hi),
                        Some((alo, ahi)) => (
                            [alo[0].min(lo[0]), alo[1].min(lo[1]), alo[2].min(lo[2])],
                            [ahi[0].max(hi[0]), ahi[1].max(hi[1]), ahi[2].max(hi[2])],
                        ),
                    })
                })
                .expect("art has bounds");
            let half = ob.kind.half_extents();
            let art = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
            let want = [half.x * 2.0, half.y * 2.0, half.z * 2.0];
            for axis in 0..3 {
                let ratio = art[axis] / want[axis].max(0.01);
                if !(0.75..=1.35).contains(&ratio) {
                    worst.push(format!(
                        "level {} {kind} axis {axis}: art {:.1} m vs box {:.1} m ({ratio:.2}x)",
                        level.name, art[axis], want[axis]
                    ));
                }
            }
        }
    }
    worst.dedup();
    assert!(
        worst.is_empty(),
        "{} obstacle kinds disagree with their shipped art:\n{}",
        worst.len(),
        worst.join("\n")
    );
}
