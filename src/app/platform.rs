use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Once;
use std::sync::atomic::{AtomicBool, Ordering};

use bevy_ecs::prelude::World;
use playgama_bridge::{Backend, Bridge, ErrorCode, InterstitialState, RewardedState};

use crate::sim::meta::{CLOUD_KEY, Progress};
use crate::sim::platform::{BreakKind, Platform, PlatformMessage, PlatformRequest};

pub const UNSUPPORTED_BREAK_DELAY: f32 = 0.1;

/// The single in-flight purchase the host reports back; the app checks the id it
/// asked for when it drains the slot.
const PENDING_PURCHASE_ID: &str = "purchase";

enum PlatformEvent {
    AudioEnabled(bool),
    HostPaused(bool),
    AdOpened(BreakKind),
    AdClosed(BreakKind),
    AdFailed(BreakKind),
    RewardGranted,
}

#[derive(Default)]
struct Inbox {
    events: Vec<PlatformEvent>,
    cloud: Option<serde_json::Value>,
}

enum Host {
    Offline,
    Web(Rc<Bridge>, Rc<RefCell<Inbox>>),
}

pub struct PlatformHost {
    host: Host,
    unsupported_break: f32,
    audio_enabled: bool,
    ad_open: bool,
    host_paused: bool,
    announce_ready: bool,
    /// Set on the bridge worker when a score post finishes.
    score_posted: Rc<Cell<bool>>,
    /// Set on the bridge worker when a purchase finishes.
    purchase_result: Option<Rc<Cell<bool>>>,
    cloud_retry: Rc<Cell<bool>>,
}

impl Default for PlatformHost {
    fn default() -> Self {
        Self::offline()
    }
}

impl PlatformHost {
    pub fn offline() -> Self {
        Self {
            host: Host::Offline,
            unsupported_break: 0.0,
            audio_enabled: true,
            ad_open: false,
            host_paused: false,
            announce_ready: false,
            score_posted: Rc::new(Cell::new(false)),
            purchase_result: None,
            cloud_retry: Rc::new(Cell::new(false)),
        }
    }

    pub fn web(bridge: Bridge) -> Self {
        if bridge.backend() != Backend::Web {
            return Self::offline();
        }
        let audio_enabled = bridge.platform.is_audio_enabled();
        let ad_open = bridge.advertisement.interstitial_state() == Some(InterstitialState::Opened)
            || bridge.advertisement.rewarded_state() == Some(RewardedState::Opened);
        let bridge = Rc::new(bridge);
        let inbox = Rc::new(RefCell::new(Inbox::default()));
        Self::listen(&bridge, &inbox);
        log::info!("deadly-descent: platform {}", bridge_label(&bridge));
        Self {
            host: Host::Web(bridge, inbox),
            unsupported_break: 0.0,
            audio_enabled,
            ad_open,
            host_paused: false,
            announce_ready: false,
            score_posted: Rc::new(Cell::new(false)),
            purchase_result: Some(Rc::new(Cell::new(false))),
            cloud_retry: Rc::new(Cell::new(false)),
        }
    }

    pub fn silenced(&self) -> bool {
        self.ad_open
            || !self.audio_enabled
            || self.host_paused
            || BACKGROUNDED.load(Ordering::Relaxed)
    }

    pub fn suspends_gameplay(&self) -> bool {
        self.ad_open || self.host_paused
    }

    pub fn update(&mut self, world: &mut World, dt: f32, frame_ready: bool) {
        self.apply_events(world);
        if self.score_posted.get() {
            self.score_posted.set(false);
            if let Some(mut platform) = world.get_resource_mut::<Platform>() {
                platform.note_score_posted();
            }
        }
        if self.purchase_result.as_ref().is_some_and(|r| r.get()) {
            if let Some(result) = &self.purchase_result {
                result.set(false);
            }
            if let Some(mut platform) = world.get_resource_mut::<Platform>() {
                platform.note_purchase(PENDING_PURCHASE_ID);
            }
        }
        if self.ad_open
            && world
                .get_resource::<Platform>()
                .and_then(Platform::pending)
                .is_none()
        {
            self.ad_open = false;
        }
        self.run_requests(world, frame_ready);
        if self.unsupported_break > 0.0 {
            self.unsupported_break -= dt;
            if self.unsupported_break <= 0.0 {
                self.unsupported_break = 0.0;
                world.resource_mut::<Platform>().break_finished();
            }
        }
        self.sync_cloud(world);
    }

