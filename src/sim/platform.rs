//! Platform intents, free of any SDK type so the simulation stays headless.
//! `app/platform.rs` runs them through the bridge; a break gives up after
//! its timeout so a silent portal cannot freeze the game.

use bevy_ecs::prelude::*;

/// The SDK message enum, mirrored locally: the sim stays headless and the
/// shell decides what, if anything, to forward.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformMessage {
    GameReady,
    GameplayStarted,
    GameplayStopped,
}

/// Wall-clock ceiling on one interstitial.
pub const BREAK_TIMEOUT: f32 = 10.0;
/// Wall-clock ceiling on one rewarded ad. Longer than an interstitial: the
/// verdict round trip on a slow ad is the whole point.
pub const REWARD_TIMEOUT: f32 = 30.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BreakKind {
    Interstitial,
    Rewarded,
}

impl BreakKind {
    pub fn timeout(self) -> f32 {
        match self {
            Self::Interstitial => BREAK_TIMEOUT,
            Self::Rewarded => REWARD_TIMEOUT,
        }
    }
}

/// One call the sim wants the platform to make.
#[derive(Clone, Debug, PartialEq)]
pub enum PlatformRequest {
    SendMessage(PlatformMessage),
    ShowInterstitial { placement: String },
    ShowRewarded { placement: String },
}

#[derive(Resource, Clone, Debug, Default)]
pub struct Platform {
    /// An ad is on screen: the schedule stops and input must not act.
    pub ad_in_progress: bool,
    gameplay_active: bool,
    pending: Option<BreakKind>,
    reward_granted: bool,
    requests: Vec<PlatformRequest>,
}

impl Platform {
    /// The first frame the player could interact with is on its way; only
    /// then may `game_ready` go out.
    pub fn loading_finished(&mut self) {
        self.requests
            .push(PlatformRequest::SendMessage(PlatformMessage::GameReady));
    }

    pub fn gameplay_start(&mut self) {
        if self.gameplay_active {
            return;
        }
        self.gameplay_active = true;
        self.requests.push(PlatformRequest::SendMessage(
            PlatformMessage::GameplayStarted,
        ));
    }

    pub fn gameplay_stop(&mut self) {
        if !self.gameplay_active {
            return;
        }
        self.gameplay_active = false;
        self.requests.push(PlatformRequest::SendMessage(
            PlatformMessage::GameplayStopped,
        ));
    }

    /// Start an interstitial break under `placement`, an id
    /// `playgama-bridge-config.json` maps to a native slot. Returns `false` if
    /// one is already running.
    pub fn commercial_break(&mut self, placement: &str) -> bool {
        if self.ad_in_progress {
            return false;
        }
        self.ad_in_progress = true;
        self.gameplay_stop();
        self.pending = Some(BreakKind::Interstitial);
        self.requests.push(PlatformRequest::ShowInterstitial {
            placement: placement.to_string(),
        });
        true
    }

    /// Start a rewarded break. Returns `false` only when a break is already
    /// running; whether the reward is granted is not known until the ad
    /// reports back, which is what `reward_granted` then says.
    pub fn rewarded_break(&mut self, placement: &str) -> bool {
        if self.ad_in_progress {
            return false;
        }
        self.ad_in_progress = true;
        self.reward_granted = false;
        self.gameplay_stop();
        self.pending = Some(BreakKind::Rewarded);
        self.requests.push(PlatformRequest::ShowRewarded {
            placement: placement.to_string(),
        });
        true
    }

    pub fn pending(&self) -> Option<BreakKind> {
        self.pending
    }

    /// Whether the rewarded ad that just finished paid out.
    pub fn reward_granted(&self) -> bool {
        self.reward_granted
    }

    /// The shell's report that the ad closed, failed or ran out of time.
    pub fn break_finished(&mut self) {
        self.ad_in_progress = false;
        self.pending = None;
    }

    /// The platform saw the rewarded ad grant the reward.
    pub fn note_reward_granted(&mut self) {
        self.reward_granted = true;
    }

    /// Take the calls the sim queued since the last drain.
    pub fn take_requests(&mut self) -> Vec<PlatformRequest> {
        std::mem::take(&mut self.requests)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_break_is_refused_while_one_runs() {
        let mut platform = Platform::default();
        platform.gameplay_start();
        assert!(platform.commercial_break("game_over"));
        assert!(platform.ad_in_progress);
        assert!(!platform.rewarded_break("revive"));
        platform.break_finished();
        assert!(platform.rewarded_break("revive"));
    }

    #[test]
    fn gameplay_flags_toggle_once() {
        let mut platform = Platform::default();
        platform.gameplay_start();
        platform.gameplay_start();
        platform.gameplay_stop();
        assert!(!platform.gameplay_active);
        let requests = platform.take_requests();
        assert_eq!(
            requests,
            vec![
                PlatformRequest::SendMessage(PlatformMessage::GameplayStarted),
                PlatformRequest::SendMessage(PlatformMessage::GameplayStopped),
            ]
        );
        assert!(platform.take_requests().is_empty());
    }

    #[test]
    fn a_break_stops_gameplay_and_queues_the_ad() {
        let mut platform = Platform::default();
        platform.gameplay_start();
        platform.take_requests();
        platform.commercial_break("game_over");
        assert_eq!(
            platform.take_requests(),
            vec![
                PlatformRequest::SendMessage(PlatformMessage::GameplayStopped),
                PlatformRequest::ShowInterstitial {
                    placement: "game_over".to_string(),
                },
            ]
        );
        assert_eq!(platform.pending(), Some(BreakKind::Interstitial));
    }

    #[test]
    fn a_reward_is_only_granted_once_the_ad_says_so() {
        let mut platform = Platform::default();
        assert!(platform.rewarded_break("revive"));
        assert!(!platform.reward_granted());
        platform.note_reward_granted();
        platform.break_finished();
        assert!(platform.reward_granted());
        assert_eq!(platform.pending(), None);
    }
}
