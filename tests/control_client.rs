use samsung_tv_remote::control_client::reconnect::{
    Blocked, Reconnect, State, ATTEMPT_TIMEOUT, MAX_DELAY,
};
use std::time::Duration;
use tokio::time::Instant;

#[tokio::test(start_paused = true)]
async fn immediate_attempt_then_exponential_delays_repeat_the_one_hour_cap() {
    let mut policy = Reconnect::new(Instant::now());
    assert!(policy.begin_attempt(Instant::now()));
    for seconds in [
        1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048, 3600, 3600, 3600,
    ] {
        assert!(!policy.begin_attempt(Instant::now()));
        let now = Instant::now();
        policy.failed(now);
        assert_eq!(
            policy.state(),
            State::Waiting {
                due: now + Duration::from_secs(seconds)
            }
        );
        assert!(!policy.begin_attempt(now));
        tokio::time::advance(Duration::from_secs(seconds)).await;
        assert!(policy.begin_attempt(Instant::now()));
    }
}

#[tokio::test(start_paused = true)]
async fn flapping_preserves_backoff_and_healthy_stream_resets_it() {
    let mut policy = Reconnect::new(Instant::now());
    assert!(policy.begin_attempt(Instant::now()));
    policy.failed(Instant::now());
    tokio::time::advance(Duration::from_secs(1)).await;
    assert!(policy.begin_attempt(Instant::now()));
    policy.subscribed(Instant::now());
    tokio::time::advance(Duration::from_secs(29)).await;
    policy.failed(Instant::now());
    assert_eq!(
        policy.state(),
        State::Waiting {
            due: Instant::now() + Duration::from_secs(2)
        }
    );
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(policy.begin_attempt(Instant::now()));
    policy.subscribed(Instant::now());
    tokio::time::advance(Duration::from_secs(30)).await;
    policy.failed(Instant::now());
    assert_eq!(
        policy.state(),
        State::Waiting {
            due: Instant::now() + Duration::from_secs(1)
        }
    );
}

#[tokio::test(start_paused = true)]
async fn sleep_does_not_overlap_attempts_or_create_catch_up_bursts() {
    let mut policy = Reconnect::new(Instant::now());
    assert!(policy.begin_attempt(Instant::now()));
    assert_eq!(
        policy.state(),
        State::Attaching {
            deadline: Instant::now() + ATTEMPT_TIMEOUT
        }
    );
    tokio::time::advance(MAX_DELAY * 5).await;
    policy.tick(Instant::now());
    // The attachment future must be cancelled before reporting its timeout.
    assert!(!policy.begin_attempt(Instant::now()));
    policy.failed(Instant::now());
    tokio::time::advance(MAX_DELAY * 5).await;
    assert!(policy.begin_attempt(Instant::now()));
    assert!(!policy.begin_attempt(Instant::now()));
}

#[tokio::test(start_paused = true)]
async fn blocked_states_require_the_matching_resolution_and_exit_is_terminal() {
    for reason in [
        Blocked::Stopped,
        Blocked::IncompatibleBuild,
        Blocked::Revoked,
        Blocked::BootstrapDenied,
        Blocked::InteractionRequired,
        Blocked::UnsafeMetadata,
    ] {
        let mut policy = Reconnect::new(Instant::now());
        policy.block(reason);
        tokio::time::advance(MAX_DELAY * 5).await;
        policy.tick(Instant::now());
        assert!(!policy.begin_attempt(Instant::now()));
        policy.resolved(
            if reason == Blocked::Stopped {
                Blocked::Revoked
            } else {
                Blocked::Stopped
            },
            Instant::now(),
        );
        assert!(!policy.begin_attempt(Instant::now()));
        policy.resolved(reason, Instant::now());
        assert!(policy.begin_attempt(Instant::now()));
        policy.exit();
        policy.failed(Instant::now());
        policy.subscribed(Instant::now());
        policy.block(reason);
        policy.resolved(reason, Instant::now());
        assert_eq!(policy.state(), State::Exited);
    }
}
