//! The import's inputs: the source configuration file, the user files and
//! the role map (warden `internal/migrate3/mapping.go`, sms-gw
//! `deploy/legacy-import.dev.yaml`).

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use serde::Deserialize;
use uuid::Uuid;

use super::MigrateError;

/// The v4 platform tenant: v3 tenant 0 (the platform) lands here unless
/// the configuration maps it elsewhere.
pub const PLATFORM_TENANT: &str = "00000000-0000-0000-0000-000000000001";

/// A secret given by reference, never inline: an environment variable or a
/// file (trailing newline trimmed).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecretRef {
    /// Environment variable holding the value.
    pub env: String,
    /// File holding the value.
    pub file: String,
}

impl SecretRef {
    /// Reads the value.
    pub fn read(&self) -> Result<String, MigrateError> {
        let v = match (self.env.is_empty(), self.file.is_empty()) {
            (false, true) => std::env::var(&self.env).map_err(|_| {
                MigrateError(format!(
                    "source.dsn: environment variable {} is not set",
                    self.env
                ))
            })?,
            (true, false) => std::fs::read_to_string(&self.file)
                .map_err(|e| MigrateError(format!("source.dsn: {}: {e}", self.file)))?
                .trim_end_matches(['\n', '\r'])
                .to_owned(),
            _ => return Err(MigrateError("source.dsn needs exactly one of env or file".into())),
        };
        if v.is_empty() {
            return Err(MigrateError("source.dsn is empty".into()));
        }
        Ok(v)
    }
}

/// `source:`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Source {
    /// The v3 database (`gwa`): ideally a restored snapshot, never written.
    pub dsn: SecretRef,
}

/// The import configuration (`-source-config`):
///
/// ```yaml
/// source:
///   dsn: { env: BOOKMARK_LEGACY_DSN }
/// tenants: { "0": "00000000-0000-0000-0000-000000000001" }
/// users_csv: /import/users.csv          # email,uuid (auth's users)
/// v3_users_csv: /import/v3-users.csv    # v3id,uuid (optional, wins)
/// roles: { "platform:admin": admin }
/// platform_actor: ""
/// exclude_orphans: false
/// exclude_invalid: false
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SourceConfig {
    /// The read-only source.
    pub source: Source,
    /// v3 tenant ID → v4 tenant UUID (tenant 0 defaults to the platform
    /// tenant).
    pub tenants: BTreeMap<String, String>,
    /// `email,uuid` lines: the v4 users (with the v3 `sys_users` table of
    /// the source, which gives each v3 user ID its e-mail).
    pub users_csv: String,
    /// `v3id,uuid` lines: a direct v3 user ID → v4 user mapping.
    pub v3_users_csv: String,
    /// v3 role code → v4 role slug.
    pub roles: BTreeMap<String, String>,
    /// The author recorded for v3 rows without one (`created_by`/`granted_by`
    /// NULL or 0), and for unmapped authors under `exclude_orphans`.
    pub platform_actor: String,
    /// Leave out (and list) rows whose tenant, user, role or bookmark has no
    /// mapping instead of failing.
    pub exclude_orphans: bool,
    /// Leave out (and list) rows the v4 schema refuses instead of failing.
    pub exclude_invalid: bool,
}

impl SourceConfig {
    /// Reads and decodes `path` strictly.
    pub fn load(path: &str) -> Result<Self, MigrateError> {
        let raw = std::fs::read(path).map_err(|e| MigrateError(format!("source config {path}: {e}")))?;
        serde_norway::from_slice(&raw).map_err(|e| MigrateError(format!("source config {path}: {e}")))
    }
}

/// Canonical (lower-case, hyphenated) UUID text, or `None`.
pub fn canonical_uuid(s: &str) -> Option<String> {
    let s = s.trim();
    let u = Uuid::parse_str(s).ok()?;
    let c = u.to_string();
    (c == s.to_ascii_lowercase() && s.len() == 36).then_some(c)
}

/// Parses `v3=v4` pairs separated by commas (warden `ParseRoleMap`), e.g.
/// `0=00000000-0000-0000-0000-000000000001` or `platform:admin=admin`.
pub fn parse_pairs(s: &str, what: &str) -> Result<BTreeMap<String, String>, MigrateError> {
    let mut m = BTreeMap::new();
    for pair in s.split(',').map(str::trim).filter(|p| !p.is_empty()) {
        let (from, to) = pair
            .split_once('=')
            .map(|(a, b)| (a.trim(), b.trim()))
            .filter(|(a, b)| !a.is_empty() && !b.is_empty() && !b.contains(char::is_whitespace))
            .ok_or_else(|| MigrateError(format!("{what} entry {pair:?}: want v3=v4")))?;
        if m.insert(from.to_owned(), to.to_owned()).is_some() {
            return Err(MigrateError(format!("{what}: {from:?} mapped twice")));
        }
    }
    Ok(m)
}

