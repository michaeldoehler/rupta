//! Who may do what: permissions, the role and token scopes each needs, and
//! the single decision function used by every endpoint.
//!
//! A request acts as a [`Principal`]: a user signed in with a session, a
//! user's API token, or an organization token. Every endpoint names one
//! [`Permission`]. [`authorize`] decides:
//!
//! - sessions need the permission's minimum role in the organization;
//! - user tokens need that role *and* one of the permission's scopes;
//! - organization tokens need one of the scopes, work only in their own
//!   organization, and never have more than the admin role.
//!
//! Superusers act as owners of every organization.
//!
//! Callers that are not members get [`Denied::NotFound`], so that the
//! existence of other organizations' resources does not leak; members
//! without enough rights get [`Denied::Forbidden`].

use rupta_core::role::Role;
use serde::Serialize;

/// Every scope an API token can carry.
pub const SCOPES: &[(&str, &str)] = &[
    ("org:read", "Read the organization, its projects, issues, events and releases"),
    ("org:write", "Change organization settings, teams and notification channels"),
    ("org:admin", "Manage organization tokens and read the audit log"),
    ("member:read", "List members and teams"),
    ("member:write", "Invite, change and remove members"),
    ("project:read", "Read projects, issues and events"),
    ("project:write", "Change projects: settings, keys, alert rules"),
    ("project:admin", "Create and delete projects"),
    ("project:releases", "Upload source maps and create releases (upload CLI)"),
    ("issue:write", "Resolve, ignore, assign and comment on issues"),
    ("release:write", "Create and change releases and deploys"),
];

/// True if `scope` is a known scope.
pub fn is_scope(scope: &str) -> bool {
    SCOPES.iter().any(|(s, _)| *s == scope)
}

/// An action that needs authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read the organization and anything in it.
    ReadOrg,
    /// Read projects, issues, events, releases.
    ReadProject,
    /// Change issue status, assignment, bookmarks are per-user; comments.
    TriageIssues,
    /// Delete issues.
    DeleteIssues,
    /// Create releases and deploys.
    WriteReleases,
    /// Upload artifact bundles (source maps).
    UploadArtifacts,
    /// Change project settings, keys and alert rules.
    WriteProject,
    /// Create projects.
    CreateProject,
    /// Delete projects.
    DeleteProject,
    /// Teams.
    ManageTeams,
    /// List members.
    ReadMembers,
    /// Invite, change, remove members.
    ManageMembers,
    /// Notification channels.
    ManageChannels,
    /// Rename the organization, change its settings.
    ManageOrg,
    /// Delete the organization.
    DeleteOrg,
    /// Configure single sign-on.
    ManageSso,
    /// Create and delete organization tokens.
    ManageOrgTokens,
    /// Read the audit log.
    ReadAuditLog,
}

impl Permission {
    pub const ALL: &'static [Permission] = &[
        Permission::ReadOrg,
        Permission::ReadProject,
        Permission::TriageIssues,
        Permission::DeleteIssues,
        Permission::WriteReleases,
        Permission::UploadArtifacts,
        Permission::WriteProject,
        Permission::CreateProject,
        Permission::DeleteProject,
        Permission::ManageTeams,
        Permission::ReadMembers,
        Permission::ManageMembers,
        Permission::ManageChannels,
        Permission::ManageOrg,
        Permission::DeleteOrg,
        Permission::ManageSso,
        Permission::ManageOrgTokens,
        Permission::ReadAuditLog,
    ];

    /// The lowest role that has this permission.
    pub fn role(self) -> Role {
        use Permission::*;
        match self {
            ReadOrg | ReadProject | ReadMembers => Role::Viewer,
            TriageIssues | WriteReleases | UploadArtifacts => Role::Member,
            DeleteIssues | WriteProject | CreateProject | DeleteProject | ManageTeams | ManageMembers
            | ManageChannels | ManageOrg | ManageOrgTokens | ReadAuditLog => Role::Admin,
            DeleteOrg | ManageSso => Role::Owner,
        }
    }

    /// Token scopes that grant this permission (any one suffices).
    pub fn scopes(self) -> &'static [&'static str] {
        use Permission::*;
        match self {
            ReadOrg => &["org:read", "org:write", "org:admin"],
            ReadProject => &["project:read", "project:write", "project:admin", "org:read"],
            TriageIssues => &["issue:write", "project:write"],
            DeleteIssues => &["project:write", "project:admin"],
            WriteReleases => &["release:write", "project:releases", "project:write"],
            UploadArtifacts => &["project:releases", "project:write"],
            WriteProject => &["project:write", "project:admin"],
            CreateProject | DeleteProject => &["project:admin"],
            ManageTeams | ManageChannels | ManageOrg => &["org:write", "org:admin"],
            ReadMembers => &["member:read", "member:write", "org:read"],
            ManageMembers => &["member:write"],
            DeleteOrg | ManageSso | ManageOrgTokens | ReadAuditLog => &["org:admin"],
        }
    }
}

