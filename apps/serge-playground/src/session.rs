//! The demo session belongs to one running UI. Each call keeps the transport
//! it started with, including when sign-in or sign-out replaces it.

use std::sync::{Arc, RwLock};

use hyve::HyveResult;
use hyve::api::{BoxFuture, Call, Reply, Transport};
use serge::RemoteTransport;

use crate::{demo_login, demo_logout};

#[derive(Clone)]
enum Connection {
    Remote(RemoteTransport),
    #[cfg(feature = "server")]
    InProcess(orbic::InProcess),
}

impl Connection {
    fn transport(&self, token: Option<&str>) -> Arc<dyn Transport> {
        match self {
            Self::Remote(base) => {
                let mut transport = base.clone();
                if let Some(token) = token {
                    transport = if cfg!(target_arch = "wasm32") {
                        transport.csrf_token(token)
                    } else {
                        transport.bearer(token)
                    };
                }
                transport.into_arc()
            }
            #[cfg(feature = "server")]
            Self::InProcess(base) => {
                let mut transport = base.clone();
                if let Some(token) = token {
                    transport = transport.with_credential(orbic::Credential::Bearer(token.into()));
                }
                Arc::new(transport)
            }
        }
    }
}

#[derive(Clone)]
pub struct PlaygroundSession {
    connection: Connection,
    current: Arc<RwLock<Arc<dyn Transport>>>,
}

impl PlaygroundSession {
    fn new(connection: Connection) -> Self {
        Self {
            current: Arc::new(RwLock::new(connection.transport(None))),
            connection,
        }
    }

    pub fn remote(transport: RemoteTransport) -> Self {
        Self::new(Connection::Remote(transport))
    }

    #[cfg(feature = "server")]
    pub fn in_process(pipeline: orbic::Pipeline) -> Self {
        Self::new(Connection::InProcess(orbic::InProcess::new(pipeline)))
    }

    pub async fn sign_in(&self) -> HyveResult<()> {
        let token = demo_login(cfg!(target_arch = "wasm32")).await?;
        self.use_token(Some(&token));
        Ok(())
    }

    pub async fn sign_out(&self) -> HyveResult<()> {
        match demo_logout().await {
            Err(e) if e.code() == "unauthenticated" => {}
            result => result?,
        }
        self.use_token(None);
        Ok(())
    }

    fn use_token(&self, token: Option<&str>) {
        *self.current.write().unwrap() = self.connection.transport(token);
    }
}

impl Transport for PlaygroundSession {
    fn call(&self, call: Call) -> BoxFuture<'_, HyveResult<Reply>> {
        let transport = self.current.read().unwrap().clone();
        Box::pin(async move { transport.call(call).await })
    }
}