    fn listen(bridge: &Rc<Bridge>, inbox: &Rc<RefCell<Inbox>>) {
        let push = Rc::clone(inbox);
        bridge
            .platform
            .audio_state_changed()
            .connect(move |enabled| {
                push.borrow_mut()
                    .events
                    .push(PlatformEvent::AudioEnabled(enabled));
            });
        let push = Rc::clone(inbox);
        bridge
            .platform
            .pause_state_changed()
            .connect(move |paused| {
                push.borrow_mut()
                    .events
                    .push(PlatformEvent::HostPaused(paused));
            });
        let push = Rc::clone(inbox);
        bridge
            .advertisement
            .interstitial_state_changed()
            .connect(move |state| {
                let event = match state {
                    Some(InterstitialState::Opened) => {
                        Some(PlatformEvent::AdOpened(BreakKind::Interstitial))
                    }
                    Some(InterstitialState::Closed | InterstitialState::Failed) => {
                        Some(PlatformEvent::AdClosed(BreakKind::Interstitial))
                    }
                    _ => None,
                };
                push.borrow_mut().events.extend(event);
            });
        let push = Rc::clone(inbox);
        bridge
            .advertisement
            .rewarded_state_changed()
            .connect(move |state| {
                let event = match state {
                    Some(RewardedState::Opened) => {
                        Some(PlatformEvent::AdOpened(BreakKind::Rewarded))
                    }
                    Some(RewardedState::Rewarded) => Some(PlatformEvent::RewardGranted),
                    Some(RewardedState::Closed) => {
                        Some(PlatformEvent::AdClosed(BreakKind::Rewarded))
                    }
                    Some(RewardedState::Failed) => {
                        Some(PlatformEvent::AdFailed(BreakKind::Rewarded))
                    }
                    Some(RewardedState::Loading) | None => None,
                };
                push.borrow_mut().events.extend(event);
            });
        let push = Rc::clone(inbox);
        playgama_bridge::get_json::<serde_json::Value>(
            bridge.storage.as_ref(),
            CLOUD_KEY,
            move |cloud: playgama_bridge::BridgeResult<Option<serde_json::Value>>| {
                push.borrow_mut().cloud = cloud.ok().flatten();
            },
        );
    }

    fn apply_events(&mut self, world: &mut World) {
        let Host::Web(_, inbox) = &self.host else {
            return;
        };
        let events = std::mem::take(&mut inbox.borrow_mut().events);
        // Re-read per event: one batch can close a break and open the next, so a
        // snapshot taken before the loop would judge later events wrongly.
        for event in events {
            let running = world.get_resource::<Platform>().and_then(Platform::pending);
            self.apply_event(world, running, event);
        }
    }

    fn apply_event(&mut self, world: &mut World, running: Option<BreakKind>, event: PlatformEvent) {
        match event {
            PlatformEvent::AudioEnabled(enabled) => self.audio_enabled = enabled,
            PlatformEvent::HostPaused(paused) => self.host_paused = paused,
            PlatformEvent::AdOpened(kind) if running == Some(kind) => self.ad_open = true,
            PlatformEvent::AdClosed(BreakKind::Rewarded)
                if running == Some(BreakKind::Rewarded) =>
            {
                self.ad_open = false;
            }
            PlatformEvent::AdClosed(kind) if running == Some(kind) => {
                // A rewarded ad can close without granting: clear the request so
                // the next one is accepted.
                if kind == BreakKind::Rewarded {
                    let granted = world
                        .get_resource::<Platform>()
                        .is_some_and(Platform::reward_granted);
                    if !granted {
                        world.resource_mut::<Platform>().note_reward_granted();
                    }
                }
                self.finish_break(world)
            }
            PlatformEvent::AdFailed(kind) if running == Some(kind) => self.finish_break(world),
            PlatformEvent::RewardGranted if running == Some(BreakKind::Rewarded) => {
                world.resource_mut::<Platform>().note_reward_granted();
                self.finish_break(world);
            }
            _ => {}
        }
    }

    fn finish_break(&mut self, world: &mut World) {
        self.ad_open = false;
        world.resource_mut::<Platform>().break_finished();
    }