/// Reads two-column CSV lines (`psql \copy ... CSV`): no quoting, a first
/// line whose second field is not a UUID is a header.
fn read_csv(raw: &str, file: &str) -> Result<Vec<(usize, String, String)>, MigrateError> {
    let mut out = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let bad = || MigrateError(format!("{file} line {}: want two fields: key,uuid", i + 1));
        let (a, b) = line.split_once(',').ok_or_else(bad)?;
        if b.contains(',') || line.contains('"') {
            return Err(bad());
        }
        let (a, b) = (a.trim(), b.trim());
        match canonical_uuid(&b.to_ascii_lowercase()) {
            Some(id) => out.push((i + 1, a.to_owned(), id)),
            None if out.is_empty() && i == 0 => {} // header
            None => {
                return Err(MigrateError(format!(
                    "{file} line {}: {b:?} is not a UUID",
                    i + 1
                )));
            }
        }
    }
    Ok(out)
}

/// `email,uuid`: lower-cased e-mail → v4 user UUID.
pub fn parse_users(raw: &str, file: &str) -> Result<HashMap<String, String>, MigrateError> {
    let mut m = HashMap::new();
    for (line, email, id) in read_csv(raw, file)? {
        let email = email.to_lowercase();
        if !email.contains('@') {
            return Err(MigrateError(format!(
                "{file} line {line}: {email:?} is not an e-mail"
            )));
        }
        if m.get(&email).is_some_and(|p| *p != id) {
            return Err(MigrateError(format!(
                "{file} line {line}: {email} maps to two users"
            )));
        }
        m.insert(email, id);
    }
    Ok(m)
}

/// `v3id,uuid`: v3 user ID → v4 user UUID.
pub fn parse_v3_users(raw: &str, file: &str) -> Result<HashMap<i64, String>, MigrateError> {
    let mut m = HashMap::new();
    for (line, v3, id) in read_csv(raw, file)? {
        let v3: i64 = v3
            .parse()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| MigrateError(format!("{file} line {line}: {v3:?} is not a v3 user ID")))?;
        if m.get(&v3).is_some_and(|p| *p != id) {
            return Err(MigrateError(format!(
                "{file} line {line}: v3 user {v3} maps to two users"
            )));
        }
        m.insert(v3, id);
    }
    Ok(m)
}

/// Everything that maps v3 identifiers to v4 ones.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Mapping {
    /// v3 tenant → v4 tenant UUID.
    pub tenants: BTreeMap<i64, String>,
    /// e-mail (lower case) → v4 user UUID.
    pub users_by_email: HashMap<String, String>,
    /// v3 user ID → v4 user UUID (wins over the e-mail route).
    pub users_by_v3_id: HashMap<i64, String>,
    /// v3 role code → v4 role slug.
    pub roles: BTreeMap<String, String>,
    /// The author for rows without one.
    pub platform_actor: String,
}

impl Mapping {
    /// Builds the mapping from the configuration and the command-line
    /// overrides (`-tenant`, `-roles`, `-users`, `-v3-users`,
    /// `-platform-actor`; each wins over the file).
    pub fn build(cfg: &SourceConfig, o: &Overrides) -> Result<Self, MigrateError> {
        let mut tenants = BTreeMap::from([(0_i64, PLATFORM_TENANT.to_owned())]);
        let mut raw_tenants = cfg.tenants.clone();
        raw_tenants.extend(parse_pairs(&o.tenants, "-tenant")?);
        for (k, v) in raw_tenants {
            let k: i64 = k
                .parse()
                .ok()
                .filter(|n| *n >= 0)
                .ok_or_else(|| MigrateError(format!("tenants: {k:?} is not a v3 tenant ID")))?;
            let v = canonical_uuid(&v)
                .ok_or_else(|| MigrateError(format!("tenants: {v:?} is not a lower-case UUID")))?;
            tenants.insert(k, v);
        }
        let mut roles = cfg.roles.clone();
        roles.extend(parse_pairs(&o.roles, "-roles")?);
        for (k, v) in &roles {
            if crate::api::input::subject_id(crate::authz::SubjectType::Role, v).is_err() {
                return Err(MigrateError(format!(
                    "roles: {k} maps to {v:?}, not a v4 role slug"
                )));
            }
        }
        let read = |flag: &str, file: &str| -> Result<Option<(String, String)>, MigrateError> {
            let path = if flag.is_empty() { file } else { flag };
            if path.is_empty() {
                return Ok(None);
            }
            let raw =
                std::fs::read_to_string(Path::new(path)).map_err(|e| MigrateError(format!("{path}: {e}")))?;
            Ok(Some((raw, path.to_owned())))
        };
        let users_by_email = match read(&o.users, &cfg.users_csv)? {
            Some((raw, p)) => parse_users(&raw, &p)?,
            None => HashMap::new(),
        };
        let users_by_v3_id = match read(&o.v3_users, &cfg.v3_users_csv)? {
            Some((raw, p)) => parse_v3_users(&raw, &p)?,
            None => HashMap::new(),
        };
        let platform_actor = o
            .platform_actor
            .clone()
            .unwrap_or_else(|| cfg.platform_actor.clone());
        if platform_actor.chars().count() > crate::api::input::MAX_SUBJECT_ID {
            return Err(MigrateError("platform actor is longer than 128".into()));
        }
        Ok(Self {
            tenants,
            users_by_email,
            users_by_v3_id,
            roles,
            platform_actor,
        })
    }
}

