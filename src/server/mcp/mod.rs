//! In-process Streamable HTTP adapter. Owns projections, never TV effects.
pub mod catalog;
pub mod client_config;
pub mod connection;
mod cursor;
pub mod enrollment;
pub mod helper;
pub mod management;
pub mod protocol;
mod results;
mod tools;
pub(crate) use results::MAX_RESULT_BYTES;

use crate::application::control_service::{ClientId, ControlService};
use crate::server::access::{AuthorizedClient, Capability};
use protocol::{Reply, Rpc};
use serde_json::json;
use std::{collections::HashMap, sync::Arc};
use tokio::{sync::Semaphore, time::Instant};

pub struct Adapter {
    cursors: cursor::Cursors,
    waiters: Arc<Semaphore>,
    waiters_by_client: HashMap<ClientId, Arc<Semaphore>>,
}
impl Default for Adapter {
    fn default() -> Self {
        Self {
            cursors: cursor::Cursors::default(),
            waiters: Arc::new(Semaphore::new(4)),
            waiters_by_client: [ClientId(1), ClientId(2)]
                .into_iter()
                .map(|id| (id, Arc::new(Semaphore::new(2))))
                .collect(),
        }
    }
}
impl Adapter {
    pub async fn dispatch(
        &self,
        rpc: Rpc,
        service: &ControlService,
        client: &AuthorizedClient,
        build: &str,
        deadline: Instant,
    ) -> Reply {
        if !client.admission.is_active() || client.capability == Capability::Readiness {
            return Reply::error(403, Some(&rpc.id), -32600, "Control authorization required");
        }
        let result = match rpc.method.as_str() {
            "server/discover" => {
                json!({"supportedVersions":[protocol::VERSION],"capabilities":{"tools":{}},"instructions":catalog::INSTRUCTIONS,
                "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"samsung-tv-remote","version":build}},"ttlMs":catalog::CACHE_TTL_MS,"cacheScope":"private"})
            }
            "tools/list" => {
                json!({"tools":catalog::tools(),"ttlMs":catalog::CACHE_TTL_MS,"cacheScope":"private"})
            }
            "tools/call" => {
                let arguments = rpc
                    .params
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                let value = tools::call(
                    self,
                    rpc.tool_name().expect("validated tool"),
                    arguments,
                    service,
                    client,
                    deadline,
                )
                .await;
                results::tool(value)
            }
            _ => return Reply::error(404, Some(&rpc.id), -32601, "Unknown method"),
        };
        Reply::result(&rpc.id, result)
    }
}