/// An API token's grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenGrant {
    pub id: i64,
    /// Set for organization tokens.
    pub org_id: Option<i64>,
    pub scopes: Vec<String>,
}

/// Who is making a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    /// The user; `None` for organization tokens.
    pub user_id: Option<i64>,
    pub is_superuser: bool,
    /// Set when authenticated with an API token.
    pub token: Option<TokenGrant>,
    /// SHA-256 of the session token, when authenticated with a session.
    pub session_id: Option<String>,
}

impl Principal {
    pub fn has_scope(&self, scopes: &[&str]) -> bool {
        match &self.token {
            None => true,
            Some(t) => t.scopes.iter().any(|s| scopes.contains(&s.as_str())),
        }
    }
}

/// Why access was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Denied {
    /// Not a member (or another organization's token): answer 404.
    NotFound,
    /// A member without enough rights, or a token without the scope: 403.
    Forbidden,
}

/// The role organization tokens act with at most.
pub const ORG_TOKEN_ROLE: Role = Role::Admin;

/// Decides whether `principal` may use `perm` in organization `org_id`,
/// where `membership` is the principal's role there (if a member).
/// Returns the effective role.
pub fn authorize(
    principal: &Principal,
    org_id: i64,
    membership: Option<Role>,
    perm: Permission,
) -> Result<Role, Denied> {
    let role = match (&principal.token, principal.user_id) {
        (Some(TokenGrant { org_id: Some(token_org), .. }), _) => {
            if *token_org != org_id {
                return Err(Denied::NotFound);
            }
            ORG_TOKEN_ROLE
        }
        (_, Some(_)) => {
            if principal.is_superuser {
                Role::Owner
            } else {
                membership.ok_or(Denied::NotFound)?
            }
        }
        (_, None) => return Err(Denied::NotFound),
    };
    if !role.at_least(perm.role()) || !principal.has_scope(perm.scopes()) {
        return Err(Denied::Forbidden);
    }
    Ok(role)
}

