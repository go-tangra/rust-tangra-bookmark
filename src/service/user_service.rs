use tonic::{Request, Response, Status};

use crate::client::admin_client::AdminClient;
use crate::service::bookmark_service::proto::{
    bookmark_user_service_server::BookmarkUserService,
    BookmarkRole, BookmarkUser,
    ListBookmarkRolesRequest, ListBookmarkRolesResponse,
    ListBookmarkUsersRequest, ListBookmarkUsersResponse,
};

pub struct UserServiceImpl {
    admin_client: AdminClient,
}

impl UserServiceImpl {
    pub fn new(admin_client: AdminClient) -> Self {
        Self { admin_client }
    }
}

#[tonic::async_trait]
impl BookmarkUserService for UserServiceImpl {
    async fn list_users(
        &self,
        _request: Request<ListBookmarkUsersRequest>,
    ) -> Result<Response<ListBookmarkUsersResponse>, Status> {
        let resp = self.admin_client.list_users().await.map_err(|e| {
            tracing::error!(error = %e, "failed to list users from admin-service");
            Status::internal("failed to list users")
        })?;

        let items: Vec<BookmarkUser> = resp
            .items
            .into_iter()
            .map(|u| BookmarkUser {
                id: u.id,
                username: u.username,
                realname: u.realname,
                email: u.email,
                org_unit_names: u.org_unit_names,
                position_names: u.position_names,
            })
            .collect();

        let total = items.len() as i32;
        Ok(Response::new(ListBookmarkUsersResponse { items, total }))
    }

    async fn list_roles(
        &self,
        _request: Request<ListBookmarkRolesRequest>,
    ) -> Result<Response<ListBookmarkRolesResponse>, Status> {
        let resp = self.admin_client.list_roles().await.map_err(|e| {
            tracing::error!(error = %e, "failed to list roles from admin-service");
            Status::internal("failed to list roles")
        })?;

        let items: Vec<BookmarkRole> = resp
            .items
            .into_iter()
            .map(|r| BookmarkRole {
                id: r.id,
                name: r.name,
                code: r.code,
                description: r.description,
            })
            .collect();

        let total = items.len() as i32;
        Ok(Response::new(ListBookmarkRolesResponse { items, total }))
    }
}
