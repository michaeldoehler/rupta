//! First start: the superuser from `RUPTA_ADMIN_EMAIL` and
//! `RUPTA_ADMIN_PASSWORD`.

use rupta_auth::audit::{self, action};
use rupta_auth::{password, users};

use crate::state::ApiState;

/// Creates the superuser, the organization `default_org` and the owner
/// membership if no user exists yet. Returns the new user's id, or `None`
/// when users exist (then nothing changes and the setup endpoint stays
/// closed).
pub async fn admin_from_env(
    state: &ApiState,
    email: &str,
    password_plain: &str,
    default_org: &str,
) -> Result<Option<i64>, String> {
    let email =
        users::normalize_email(email).ok_or_else(|| format!("RUPTA_ADMIN_EMAIL: invalid email address {email:?}"))?;
    if !users::valid_slug(default_org) {
        return Err(format!(
            "RUPTA_DEFAULT_ORG: {default_org:?} is not a valid slug (lowercase letters, digits, dashes)"
        ));
    }
    let existing = state.db(|c| Ok(users::count(c)?)).await.map_err(|e| e.message)?;
    if existing > 0 {
        return Ok(None);
    }
    password::check_policy(password_plain).map_err(|e| format!("RUPTA_ADMIN_PASSWORD: {e}"))?;
    let pw = password_plain.to_string();
    let hash = tokio::task::spawn_blocking(move || password::hash(&pw)).await.map_err(|e| e.to_string())?;
    let org = default_org.to_string();
    let created = state
        .db(move |c| {
            let b = users::bootstrap(c, &email, "Admin", &hash, &org, &org)?;
            if let Some(b) = &b {
                audit::record(
                    c,
                    &audit::Entry {
                        org_id: Some(b.org_id),
                        actor_user_id: Some(b.user_id),
                        action: action::SETUP,
                        target_type: Some("user"),
                        target_id: Some(b.user_id.to_string()),
                        data: serde_json::json!({ "via": "environment" }),
                        ..Default::default()
                    },
                )?;
            }
            Ok(b.map(|b| b.user_id))
        })
        .await
        .map_err(|e| e.message)?;
    if created.is_some() {
        tracing::info!(org = default_org, "created the administrator from RUPTA_ADMIN_EMAIL");
    }
    Ok(created)
}
