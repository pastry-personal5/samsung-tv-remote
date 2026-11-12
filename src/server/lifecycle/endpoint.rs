//! Non-secret stable endpoint allocation, always under profile ownership.
use super::files::{FileError, OwnershipLock, RuntimeDirectory};
use crate::infrastructure::private_files::DEVELOPMENT_NAMESPACE;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::net::{Ipv4Addr, TcpListener};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: u8,
    pub profile: String,
    pub port: u16,
    pub url: String,
    pub mcp_url: String,
    pub build: String,
    pub api: String,
}
impl Manifest {
    pub fn read(directory: &RuntimeDirectory) -> Result<Option<Self>, FileError> {
        directory
            .read("endpoint.json", 4096)?
            .map(|bytes| {
                let manifest: Self =
                    serde_json::from_slice(&bytes).map_err(|_| FileError::UnsafePath)?;
                if manifest.version != 1
                    || manifest.profile != DEVELOPMENT_NAMESPACE
                    || manifest.port < 1024
                    || manifest.url != format!("http://127.0.0.1:{}", manifest.port)
                    || manifest.mcp_url != format!("{}/mcp", manifest.url)
                    || manifest.api != "v1"
                    || crate::server::bootstrap::unhex(&manifest.build).is_err()
                {
                    return Err(FileError::UnsafePath);
                }
                Ok(manifest)
            })
            .transpose()
    }
    pub fn bind(
        directory: &RuntimeDirectory,
        _ownership: &OwnershipLock,
        build: &str,
    ) -> Result<(Self, TcpListener), EndpointError> {
        let previous = Self::read(directory).map_err(EndpointError::File)?;
        let listener = match previous {
            Some(ref previous) => TcpListener::bind((Ipv4Addr::LOCALHOST, previous.port))
                .map_err(|_| EndpointError::Collision)?,
            None => TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
                .map_err(|_| EndpointError::Unavailable)?,
        };
        let port = listener
            .local_addr()
            .map_err(|_| EndpointError::Unavailable)?
            .port();
        if port < 1024 {
            return Err(EndpointError::Unavailable);
        }
        listener
            .set_nonblocking(true)
            .map_err(|_| EndpointError::Unavailable)?;
        let url = format!("http://127.0.0.1:{port}");
        let manifest = Self {
            version: 1,
            profile: DEVELOPMENT_NAMESPACE.into(),
            port,
            mcp_url: format!("{url}/mcp"),
            url,
            build: build.into(),
            api: "v1".into(),
        };
        directory
            .replace(
                "endpoint.json",
                &serde_json::to_vec(&manifest).map_err(|_| EndpointError::Unavailable)?,
            )
            .map_err(EndpointError::File)?;
        Ok((manifest, listener))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointError {
    InvalidLockDescriptor,
    InvalidListenerDescriptor,
    InvalidDescriptorFlags,
    InheritedEndpoint,
    InheritedBuild,
    UnsafeInheritance,
    RejectedProfile,
    File(FileError),
    Collision,
    Unavailable,
}
pub fn build_identity(executable: &Path) -> Result<String, EndpointError> {
    let mut file = File::open(executable).map_err(|_| EndpointError::Unavailable)?;
    let mut digest = Sha256::new();
    let mut bytes = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut bytes)
            .map_err(|_| EndpointError::Unavailable)?;
        if count == 0 {
            break;
        }
        digest.update(&bytes[..count]);
    }
    Ok(crate::server::bootstrap::hex(&digest.finalize().into()))
}
