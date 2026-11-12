//! Authenticate the protected peer, prove the actual HTTP listener, then redeem
//! a reusable credential. An occupied port never sees that reusable bearer.
use super::http::{request, ClientError, Endpoint, HttpClient};
use crate::server::bootstrap::*;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::UnixStream;
use tokio::time::timeout;

pub async fn authenticate(
    path: &Path,
    profile: &str,
    endpoint: &str,
    role: Role,
    expected_build: Option<&str>,
    verifier: Arc<dyn PeerVerifier>,
) -> Result<HttpClient, BootstrapError> {
    timeout(Duration::from_secs(5), async {
        socket::validate(path)?;
        let mut channel = UnixStream::connect(path)
            .await
            .map_err(|_| BootstrapError::Unavailable)?;
        verifier.verify(&channel, role)?;
        let nonce = hex(&random()?);
        write_frame(
            &mut channel,
            &ClientFrame::Hello(Hello {
                version: 1,
                role,
                nonce: nonce.clone(),
                profile: profile.into(),
            }),
        )
        .await?;
        let challenge = match read_frame(&mut channel, Duration::from_secs(5)).await? {
            ServerFrame::Challenge(c) => c,
            ServerFrame::Error(e) => return Err(e),
            _ => return Err(BootstrapError::InvalidFrame),
        };
        if challenge.version != 1
            || challenge.role != role
            || challenge.nonce != nonce
            || challenge.context.profile != profile
            || challenge.context.endpoint != endpoint
            || challenge.context.api != "v1"
        {
            return Err(BootstrapError::UnsafeMetadata);
        }
        crate::server::control_http::wire::parse_epoch(&challenge.context.epoch)
            .map_err(|_| BootstrapError::InvalidFrame)?;
        unhex(&challenge.probe_bearer)?;
        let endpoint = Endpoint::parse(endpoint).map_err(|_| BootstrapError::UnsafeMetadata)?;
        let (status, ready) = request(
            endpoint,
            &challenge.probe_bearer,
            "GET",
            "/v1/ready",
            None,
            Some(&challenge.challenge),
        )
        .await
        .map_err(|error| match error {
            ClientError::Transport => BootstrapError::Unavailable,
            _ => BootstrapError::UntrustedListener,
        })?;
        if status == 401 {
            // Rotation may invalidate this disposable grant or close its
            // connection. Retry with a new protected challenge; revocation is
            // then denied by that peer. Never redeem this rejected grant.
            return Err(BootstrapError::Expired);
        }
        if matches!(status, 429 | 503) {
            // A bounded readiness/query pool may be busy. Withhold the API
            // credential and retry attachment instead of declaring a valid
            // protected peer permanently unsafe under transient load.
            return Err(BootstrapError::Unavailable);
        }
        let expected = proof(&challenge)?;
        if status != 200
            || !ready["bootstrap_proof"]
                .as_str()
                .is_some_and(|v| equal(v, &expected))
            || ready["server_epoch"] != challenge.context.epoch
            || ready["build"] != challenge.context.build
            || ready["api_version"] != challenge.context.api
        {
            return Err(BootstrapError::UntrustedListener);
        }
        // A proved listener may be quiescing after a signal. That is a
        // transient lifecycle condition, not an untrusted peer or a reason to
        // permanently block automatic reconnect. Never redeem while fenced.
        match ready["serving"].as_bool() {
            Some(true) => (),
            Some(false) => return Err(BootstrapError::Unavailable),
            None => return Err(BootstrapError::UnsafeMetadata),
        }
        if expected_build.is_some_and(|build| build != challenge.context.build) {
            return Err(BootstrapError::IncompatibleBuild);
        }
        write_frame(
            &mut channel,
            &ClientFrame::Redeem(Redeem {
                challenge: challenge.challenge,
                proof: expected,
            }),
        )
        .await?;
        let credentials = match read_frame(&mut channel, Duration::from_secs(5)).await? {
            ServerFrame::Credentials(c) => c,
            ServerFrame::Error(e) => return Err(e),
            _ => return Err(BootstrapError::InvalidFrame),
        };
        if credentials.role != role
            || credentials.context.profile != profile
            || credentials.context.endpoint != endpoint.url()
            || credentials.context.build != challenge.context.build
            || credentials.context.epoch != challenge.context.epoch
            || credentials.context.api != "v1"
        {
            return Err(BootstrapError::UnsafeMetadata);
        }
        unhex(&credentials.bearer)?;
        Ok(HttpClient {
            endpoint,
            bearer: credentials.bearer,
        })
    })
    .await
    .map_err(|_| BootstrapError::Expired)?
}
impl From<BootstrapError> for ClientError {
    fn from(value: BootstrapError) -> Self {
        Self::Bootstrap(value)
    }
}
