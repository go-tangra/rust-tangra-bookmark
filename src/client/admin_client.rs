use tonic::transport::{Channel, Endpoint};
use crate::cert::load_client_tls_config;

/// Generated admin stub types (client only).
pub mod proto {
    tonic::include_proto!("admin.service.v1");
}

use proto::user_service_client::UserServiceClient;
use proto::role_service_client::RoleServiceClient;
use proto::{ListUserResponse, ListRoleResponse, PagingRequest};

/// Calls admin-service gRPC API for user and role listing.
#[derive(Clone)]
pub struct AdminClient {
    user_client: UserServiceClient<Channel>,
    role_client: RoleServiceClient<Channel>,
}

impl AdminClient {
    pub fn new(channel: Channel) -> Self {
        Self {
            user_client: UserServiceClient::new(channel.clone()),
            role_client: RoleServiceClient::new(channel),
        }
    }

    pub async fn connect(endpoint: &str) -> Result<Self, tonic::transport::Error> {
        let client_tls = load_client_tls_config();
        let scheme = if client_tls.is_some() { "https" } else { "http" };
        let mut ep = Endpoint::from_shared(format!("{scheme}://{endpoint}"))
            .expect("invalid endpoint");
        if let Some(tls) = client_tls {
            ep = ep.tls_config(tls).expect("invalid TLS config");
        }
        let channel = ep.connect().await?;
        Ok(Self::new(channel))
    }

    pub async fn list_users(&self) -> Result<ListUserResponse, tonic::Status> {
        let req = PagingRequest {
            no_paging: Some(true),
        };
        let resp = self.user_client.clone().list(req).await?;
        Ok(resp.into_inner())
    }

    pub async fn list_roles(&self) -> Result<ListRoleResponse, tonic::Status> {
        let req = PagingRequest {
            no_paging: Some(true),
        };
        let resp = self.role_client.clone().list(req).await?;
        Ok(resp.into_inner())
    }
}
