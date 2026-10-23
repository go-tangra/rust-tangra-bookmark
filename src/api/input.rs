//! Request bodies and parameters, validated in the handlers with the
//! outcomes the deployer's kin-openapi validation gives (the SDK has no
//! OpenAPI request validator): not JSON is `400 malformed_body`, a schema
//! violation `422 validation_failed` with `detail.fields` (codes only, never
//! values), a bad query parameter `422` with `detail.param`.

use axum::body::Body;
use chrono::{DateTime, Utc};
use serde_json::{Map, Value, json};
use tangra_sdk::error::ApiError;
use uuid::Uuid;

use crate::authz::{Action, RESOURCE, Relation, SubjectType};

/// JSON bodies of ordinary operations (deployer `MaxBodyBytes`).
pub const MAX_BODY_BYTES: usize = 64 << 10;
/// URL length limit (characters).
pub const MAX_URL: usize = 2048;
/// Title length limit.
pub const MAX_TITLE: usize = 500;
/// Description length limit.
pub const MAX_DESCRIPTION: usize = 5000;
/// Tags per bookmark.
pub const MAX_TAGS: usize = 20;
/// Tag length limit.
pub const MAX_TAG: usize = 64;
/// `q` length limit.
pub const MAX_QUERY: usize = 200;
/// `tag` filter values per list request.
pub const MAX_TAG_FILTER: usize = 10;
/// Subject ID length limit.
pub const MAX_SUBJECT_ID: usize = 128;

/// Field errors collected while decoding (the first code per field wins).
#[derive(Debug, Default)]
pub struct Errors(Map<String, Value>);

impl Errors {
    /// Records `code` for `field` unless it already has one.
    pub fn set(&mut self, field: &str, code: &str) {
        self.0.entry(field.to_owned()).or_insert_with(|| json!(code));
    }

    /// Whether `field` failed.
    pub fn has(&self, field: &str) -> bool {
        self.0.contains_key(field)
    }

    /// `Ok(v)` when nothing failed, else `422` with `detail.fields`.
    pub fn finish<T>(self, v: T) -> Result<T, ApiError> {
        if self.0.is_empty() {
            Ok(v)
        } else {
            Err(fields(self.0))
        }
    }
}

/// `422 validation_failed` with `detail.fields`.
pub fn fields(f: Map<String, Value>) -> ApiError {
    let mut d = Map::new();
    d.insert("fields".into(), Value::Object(f));
    ApiError::validation_failed().with_detail(d)
}

/// `422` for one field.
pub fn field(name: &str, code: &str) -> ApiError {
    let mut e = Errors::default();
    e.set(name, code);
    fields(e.0)
}

/// Reads a JSON body of at most `limit` bytes.
pub async fn read_json(body: Body, limit: usize) -> Result<Value, ApiError> {
    let raw = axum::body::to_bytes(body, limit)
        .await
        .map_err(|_| ApiError::body_too_large())?;
    serde_json::from_slice(&raw).map_err(|_| ApiError::malformed_body())
}

/// Reads a JSON object body (anything else is `422`).
pub async fn read_object(body: Body, limit: usize) -> Result<Map<String, Value>, ApiError> {
    match read_json(body, limit).await? {
        Value::Object(o) => Ok(o),
        _ => Err(ApiError::validation_failed()),
    }
}

/// Marks every key outside `known` as `unknown`.
pub fn unknown_keys(obj: &Map<String, Value>, known: &[&str], e: &mut Errors) {
    for k in obj.keys() {
        if !known.contains(&k.as_str()) {
            e.set(k, "unknown");
        }
    }
}

fn has_control(s: &str, allow_newlines: bool) -> bool {
    s.chars()
        .any(|c| c.is_control() && !(allow_newlines && matches!(c, '\n' | '\r' | '\t')))
}

/// An absolute `http`/`https` URL with a host, at most [`MAX_URL`]
/// characters, trimmed (SR-004: no `javascript:`, `data:`, ...).
pub fn url(s: &str) -> Result<String, &'static str> {
    let s = s.trim();
    if s.is_empty() {
        return Err("required");
    }
    if s.chars().count() > MAX_URL {
        return Err("too_long");
    }
    if has_control(s, false) || s.contains(char::is_whitespace) {
        return Err("invalid");
    }
    let u = url::Url::parse(s).map_err(|_| "invalid")?;
    if !matches!(u.scheme(), "http" | "https") {
        return Err("scheme");
    }
    if u.host_str().is_none_or(str::is_empty) {
        return Err("invalid");
    }
    Ok(s.to_owned())
}