    fn run_requests(&mut self, world: &mut World, frame_ready: bool) {
        let requests = world.resource_mut::<Platform>().take_requests();
        for request in requests {
            if matches!(
                request,
                PlatformRequest::SendMessage(PlatformMessage::GameReady)
            ) && !frame_ready
            {
                self.announce_ready = true;
                continue;
            }
            match &self.host {
                Host::Web(bridge, _) => match request {
                    PlatformRequest::SendMessage(message) => {
                        bridge.platform.send_message(wire(message));
                    }
                    PlatformRequest::ShowInterstitial { placement } => {
                        if bridge.advertisement.is_interstitial_supported() {
                            bridge.advertisement.show_interstitial(Some(&placement));
                        } else {
                            self.unsupported_break = UNSUPPORTED_BREAK_DELAY;
                        }
                    }
                    PlatformRequest::ShowRewarded { placement } => {
                        if bridge.advertisement.is_rewarded_supported() {
                            bridge.advertisement.show_rewarded(Some(&placement));
                        } else {
                            world.resource_mut::<Platform>().note_reward_granted();
                            self.unsupported_break = UNSUPPORTED_BREAK_DELAY;
                        }
                    }
                    PlatformRequest::SubmitScore { id, score } => {
                        let posted = self.score_posted.clone();
                        bridge.leaderboards.set_score(
                            id.as_str(),
                            score,
                            Box::new(move |outcome| {
                                if outcome.is_ok() {
                                    posted.set(true);
                                }
                            }),
                        );
                    }
                    PlatformRequest::ShowLeaderboard { id } => {
                        bridge
                            .leaderboards
                            .show_native_popup(id.as_str(), Box::new(|_| {}));
                    }
                    PlatformRequest::Purchase { id } => match &self.purchase_result {
                        Some(result) => {
                            let result = result.clone();
                            let id = id.clone();
                            bridge.payments.purchase(
                                id.as_str(),
                                None,
                                Box::new(move |outcome| {
                                    result.set(outcome.is_ok());
                                }),
                            );
                        }
                        None => {
                            world.resource_mut::<Platform>().note_purchase(id.as_str());
                        }
                    },
                },
                Host::Offline => match request {
                    PlatformRequest::SendMessage(_) => {}
                    PlatformRequest::ShowInterstitial { .. } => {
                        self.unsupported_break = UNSUPPORTED_BREAK_DELAY;
                    }
                    PlatformRequest::ShowRewarded { .. } => {
                        world.resource_mut::<Platform>().note_reward_granted();
                        self.unsupported_break = UNSUPPORTED_BREAK_DELAY;
                    }
                    PlatformRequest::SubmitScore { .. } => {
                        // Offline: the local save already keeps the best score.
                    }
                    PlatformRequest::ShowLeaderboard { .. } => {}
                    PlatformRequest::Purchase { id } => {
                        // No store offline: grant the item so the flow is testable.
                        world.resource_mut::<Platform>().note_purchase(id.as_str());
                    }
                },
            }
        }
        if self.announce_ready && frame_ready {
            self.announce_ready = false;
            if let Host::Web(bridge, _) = &self.host {
                bridge
                    .platform
                    .send_message(playgama_bridge::PlatformMessage::GameReady);
            }
        }
    }

    fn sync_cloud(&mut self, world: &mut World) {
        let Host::Web(bridge, inbox) = &self.host else {
            return;
        };
        let cloud = inbox.borrow_mut().cloud.take();
        if let Some(cloud) = cloud
            && let Some(mut save) = world.get_resource_mut::<Progress>()
        {
            save.adopt_cloud(&cloud);
        }
        if self.cloud_retry.get() {
            if let Some(mut save) = world.get_resource_mut::<Progress>() {
                save.cloud_dirty = true;
            }
            self.cloud_retry.set(false);
        }
        let data = {
            let Some(mut save) = world.get_resource_mut::<Progress>() else {
                return;
            };
            if !save.cloud_dirty {
                return;
            }
            save.cloud_dirty = false;
            serde_json::to_value(&save.data).unwrap_or(serde_json::Value::Null)
        };
        let retry = Rc::clone(&self.cloud_retry);
        playgama_bridge::set_json(
            bridge.storage.as_ref(),
            CLOUD_KEY,
            &data,
            move |outcome: playgama_bridge::BridgeResult<()>| {
                if outcome.is_ok() {
                    return;
                }
                let code = outcome.err().and_then(|e| e.code());
                if matches!(code, Some(ErrorCode::StorageQuotaExceeded)) {
                    log::warn!("deadly-descent: the cloud is full; local save stands");
                    return;
                }
                log::warn!("deadly-descent: the cloud refused the save");
                retry.set(true);
            },
        );
    }
}

pub fn watch_foreground() {
    use repose_platform::AppLifecycle;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        repose_platform::add_lifecycle_listener(Box::new(|state| {
            BACKGROUNDED.store(state == AppLifecycle::Background, Ordering::Relaxed);
        }));
    });
}

static BACKGROUNDED: AtomicBool = AtomicBool::new(false);

fn bridge_label(bridge: &Bridge) -> String {
    let id = bridge
        .platform
        .id()
        .unwrap_or_else(|| "unknown".to_string());
    let language = bridge
        .platform
        .language()
        .map_or_else(String::new, |language| format!(" [{language}]"));
    format!("playgama {id}{language}")
}

fn wire(message: PlatformMessage) -> playgama_bridge::PlatformMessage {
    match message {
        PlatformMessage::GameReady => playgama_bridge::PlatformMessage::GameReady,
        PlatformMessage::GameplayStarted => playgama_bridge::PlatformMessage::GameplayStarted,
        PlatformMessage::GameplayStopped => playgama_bridge::PlatformMessage::GameplayStopped,
    }
}
