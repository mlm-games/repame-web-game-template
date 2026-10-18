use bevy_ecs::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlatformMessage {
    GameReady,
    GameplayStarted,
    GameplayStopped,
}

pub const BREAK_TIMEOUT: f32 = 10.0;
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

#[derive(Clone, Debug, PartialEq)]
pub enum PlatformRequest {
    SendMessage(PlatformMessage),
    ShowInterstitial {
        placement: String,
    },
    ShowRewarded {
        placement: String,
    },
    /// Post a level score to the platform leaderboard.
    SubmitScore {
        id: String,
        score: f64,
    },
    /// Open the platform's own leaderboard UI.
    ShowLeaderboard {
        id: String,
    },
    /// Buy a shop item (coin packs, "no ads").
    Purchase {
        id: String,
    },
}

#[derive(Resource, Clone, Debug, Default)]
pub struct Platform {
    pub ad_in_progress: bool,
    gameplay_active: bool,
    pending: Option<BreakKind>,
    reward_granted: bool,
    requests: Vec<PlatformRequest>,
    /// Shop item the platform reported as purchased and not yet consumed.
    purchase_granted: Option<String>,
    /// True once a leaderboard submit succeeded, cleared when read.
    score_posted: bool,
}

impl Platform {
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

    pub fn reward_granted(&self) -> bool {
        self.reward_granted
    }

    pub fn break_finished(&mut self) {
        self.ad_in_progress = false;
        self.pending = None;
    }

    pub fn note_reward_granted(&mut self) {
        self.reward_granted = true;
    }

    /// Queue a leaderboard score post.
    pub fn submit_score(&mut self, id: &str, score: f64) {
        self.requests.push(PlatformRequest::SubmitScore {
            id: id.to_string(),
            score,
        });
    }

    /// Queue the platform's leaderboard popup.
    pub fn show_leaderboard(&mut self, id: &str) {
        self.requests
            .push(PlatformRequest::ShowLeaderboard { id: id.to_string() });
    }

    /// Queue a shop purchase.
    pub fn purchase(&mut self, id: &str) {
        self.requests
            .push(PlatformRequest::Purchase { id: id.to_string() });
    }

    /// Record a purchase reported by the platform.
    pub fn note_purchase(&mut self, id: &str) {
        self.purchase_granted = Some(id.to_string());
    }

    /// Take the purchased item id, if any arrived since the last call.
    pub fn take_purchase(&mut self) -> Option<String> {
        self.purchase_granted.take()
    }

    pub fn note_score_posted(&mut self) {
        self.score_posted = true;
    }

    pub fn take_score_posted(&mut self) -> bool {
        std::mem::take(&mut self.score_posted)
    }

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