/// Text of at most `max` characters (trimmed when `trim`); control
/// characters only as line breaks and tabs in multi-line text.
pub fn text(s: &str, max: usize, multiline: bool) -> Result<String, &'static str> {
    let s = if multiline { s } else { s.trim() };
    if s.chars().count() > max {
        return Err("too_long");
    }
    if has_control(s, multiline) {
        return Err("invalid");
    }
    Ok(s.to_owned())
}

/// Normalizes tags: trimmed, non-empty, at most [`MAX_TAG`] characters, no
/// control characters, duplicates collapsed (first kept), at most
/// [`MAX_TAGS`].
pub fn tags(raw: &[String]) -> Result<Vec<String>, &'static str> {
    let mut out: Vec<String> = Vec::new();
    for t in raw {
        let t = t.trim();
        if t.is_empty() || has_control(t, false) {
            return Err("invalid");
        }
        if t.chars().count() > MAX_TAG {
            return Err("too_long");
        }
        if !out.iter().any(|x| x == t) {
            out.push(t.to_owned());
        }
    }
    if out.len() > MAX_TAGS {
        return Err("too_many");
    }
    Ok(out)
}

fn string_of<'a>(v: &'a Value, name: &str, e: &mut Errors) -> Option<&'a str> {
    match v {
        Value::String(s) => Some(s),
        _ => {
            e.set(name, "invalid");
            None
        }
    }
}

fn tags_of(v: &Value, e: &mut Errors) -> Option<Vec<String>> {
    let Value::Array(items) = v else {
        e.set("tags", "invalid");
        return None;
    };
    let mut raw = Vec::with_capacity(items.len());
    for i in items {
        match i {
            Value::String(s) => raw.push(s.clone()),
            _ => {
                e.set("tags", "invalid");
                return None;
            }
        }
    }
    match tags(&raw) {
        Ok(t) => Some(t),
        Err(code) => {
            e.set("tags", code);
            None
        }
    }
}

/// `NewBookmark`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBookmark {
    /// URL.
    pub url: String,
    /// Title.
    pub title: String,
    /// Description.
    pub description: String,
    /// Tags.
    pub tags: Vec<String>,
}

/// `BookmarkPatch`: the fields to change.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Patch {
    /// New URL.
    pub url: Option<String>,
    /// New title.
    pub title: Option<String>,
    /// New description.
    pub description: Option<String>,
    /// The new tag set.
    pub tags: Option<Vec<String>>,
}

impl Patch {
    /// The names of the fields given (audit details).
    pub fn names(&self) -> Vec<&'static str> {
        let mut n = Vec::new();
        if self.url.is_some() {
            n.push("url");
        }
        if self.title.is_some() {
            n.push("title");
        }
        if self.description.is_some() {
            n.push("description");
        }
        if self.tags.is_some() {
            n.push("tags");
        }
        n
    }
}

const BOOKMARK_FIELDS: [&str; 4] = ["url", "title", "description", "tags"];

fn decode_fields(obj: &Map<String, Value>, e: &mut Errors) -> Patch {
    unknown_keys(obj, &BOOKMARK_FIELDS, e);
    let mut p = Patch::default();
    if let Some(v) = obj.get("url")
        && let Some(s) = string_of(v, "url", e)
    {
        match url(s) {
            Ok(u) => p.url = Some(u),
            Err(c) => e.set("url", c),
        }
    }
    for (name, max, multiline) in [
        ("title", MAX_TITLE, false),
        ("description", MAX_DESCRIPTION, true),
    ] {
        if let Some(v) = obj.get(name)
            && !v.is_null()
            && let Some(s) = string_of(v, name, e)
        {
            match text(s, max, multiline) {
                Ok(t) if name == "title" => p.title = Some(t),
                Ok(t) => p.description = Some(t),
                Err(c) => e.set(name, c),
            }
        }
    }
    if let Some(v) = obj.get("tags")
        && !v.is_null()
    {
        p.tags = tags_of(v, e);
    }
    p
}

/// Decodes a `NewBookmark`.
pub fn new_bookmark(obj: &Map<String, Value>) -> Result<NewBookmark, ApiError> {
    let mut e = Errors::default();
    let p = decode_fields(obj, &mut e);
    if !obj.contains_key("url") {
        e.set("url", "required");
    }
    let nb = NewBookmark {
        url: p.url.unwrap_or_default(),
        title: p.title.unwrap_or_default(),
        description: p.description.unwrap_or_default(),
        tags: p.tags.unwrap_or_default(),
    };
    e.finish(nb)
}

