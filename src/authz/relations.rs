/// Relation represents a permission level in the Zanzibar-like authorization system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Relation {
    Owner,
    Editor,
    Viewer,
    Sharer,
}

impl Relation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Owner => "RELATION_OWNER",
            Self::Editor => "RELATION_EDITOR",
            Self::Viewer => "RELATION_VIEWER",
            Self::Sharer => "RELATION_SHARER",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "RELATION_OWNER" => Some(Self::Owner),
            "RELATION_EDITOR" => Some(Self::Editor),
            "RELATION_VIEWER" => Some(Self::Viewer),
            "RELATION_SHARER" => Some(Self::Sharer),
            _ => None,
        }
    }

    pub fn from_proto(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Owner),
            2 => Some(Self::Editor),
            3 => Some(Self::Viewer),
            4 => Some(Self::Sharer),
            _ => None,
        }
    }

    pub fn to_proto(self) -> i32 {
        match self {
            Self::Owner => 1,
            Self::Editor => 2,
            Self::Viewer => 3,
            Self::Sharer => 4,
        }
    }

    /// Hierarchy level (higher = more permissions).
    pub fn hierarchy_level(self) -> u8 {
        match self {
            Self::Owner => 4,
            Self::Editor => 3,
            Self::Sharer => 2,
            Self::Viewer => 1,
        }
    }

    /// Check if this relation has at least as many permissions as another.
    pub fn is_at_least(self, other: Relation) -> bool {
        self.hierarchy_level() >= other.hierarchy_level()
    }

    /// Returns all permissions granted by this relation.
    pub fn granted_permissions(self) -> &'static [Permission] {
        match self {
            Self::Owner => &[Permission::Read, Permission::Write, Permission::Delete, Permission::Share],
            Self::Editor => &[Permission::Read, Permission::Write],
            Self::Viewer => &[Permission::Read],
            Self::Sharer => &[Permission::Read, Permission::Share],
        }
    }

    /// Check if this relation grants a specific permission.
    pub fn grants(self, perm: Permission) -> bool {
        self.granted_permissions().contains(&perm)
    }
}

/// Permission represents an action that can be performed on a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permission {
    Read,
    Write,
    Delete,
    Share,
}

impl Permission {
    pub fn from_proto(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Read),
            2 => Some(Self::Write),
            3 => Some(Self::Delete),
            4 => Some(Self::Share),
            _ => None,
        }
    }

    pub fn to_proto(self) -> i32 {
        match self {
            Self::Read => 1,
            Self::Write => 2,
            Self::Delete => 3,
            Self::Share => 4,
        }
    }

    pub const ALL: &[Permission] = &[
        Permission::Read,
        Permission::Write,
        Permission::Delete,
        Permission::Share,
    ];
}

/// Resource types that can be protected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResourceType {
    Bookmark,
}

impl ResourceType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Bookmark => "RESOURCE_TYPE_BOOKMARK",
        }
    }

    pub fn from_proto(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::Bookmark),
            _ => None,
        }
    }

    pub fn to_proto(self) -> i32 {
        match self {
            Self::Bookmark => 1,
        }
    }
}

/// Subject types that can be granted access.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SubjectType {
    User,
    Role,
    Tenant,
}

impl SubjectType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::User => "SUBJECT_TYPE_USER",
            Self::Role => "SUBJECT_TYPE_ROLE",
            Self::Tenant => "SUBJECT_TYPE_TENANT",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "SUBJECT_TYPE_USER" => Some(Self::User),
            "SUBJECT_TYPE_ROLE" => Some(Self::Role),
            "SUBJECT_TYPE_TENANT" => Some(Self::Tenant),
            _ => None,
        }
    }

    pub fn from_proto(v: i32) -> Option<Self> {
        match v {
            1 => Some(Self::User),
            2 => Some(Self::Role),
            3 => Some(Self::Tenant),
            _ => None,
        }
    }

    pub fn to_proto(self) -> i32 {
        match self {
            Self::User => 1,
            Self::Role => 2,
            Self::Tenant => 3,
        }
    }
}

/// Get the highest relation from a list.
pub fn get_highest_relation(relations: &[Relation]) -> Option<Relation> {
    relations
        .iter()
        .copied()
        .max_by_key(|r| r.hierarchy_level())
}