/// Roles `actor` may hand out or take away: owners manage everyone,
/// admins everyone except owners, others nobody.
pub fn may_manage_role(actor: Role, target: Role) -> bool {
    match actor {
        Role::Owner => true,
        Role::Admin => target != Role::Owner,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(user: i64) -> Principal {
        Principal { user_id: Some(user), is_superuser: false, token: None, session_id: Some("s".into()) }
    }

    #[test]
    fn role_matrix_for_sessions() {
        // (permission, viewer, member, admin, owner)
        let matrix: &[(Permission, [bool; 4])] = &[
            (Permission::ReadOrg, [true, true, true, true]),
            (Permission::ReadProject, [true, true, true, true]),
            (Permission::ReadMembers, [true, true, true, true]),
            (Permission::TriageIssues, [false, true, true, true]),
            (Permission::WriteReleases, [false, true, true, true]),
            (Permission::UploadArtifacts, [false, true, true, true]),
            (Permission::DeleteIssues, [false, false, true, true]),
            (Permission::WriteProject, [false, false, true, true]),
            (Permission::CreateProject, [false, false, true, true]),
            (Permission::DeleteProject, [false, false, true, true]),
            (Permission::ManageTeams, [false, false, true, true]),
            (Permission::ManageMembers, [false, false, true, true]),
            (Permission::ManageChannels, [false, false, true, true]),
            (Permission::ManageOrg, [false, false, true, true]),
            (Permission::ManageOrgTokens, [false, false, true, true]),
            (Permission::ReadAuditLog, [false, false, true, true]),
            (Permission::DeleteOrg, [false, false, false, true]),
            (Permission::ManageSso, [false, false, false, true]),
        ];
        assert_eq!(matrix.len(), Permission::ALL.len(), "every permission is in the matrix");
        let roles = [Role::Viewer, Role::Member, Role::Admin, Role::Owner];
        for (perm, allowed) in matrix {
            for (role, ok) in roles.iter().zip(allowed) {
                let got = authorize(&session(1), 9, Some(*role), *perm);
                if *ok {
                    assert_eq!(got, Ok(*role), "{perm:?} as {role:?}");
                } else {
                    assert_eq!(got, Err(Denied::Forbidden), "{perm:?} as {role:?}");
                }
            }
            assert_eq!(authorize(&session(1), 9, None, *perm), Err(Denied::NotFound), "{perm:?} as non-member");
        }
    }

    #[test]
    fn superusers_act_as_owners() {
        let p = Principal { is_superuser: true, ..session(1) };
        assert_eq!(authorize(&p, 9, None, Permission::DeleteOrg), Ok(Role::Owner));
    }

    #[test]
    fn user_tokens_need_role_and_scope() {
        let p = Principal {
            token: Some(TokenGrant { id: 1, org_id: None, scopes: vec!["project:read".into()] }),
            session_id: None,
            ..session(1)
        };
        assert!(authorize(&p, 9, Some(Role::Owner), Permission::ReadProject).is_ok());
        assert_eq!(authorize(&p, 9, Some(Role::Owner), Permission::TriageIssues), Err(Denied::Forbidden));
        assert_eq!(authorize(&p, 9, None, Permission::ReadProject), Err(Denied::NotFound));
        let p = Principal {
            token: Some(TokenGrant { id: 1, org_id: None, scopes: vec!["issue:write".into()] }),
            ..session(1)
        };
        assert_eq!(authorize(&p, 9, Some(Role::Viewer), Permission::TriageIssues), Err(Denied::Forbidden));
    }

    #[test]
    fn org_tokens_stay_in_their_org_and_below_owner() {
        let p = Principal {
            user_id: None,
            is_superuser: false,
            token: Some(TokenGrant {
                id: 1,
                org_id: Some(9),
                scopes: vec!["project:releases".into(), "org:admin".into()],
            }),
            session_id: None,
        };
        assert_eq!(authorize(&p, 9, None, Permission::UploadArtifacts), Ok(Role::Admin));
        assert_eq!(authorize(&p, 10, None, Permission::UploadArtifacts), Err(Denied::NotFound));
        assert_eq!(authorize(&p, 9, None, Permission::TriageIssues), Err(Denied::Forbidden));
        assert_eq!(authorize(&p, 9, None, Permission::ManageSso), Err(Denied::Forbidden));
        assert_eq!(authorize(&p, 9, None, Permission::ReadAuditLog), Ok(Role::Admin));
    }

    #[test]
    fn role_management() {
        assert!(may_manage_role(Role::Owner, Role::Owner));
        assert!(may_manage_role(Role::Admin, Role::Admin));
        assert!(!may_manage_role(Role::Admin, Role::Owner));
        assert!(!may_manage_role(Role::Member, Role::Viewer));
    }

    #[test]
    fn every_permission_scope_exists() {
        for p in Permission::ALL {
            for s in p.scopes() {
                assert!(is_scope(s), "{s}");
            }
        }
    }
}