/// Decodes a `BookmarkPatch` (at least one field).
pub fn patch(obj: &Map<String, Value>) -> Result<Patch, ApiError> {
    let mut e = Errors::default();
    let p = decode_fields(obj, &mut e);
    if obj.is_empty() {
        return Err(ApiError::validation_failed());
    }
    e.finish(p)
}

/// A bookmark ID in canonical form; anything else is no bookmark (`404`).
pub fn bookmark_id(s: &str) -> Option<Uuid> {
    Uuid::parse_str(s.trim()).ok()
}

/// What a grant, revoke or check names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target {
    /// Every bookmark of the tenant (`resource_id` omitted or empty).
    Collection,
    /// One bookmark.
    Bookmark(Uuid),
    /// An ID that names no bookmark (answered `404`, or `allowed: false`).
    Unknown,
}

impl Target {
    /// The stored `resource_id`.
    pub fn resource_id(self) -> String {
        match self {
            Self::Collection | Self::Unknown => String::new(),
            Self::Bookmark(id) => id.to_string(),
        }
    }
}

/// Reads `resource_type` (optional, `bookmark`) and `resource_id`.
fn target_of(obj: &Map<String, Value>, e: &mut Errors) -> Target {
    if let Some(v) = obj.get("resource_type")
        && v.as_str() != Some(RESOURCE)
    {
        e.set("resource_type", "invalid");
    }
    match obj.get("resource_id") {
        None | Some(Value::Null) => Target::Collection,
        Some(Value::String(s)) if s.is_empty() => Target::Collection,
        Some(Value::String(s)) => bookmark_id(s).map_or(Target::Unknown, Target::Bookmark),
        Some(_) => {
            e.set("resource_id", "invalid");
            Target::Unknown
        }
    }
}

/// The target named by a `resource_id` query parameter.
pub fn target_param(s: Option<&str>) -> Target {
    match s {
        None | Some("") => Target::Collection,
        Some(s) => bookmark_id(s).map_or(Target::Unknown, Target::Bookmark),
    }
}

/// A role slug as auth issues them.
fn role_slug(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= MAX_SUBJECT_ID
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b':' | b'-'))
        && s.as_bytes()[0].is_ascii_alphanumeric()
}

/// Checks a subject ID for its kind: a user ID is a UUID (stored in
/// canonical form), a role a slug, the tenant has none.
pub fn subject_id(st: SubjectType, s: &str) -> Result<String, &'static str> {
    let s = s.trim();
    match st {
        SubjectType::Tenant if s.is_empty() => Ok(String::new()),
        SubjectType::Tenant => Err("invalid"),
        _ if s.is_empty() => Err("required"),
        SubjectType::User => Uuid::parse_str(s).map(|u| u.to_string()).map_err(|_| "invalid"),
        SubjectType::Role if role_slug(s) => Ok(s.to_owned()),
        SubjectType::Role => Err("invalid"),
    }
}

/// `NewGrant`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewGrant {
    /// The bookmark (or the collection).
    pub target: Target,
    /// Subject kind.
    pub subject_type: SubjectType,
    /// Subject ID ("" for the tenant).
    pub subject_id: String,
    /// Relation.
    pub relation: Relation,
    /// Expiry (in the future).
    pub expires_at: Option<DateTime<Utc>>,
}

/// Decodes a `NewGrant`; the expiry must be after `now`.
pub fn new_grant(obj: &Map<String, Value>, now: DateTime<Utc>) -> Result<NewGrant, ApiError> {
    let mut e = Errors::default();
    unknown_keys(
        obj,
        &[
            "resource_type",
            "resource_id",
            "subject_type",
            "subject_id",
            "relation",
            "expires_at",
        ],
        &mut e,
    );
    let target = target_of(obj, &mut e);
    let subject_type = match obj.get("subject_type").map(Value::as_str) {
        None => {
            e.set("subject_type", "required");
            None
        }
        Some(s) => {
            let st = s.and_then(SubjectType::parse);
            if st.is_none() {
                e.set("subject_type", "invalid");
            }
            st
        }
    };
    let relation = match obj.get("relation").map(Value::as_str) {
        None => {
            e.set("relation", "required");
            None
        }
        Some(s) => {
            let r = s.and_then(Relation::parse);
            if r.is_none() {
                e.set("relation", "invalid");
            }
            r
        }
    };
    let mut subject = String::new();
    if let Some(st) = subject_type {
        let raw = match obj.get("subject_id") {
            None | Some(Value::Null) => Some(""),
            Some(Value::String(s)) => Some(s.as_str()),
            Some(_) => None,
        };
        match raw.map(|r| subject_id(st, r)) {
            Some(Ok(s)) => subject = s,
            Some(Err(c)) => e.set("subject_id", c),
            None => e.set("subject_id", "invalid"),
        }
        if target == Target::Collection && st != SubjectType::Tenant {
            // Only the tenant subject can hold a grant on every bookmark.
            e.set("resource_id", "required");
        }
    }
    let expires_at = match obj.get("expires_at") {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => match DateTime::parse_from_rfc3339(s) {
            Ok(t) if t.with_timezone(&Utc) > now => Some(t.with_timezone(&Utc)),
            Ok(_) => {
                e.set("expires_at", "past");
                None
            }
            Err(_) => {
                e.set("expires_at", "invalid");
                None
            }
        },
        Some(_) => {
            e.set("expires_at", "invalid");
            None
        }
    };
    match (subject_type, relation) {
        (Some(subject_type), Some(relation)) => e.finish(NewGrant {
            target,
            subject_type,
            subject_id: subject,
            relation,
            expires_at,
        }),
        _ => Err(fields(e.0)),
    }
}

