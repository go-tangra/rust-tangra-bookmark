use std::time::Duration;

use tokio::sync::watch;
use tonic::transport::Channel;

/// Generated module registration client.
pub mod proto {
    tonic::include_proto!("common.service.v1");
}

use proto::module_registration_service_client::ModuleRegistrationServiceClient;
use proto::{HeartbeatRequest, ModuleHealth, RegisterModuleRequest, UnregisterModuleRequest};

const MODULE_ID: &str = "bookmark";
const MODULE_NAME: &str = "Bookmark";
const VERSION: &str = "1.0.0";
const DESCRIPTION: &str = "URL Bookmark Management with Zanzibar-like permissions";

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_secs(5);
const MAX_RETRIES: u32 = 60;
const STARTUP_DELAY: Duration = Duration::from_secs(3);

/// Start module registration lifecycle in a background task.
/// Returns a shutdown sender — drop it to trigger unregistration.
pub fn start_registration(
    shutdown_rx: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let admin_endpoint = std::env::var("ADMIN_GRPC_ENDPOINT").unwrap_or_default();
        if admin_endpoint.is_empty() {
            tracing::info!("ADMIN_GRPC_ENDPOINT not set, skipping module registration");
            return;
        }

        tracing::info!(endpoint = %admin_endpoint, "will register with admin gateway");

        // Wait for gRPC server to be ready
        tokio::time::sleep(STARTUP_DELAY).await;

        let channel = match connect_with_retry(&admin_endpoint).await {
            Some(ch) => ch,
            None => {
                tracing::error!("failed to connect to admin gateway after retries");
                return;
            }
        };

        let mut client = ModuleRegistrationServiceClient::new(channel);

        // Register
        if let Err(e) = register(&mut client).await {
            tracing::error!(error = %e, "failed to register with admin gateway");
            return;
        }

        // Heartbeat loop
        heartbeat_loop(&mut client, shutdown_rx).await;

        // Unregister on shutdown
        unregister(&mut client).await;
    })
}

async fn connect_with_retry(endpoint: &str) -> Option<Channel> {
    for attempt in 1..=MAX_RETRIES {
        match Channel::from_shared(format!("http://{endpoint}"))
            .ok()?
            .connect()
            .await
        {
            Ok(ch) => return Some(ch),
            Err(e) => {
                tracing::warn!(attempt, error = %e, "connection attempt failed");
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
        }
    }
    None
}

async fn register(
    client: &mut ModuleRegistrationServiceClient<Channel>,
) -> anyhow::Result<()> {
    let grpc_endpoint = std::env::var("GRPC_ADVERTISE_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:9700".to_string());
    let auth_token = std::env::var("MODULE_AUTH_TOKEN").unwrap_or_default();

    let openapi_spec = std::fs::read("assets/openapi.yaml").unwrap_or_default();
    let menus_yaml = std::fs::read("assets/menus.yaml").unwrap_or_default();
    let proto_descriptor = std::fs::read(
        std::env::var("PROTO_DESCRIPTOR_PATH")
            .unwrap_or_else(|_| "assets/descriptor.bin".to_string()),
    )
    .unwrap_or_default();

    let req = RegisterModuleRequest {
        module_id: MODULE_ID.to_string(),
        module_name: MODULE_NAME.to_string(),
        version: VERSION.to_string(),
        description: DESCRIPTION.to_string(),
        grpc_endpoint,
        openapi_spec,
        proto_descriptor,
        menus_yaml,
        auth_token,
    };

    for attempt in 1..=MAX_RETRIES {
        match client.register_module(req.clone()).await {
            Ok(resp) => {
                let resp = resp.into_inner();
                tracing::info!(
                    registration_id = %resp.registration_id,
                    status = resp.status,
                    message = %resp.message,
                    "module registered successfully"
                );
                return Ok(());
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "registration attempt failed");
                tokio::time::sleep(RETRY_INTERVAL).await;
            }
        }
    }

    anyhow::bail!("exhausted registration retries")
}

async fn heartbeat_loop(
    client: &mut ModuleRegistrationServiceClient<Channel>,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    tracing::info!(interval = ?HEARTBEAT_INTERVAL, "starting heartbeat");
    let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
    interval.tick().await; // skip first immediate tick

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let req = HeartbeatRequest {
                    module_id: MODULE_ID.to_string(),
                    health: ModuleHealth::Healthy.into(),
                    message: "Bookmark service is healthy".to_string(),
                };
                match client.heartbeat(req).await {
                    Ok(resp) => {
                        if !resp.into_inner().acknowledged {
                            tracing::warn!("heartbeat not acknowledged");
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "heartbeat failed");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                tracing::info!("heartbeat stopped due to shutdown");
                break;
            }
        }
    }
}

async fn unregister(client: &mut ModuleRegistrationServiceClient<Channel>) {
    let auth_token = std::env::var("MODULE_AUTH_TOKEN").unwrap_or_default();
    let req = UnregisterModuleRequest {
        module_id: MODULE_ID.to_string(),
        auth_token,
    };

    match client.unregister_module(req).await {
        Ok(_) => tracing::info!("module unregistered successfully"),
        Err(e) => tracing::warn!(error = %e, "failed to unregister module"),
    }
}
