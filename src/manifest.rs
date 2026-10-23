//! What the bookmark module registers with the gateway and with auth (the
//! deployer's `pkg/deployermanifest`, paperless `pkg/paperlessmanifest`):
//! routes derived from the embedded OpenAPI document, the API permissions,
//! CASL abilities, navigation, module roles and built-in role grants.
//!
//! Bump [`VERSION`] on every manifest change: the gateway refuses changed
//! content under an unchanged version (`manifest_drift`); the test
//! `manifest_version_pinned` fails until you do.

use std::collections::BTreeMap;

use tangra_sdk::auth::{self, ModuleRole, Registration};
use tangra_sdk::manifest::{self, Ability, Manifest, ManifestError, NavEntry, Permission};

/// Module (and service) name.
pub const MODULE: &str = "bookmark";
/// Shown in the portal.
pub const DISPLAY_NAME: &str = "Bookmarks";
/// Manifest version: bump on every manifest change (pinned in
/// `testdata/manifest.sha256`).
pub const VERSION: &str = "1.0.0";
/// The owned API prefix.
pub const PREFIX: &str = "/api/bookmark";
/// The module-federation exposes (the UI remote's `module-federation.config.ts`).
pub const EXPOSES: [&str; 3] = ["./routes", "./nav", "./boot"];

/// The OpenAPI contract the routes are derived from.
pub const OPENAPI: &[u8] = include_bytes!("../api/openapi/bookmark.yaml");

/// The API permissions (paperless naming: `<resource>:<action>`).
pub fn permissions() -> Vec<Permission> {
    [
        ("bookmarks", "read", "List and open the bookmarks shared with you"),
        ("bookmarks", "create", "Create bookmarks"),
        ("bookmarks", "update", "Change bookmarks you may edit"),
        ("bookmarks", "delete", "Remove bookmarks you own"),
        ("permissions", "manage", "Share bookmarks and revoke access"),
        (
            "backup",
            "manage",
            "Export and import the tenant's bookmarks and grants",
        ),
    ]
    .into_iter()
    .map(|(resource, action, description)| Permission {
        resource: resource.into(),
        action: action.into(),
        description: description.into(),
    })
    .collect()
}

/// `resource:action` of every permission (also the catalogue entry's list).
pub fn permission_refs() -> Vec<String> {
    permissions().iter().map(Permission::reference).collect()
}

fn refs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

/// What a member uses: the bookmark operations and sharing.
const MEMBER: [&str; 5] = [
    "bookmarks:read",
    "bookmarks:create",
    "bookmarks:update",
    "bookmarks:delete",
    "permissions:manage",
];

/// Built-in role grants. The complete set: a grant withdrawn here is
/// revoked from existing tenants' built-in roles.
pub fn grants() -> BTreeMap<String, Vec<String>> {
    let all = permission_refs();
    BTreeMap::from([
        ("owner".to_owned(), all.clone()),
        ("admin".to_owned(), all),
        ("member".to_owned(), refs(&MEMBER)),
        ("operator".to_owned(), refs(&MEMBER)),
        ("auditor".to_owned(), refs(&["bookmarks:read"])),
    ])
}

/// Module roles auth provides in every tenant (v3 `bookmark.admin`,
/// `bookmark.viewer`).
pub fn roles() -> Vec<ModuleRole> {
    vec![
        ModuleRole {
            slug: "administrator".into(),
            display_name: format!("{DISPLAY_NAME} administrator"),
            description: "Every bookmark permission, including sharing and backup".into(),
            permissions: permission_refs(),
        },
        ModuleRole {
            slug: "viewer".into(),
            display_name: format!("{DISPLAY_NAME} viewer"),
            description: "Open the bookmarks shared with you".into(),
            permissions: refs(&["bookmarks:read"]),
        },
    ]
}

/// CASL rules bound to the permissions (the UI's `can()`).
pub fn abilities() -> Vec<Ability> {
    let rule = |actions: &[&str], subject: &str, requires: &str| Ability {
        action: refs(actions),
        subject: vec![subject.into()],
        requires: requires.into(),
        ..Ability::default()
    };
    vec![
        rule(&["read"], "Bookmark", "bookmarks:read"),
        rule(&["create"], "Bookmark", "bookmarks:create"),
        rule(&["update"], "Bookmark", "bookmarks:update"),
        rule(&["delete"], "Bookmark", "bookmarks:delete"),
        rule(&["manage"], "BookmarkPermission", "permissions:manage"),
        rule(&["manage"], "BookmarkBackup", "backup:manage"),
    ]
}

/// Navigation contributions (icons must be in the `@go-tangra/ui` kit set).
pub fn nav() -> Vec<NavEntry> {
    vec![
        NavEntry {
            title: "Bookmarks".into(),
            path: "/bookmark".into(),
            icon: "mdi-link-variant".into(),
            order: 600,
            requires: "bookmarks:read".into(),
        },
        NavEntry {
            title: "Sharing".into(),
            path: "/bookmark/permissions".into(),
            icon: "mdi-account-key-outline".into(),
            order: 610,
            requires: "permissions:manage".into(),
        },
        NavEntry {
            title: "Bookmark backup".into(),
            path: "/bookmark/backup".into(),
            icon: "mdi-database-arrow-up-outline".into(),
            order: 620,
            requires: "backup:manage".into(),
        },
    ]
}

/// The gateway manifest; the routes come from [`OPENAPI`] (an undeclared
/// permission is an error).
pub fn manifest() -> Result<Manifest, ManifestError> {
    let routes = manifest::openapi::routes(OPENAPI, &permission_refs())?;
    let mut m = Manifest::new(MODULE, DISPLAY_NAME, VERSION)
        .prefix(PREFIX)
        .routes(routes)
        .exposes(EXPOSES);
    m.permissions = permissions();
    m.abilities = abilities();
    m.nav = nav();
    Ok(m)
}

/// What the module registers with auth at start and every five minutes.
pub fn registration() -> Registration {
    Registration {
        module: MODULE.into(),
        display_name: DISPLAY_NAME.into(),
        permissions: permissions()
            .into_iter()
            .map(|p| auth::Permission {
                resource: p.resource,
                action: p.action,
                description: p.description,
            })
            .collect(),
        roles: roles(),
        builtin_grants: grants(),
        declares_grants: true,
        tenant_ids: Vec::new(),
    }
}