/// `RevokeRequest`: (target, grant ID).
pub fn revoke(obj: &Map<String, Value>) -> Result<(Target, Option<Uuid>), ApiError> {
    let mut e = Errors::default();
    unknown_keys(obj, &["resource_type", "resource_id", "id"], &mut e);
    let target = target_of(obj, &mut e);
    let id = match obj.get("id") {
        None => {
            e.set("id", "required");
            None
        }
        // A malformed grant ID names no grant: 404 like an unknown one.
        Some(Value::String(s)) => Uuid::parse_str(s).ok(),
        Some(_) => {
            e.set("id", "invalid");
            None
        }
    };
    e.finish((target, id))
}

/// `CheckRequest`: (target, action).
pub fn check(obj: &Map<String, Value>) -> Result<(Target, Action), ApiError> {
    let mut e = Errors::default();
    unknown_keys(obj, &["resource_type", "resource_id", "permission"], &mut e);
    let target = target_of(obj, &mut e);
    let action = match obj.get("permission").map(Value::as_str) {
        None => {
            e.set("permission", "required");
            None
        }
        Some(s) => {
            let a = s.and_then(Action::parse);
            if a.is_none() {
                e.set("permission", "invalid");
            }
            a
        }
    };
    match action {
        Some(a) => e.finish((target, a)),
        None => Err(fields(e.0)),
    }
}

