use tonic::transport::Channel;

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
        let channel = Channel::from_shared(format!("http://{endpoint}"))
            .expect("invalid endpoint")
            .connect()
            .await?;
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