/// Command-line overrides of the configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `-tenant v3=uuid,...`.
    pub tenants: String,
    /// `-roles v3=v4,...`.
    pub roles: String,
    /// `-users path` (`email,uuid`).
    pub users: String,
    /// `-v3-users path` (`v3id,uuid`).
    pub v3_users: String,
    /// `-platform-actor`.
    pub platform_actor: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "0192f6b0-0000-7000-8000-00000000a0a0";

    #[test]
    fn pairs_and_csv() {
        let m = parse_pairs("platform:admin=admin, 3 = x ,", "-roles").unwrap();
        assert_eq!(m["platform:admin"], "admin");
        assert_eq!(m["3"], "x");
        assert!(parse_pairs("a", "-roles").is_err());
        assert!(parse_pairs("a=b,a=c", "-roles").is_err());
        assert!(parse_pairs("a=b c", "-roles").is_err());

        let u = parse_users(
            &format!("email,id\nAna@Example.org,{}\n\n", A.to_uppercase()),
            "u",
        )
        .unwrap();
        assert_eq!(u["ana@example.org"], A);
        assert!(parse_users(&format!("ana,{A}\n"), "u").is_err());
        assert!(parse_users(&format!("a@x,{A}\na@x,{}\n", PLATFORM_TENANT), "u").is_err());
        assert!(parse_users("a@x,nope\nb@x,nope\n", "u").is_err());
        assert!(parse_users(&format!("\"a@x\",{A}\n"), "u").is_err());

        let v = parse_v3_users(&format!("7,{A}\n"), "v").unwrap();
        assert_eq!(v[&7], A);
        assert!(parse_v3_users(&format!("x,{A}\n"), "v").is_err());
        assert!(parse_v3_users(&format!("0,{A}\n"), "v").is_err());
    }

    #[test]
    fn mapping_defaults_and_overrides() {
        let cfg: SourceConfig = serde_norway::from_str(
            "source: {dsn: {env: X}}\ntenants: {\"5\": \"0192f6b0-0000-7000-8000-0000000000f1\"}\nroles: {\"platform:admin\": admin}\n",
        )
        .unwrap();
        let m = Mapping::build(
            &cfg,
            &Overrides {
                tenants: "6=0192f6b0-0000-7000-8000-0000000000f2".into(),
                roles: "tenant:manager=bookmark-admin".into(),
                platform_actor: Some("legacy".into()),
                ..Overrides::default()
            },
        )
        .unwrap();
        assert_eq!(m.tenants[&0], PLATFORM_TENANT);
        assert_eq!(m.tenants.len(), 3);
        assert_eq!(m.roles["tenant:manager"], "bookmark-admin");
        assert_eq!(m.platform_actor, "legacy");
        let bad = |t: &str, r: &str| {
            Mapping::build(
                &cfg,
                &Overrides {
                    tenants: t.into(),
                    roles: r.into(),
                    ..Overrides::default()
                },
            )
            .is_err()
        };
        assert!(bad("x=0192f6b0-0000-7000-8000-0000000000f2", ""));
        assert!(bad("1=NOPE", ""));
        assert!(bad("", "a=-bad"));
        assert!(serde_norway::from_str::<SourceConfig>("nope: 1\n").is_err());
        assert!(SecretRef::default().read().is_err());
    }
}
