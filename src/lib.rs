//! `bookmark`: the go-tangra v4 bookmark module, written with the Rust SDK
//! (specs/038-bookmark-v4), structured like the SDK's notes example and the
//! Go reference module (go-tangra-deployer):
//!
//! - [`config`]: framework keys + module sections (incl. `db`), strict YAML;
//! - [`manifest`]: permissions, roles, grants, abilities, nav and the
//!   gateway manifest derived from `api/openapi/bookmark.yaml`;
//! - [`module`]: the wiring (identity, App, migrations, pool, verifier, auth
//!   registration, gateway lease, event bus, HTTP);
//! - [`api`]: the tenant-scoped HTTP API and its input validation;
//! - [`authz`]: the grant model (pure); [`store`]: PostgreSQL;
//!   [`backup`]: export/import;
//! - [`docs`] and `remote` (feature `ui`): the embedded help pages and UI
//!   remote.

#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used))]

pub mod api;
pub mod authz;
pub mod backup;
pub mod config;
pub mod docs;
pub mod healthcheck;
pub mod manifest;
pub mod module;
#[cfg(feature = "ui")]
pub mod remote;
pub mod store;

/// The release, reported to the gateway as the build version: the
/// `BOOKMARK_VERSION` the image build stamps, else the crate version.
pub const VERSION: &str = match option_env!("BOOKMARK_VERSION") {
    Some(v) => v,
    None => env!("CARGO_PKG_VERSION"),
};
