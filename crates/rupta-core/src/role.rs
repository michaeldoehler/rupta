//! Organization roles, from most to least privileged.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// Read-only access to issues, events and settings.
    Viewer,
    /// Works on issues: resolve, assign, comment; manages own tokens.
    Member,
    /// Manages projects, teams, keys, alerts and members (except owners).
    Admin,
    /// Everything, including SSO, billing-free org settings and deletion.
    Owner,
}

impl Role {
    pub fn parse(s: &str) -> Option<Role> {
        match s.trim().to_ascii_lowercase().as_str() {
            "viewer" => Some(Role::Viewer),
            "member" => Some(Role::Member),
            "admin" => Some(Role::Admin),
            "owner" => Some(Role::Owner),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Viewer => "viewer",
            Role::Member => "member",
            Role::Admin => "admin",
            Role::Owner => "owner",
        }
    }

    /// True if this role includes everything `other` may do.
    pub fn at_least(self, other: Role) -> bool {
        self >= other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_are_ordered_by_privilege() {
        assert!(Role::Owner.at_least(Role::Admin));
        assert!(Role::Member.at_least(Role::Viewer));
        assert!(!Role::Viewer.at_least(Role::Member));
        assert_eq!(Role::parse(" Admin "), Some(Role::Admin));
        assert_eq!(Role::parse("root"), None);
    }
}
