-- API and authentication additions.

-- Owners may still log in with a password when their organization enforces
-- single sign-on (break-glass access when the identity provider is down).
ALTER TABLE sso_configs ADD COLUMN owner_local_login boolean NOT NULL DEFAULT true;

-- Comments are issue activity of kind 'comment'; editing needs a timestamp.
ALTER TABLE issue_activity ADD COLUMN updated_at timestamptz;

CREATE INDEX alert_rules_project ON alert_rules (project_id);
CREATE INDEX notification_channels_org ON notification_channels (org_id);
CREATE INDEX saved_searches_org ON saved_searches (org_id);
CREATE INDEX monitors_project ON monitors (project_id);
CREATE INDEX api_tokens_user ON api_tokens (user_id);
CREATE INDEX api_tokens_org ON api_tokens (org_id);
CREATE INDEX identities_user ON identities (user_id);
CREATE INDEX invitations_org ON invitations (org_id);
CREATE INDEX issue_bookmarks_user ON issue_bookmarks (user_id);
CREATE INDEX issues_assignee_user ON issues (assignee_user_id);
CREATE INDEX release_projects_project ON release_projects (project_id);
CREATE INDEX deploys_release ON deploys (release_id);
CREATE INDEX artifact_bundle_projects_project ON artifact_bundle_projects (project_id);
