//! Monotonic reconnect policy. This state machine only schedules attachment;
//! it deliberately owns no TV request, operation key, or mutation callback.
use std::time::Duration;
use tokio::time::Instant;

pub const MAX_DELAY: Duration = Duration::from_secs(3600);
pub const HEALTHY_RESET: Duration = Duration::from_secs(30);
pub const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blocked {
    Stopped,
    IncompatibleBuild,
    Revoked,
    BootstrapDenied,
    InteractionRequired,
    UnsafeMetadata,
    StopUnconfirmed,
    RecheckFailed,
    SigningRequired,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Waiting { due: Instant },
    Attaching { deadline: Instant },
    Subscribed { since: Instant },
    Blocked(Blocked),
    Exited,
}

pub struct Reconnect {
    state: State,
    delay: Duration,
}
impl Reconnect {
    pub fn new(now: Instant) -> Self {
        Self {
            state: State::Waiting { due: now },
            delay: Duration::from_secs(1),
        }
    }
    pub fn state(&self) -> State {
        self.state
    }

    /// One caller owns the loop. A second call cannot start a second attempt,
    /// and waking after sleep consumes at most one due attempt.
    pub fn begin_attempt(&mut self, now: Instant) -> bool {
        if matches!(self.state, State::Waiting { due } if now >= due) {
            self.state = State::Attaching {
                deadline: now + ATTEMPT_TIMEOUT,
            };
            true
        } else {
            false
        }
    }

    /// Call only after authenticated compatibility/readiness and the atomic
    /// initial event snapshot. Connection success alone cannot enable control.
    pub fn subscribed(&mut self, now: Instant) {
        if matches!(self.state, State::Attaching { .. }) {
            self.state = State::Subscribed { since: now };
        }
    }

    pub fn tick(&mut self, now: Instant) {
        match self.state {
            State::Subscribed { since }
                if now.saturating_duration_since(since) >= HEALTHY_RESET =>
            {
                self.delay = Duration::from_secs(1)
            }
            _ => (),
        }
    }

    /// The loop must drop/cancel the attachment future on its deadline before
    /// reporting failure. A timer tick alone never releases an in-flight slot.
    pub fn failed(&mut self, now: Instant) {
        match self.state {
            State::Attaching { .. } => (),
            State::Subscribed { since } => {
                if now.saturating_duration_since(since) >= HEALTHY_RESET {
                    self.delay = Duration::from_secs(1);
                }
            }
            _ => return,
        }
        self.state = State::Waiting {
            due: now + self.delay,
        };
        self.delay = self.delay.saturating_mul(2).min(MAX_DELAY);
    }

    pub fn block(&mut self, reason: Blocked) {
        if self.state != State::Exited {
            self.state = State::Blocked(reason);
        }
    }

    /// The caller must supply the matching explicit Resume/restart or protected
    /// credential/bootstrap resolution, never just a timer firing.
    pub fn resolved(&mut self, reason: Blocked, now: Instant) {
        if self.state == State::Blocked(reason) || matches!(self.state, State::Waiting { .. }) {
            self.state = State::Waiting { due: now };
        }
    }

    pub fn exit(&mut self) {
        self.state = State::Exited;
    }
    /// One explicit read-only check. It cannot act as Resume or build restart,
    /// and does not reset backoff or restore a prior/revoked authorization.
    pub fn recheck(&mut self, now: Instant) -> bool {
        if matches!(
            self.state,
            State::Exited
                | State::Attaching { .. }
                | State::Blocked(
                    Blocked::Stopped | Blocked::IncompatibleBuild | Blocked::SigningRequired
                )
        ) {
            return false;
        }
        self.state = State::Waiting { due: now };
        true
    }
}