/// Every value of `key` in a raw query string (`url.Values[key]`).
pub fn query_all(raw: &str, key: &str) -> Vec<String> {
    url::form_urlencoded::parse(raw.as_bytes())
        .filter(|(k, _)| k == key)
        .map(|(_, v)| v.into_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(v: Value) -> Map<String, Value> {
        match v {
            Value::Object(o) => o,
            _ => panic!("not an object"),
        }
    }

    fn codes(e: &ApiError) -> Value {
        serde_json::from_str::<Value>(&e.body()).unwrap()["detail"]["fields"].clone()
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn urls() {
        assert_eq!(url("  https://go.dev/doc#x ").unwrap(), "https://go.dev/doc#x");
        assert_eq!(
            url("http://127.0.0.1:8080/a?b=c").unwrap(),
            "http://127.0.0.1:8080/a?b=c"
        );
        for (bad, code) in [
            ("", "required"),
            ("javascript:alert(1)", "scheme"),
            ("data:text/html,x", "scheme"),
            ("ftp://x.org/", "scheme"),
            ("https://", "invalid"),
            ("go.dev", "invalid"),
            ("https://a b.org", "invalid"),
            ("https://x.org/\u{7}", "invalid"),
        ] {
            assert_eq!(url(bad), Err(code), "{bad}");
        }
        let long = format!("https://x.org/{}", "a".repeat(MAX_URL));
        assert_eq!(url(&long), Err("too_long"));
    }

    #[test]
    fn tag_rules() {
        let t = |v: &[&str]| tags(&v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(t(&[" go ", "docs", "go"]).unwrap(), ["go", "docs"]);
        assert_eq!(t(&[""]), Err("invalid"));
        assert_eq!(t(&["a\u{0}"]), Err("invalid"));
        assert_eq!(t(&[&"x".repeat(MAX_TAG + 1)]), Err("too_long"));
        let many: Vec<String> = (0..=MAX_TAGS).map(|i| format!("t{i}")).collect();
        assert_eq!(tags(&many), Err("too_many"));
    }

    #[test]
    fn new_bookmark_validation() {
        let b = new_bookmark(&obj(
            json!({"url": "https://go.dev", "title": " Go ", "tags": ["go"]}),
        ))
        .unwrap();
        assert_eq!((b.title.as_str(), b.tags.len()), ("Go", 1));
        let e = new_bookmark(&obj(json!({"title": 1, "x": true}))).unwrap_err();
        assert_eq!(e.status(), 422);
        assert_eq!(
            codes(&e),
            json!({"url": "required", "title": "invalid", "x": "unknown"})
        );
        let e = new_bookmark(&obj(json!({"url": "javascript:x", "description": "a\nb"}))).unwrap_err();
        assert_eq!(codes(&e), json!({"url": "scheme"}));
        let e = new_bookmark(&obj(json!({"url": "https://x.org", "title": "t".repeat(501)}))).unwrap_err();
        assert_eq!(codes(&e), json!({"title": "too_long"}));
        assert!(new_bookmark(&obj(json!({"url": "https://x.org", "tags": "go"}))).is_err());
    }

    #[test]
    fn patch_validation() {
        assert!(patch(&Map::new()).is_err(), "at least one field");
        let p = patch(&obj(json!({"tags": []}))).unwrap();
        assert_eq!(p.tags, Some(vec![]));
        assert_eq!(p.names(), ["tags"]);
        assert!(patch(&obj(json!({"url": ""}))).is_err());
    }

    #[test]
    fn grant_validation() {
        let u = "0192f6b0-0000-7000-8000-00000000000a";
        let g = new_grant(
            &obj(json!({"resource_id": u, "subject_type": "user", "subject_id": u.to_uppercase(), "relation": "viewer"})),
            now(),
        )
        .unwrap();
        assert_eq!(g.subject_id, u, "canonical user ID");
        assert_eq!(g.target, Target::Bookmark(Uuid::parse_str(u).unwrap()));
        let g = new_grant(
            &obj(json!({"subject_type": "tenant", "relation": "viewer"})),
            now(),
        )
        .unwrap();
        assert_eq!((g.target, g.subject_id.as_str()), (Target::Collection, ""));
        let e = new_grant(
            &obj(json!({"subject_type": "role", "subject_id": "dev", "relation": "viewer"})),
            now(),
        )
        .unwrap_err();
        assert_eq!(codes(&e), json!({"resource_id": "required"}));
        let e = new_grant(
            &obj(json!({"resource_id": u, "subject_type": "user", "subject_id": "bob", "relation": "boss"})),
            now(),
        )
        .unwrap_err();
        assert_eq!(codes(&e), json!({"relation": "invalid", "subject_id": "invalid"}));
        let e = new_grant(
            &obj(json!({"resource_id": u, "subject_type": "tenant", "subject_id": "all", "relation": "viewer"})),
            now(),
        )
        .unwrap_err();
        assert_eq!(codes(&e), json!({"subject_id": "invalid"}));
        let e = new_grant(
            &obj(json!({"resource_id": u, "subject_type": "role", "subject_id": "dev", "relation": "viewer", "expires_at": "2020-01-01T00:00:00Z"})),
            now(),
        )
        .unwrap_err();
        assert_eq!(codes(&e), json!({"expires_at": "past"}));
        let g = new_grant(
            &obj(json!({"resource_id": "nope", "subject_type": "role", "subject_id": "dev", "relation": "viewer"})),
            now(),
        )
        .unwrap();
        assert_eq!(g.target, Target::Unknown);
        assert!(
            new_grant(
                &obj(json!({"resource_type": "document", "subject_type": "tenant", "relation": "viewer"})),
                now()
            )
            .is_err()
        );
        assert!(subject_id(SubjectType::Role, "-x").is_err());
        assert_eq!(
            subject_id(SubjectType::Role, "bookmark.viewer").unwrap(),
            "bookmark.viewer"
        );
    }

    #[test]
    fn revoke_and_check() {
        let (t, id) = revoke(&obj(json!({"id": "x"}))).unwrap();
        assert_eq!((t, id), (Target::Collection, None));
        assert!(revoke(&obj(json!({}))).is_err());
        let (_, a) = check(&obj(json!({"resource_id": "", "permission": "share"}))).unwrap();
        assert_eq!(a, Action::Share);
        assert!(check(&obj(json!({"permission": "download"}))).is_err());
    }

    #[test]
    fn query_values() {
        assert_eq!(query_all("tag=a&tag=b%20c&x=1", "tag"), ["a", "b c"]);
        assert_eq!(target_param(Some("")), Target::Collection);
        assert_eq!(target_param(Some("x")), Target::Unknown);
    }
}
