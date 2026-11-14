-- Issue tracker integrations (Jira Cloud, Jira Data Center, Linear,
-- GitHub, GitLab), links between rupta issues and tickets, and incidents
-- opened in paging services (PagerDuty, Opsgenie) so that they can be
-- resolved when the rupta issue is.

-- One connection to a tracker, per organization. `config` holds the
-- non-secret settings (base URL, account email, transition names);
-- `secret_enc` the API token and `webhook_secret_enc` the secret that
-- incoming webhooks are verified with, both encrypted with the instance
-- key and bound to the row id.
CREATE TABLE integrations (
    id                  bigserial PRIMARY KEY,
    org_id              bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    kind                text NOT NULL,      -- jira_cloud, jira_server, linear, github, gitlab
    name                text NOT NULL,
    config              jsonb NOT NULL DEFAULT '{}',
    secret_enc          text,
    webhook_secret_enc  text,
    enabled             boolean NOT NULL DEFAULT true,
    created_by          bigint REFERENCES users (id) ON DELETE SET NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    updated_at          timestamptz NOT NULL DEFAULT now(),
    -- Last error of an outbound call or webhook, for the settings page.
    last_error          text,
    last_error_at       timestamptz
);
CREATE INDEX integrations_org ON integrations (org_id);

-- Per project: where new tickets go and which way statuses sync.
CREATE TABLE project_integration_defaults (
    project_id           bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    integration_id       bigint NOT NULL REFERENCES integrations (id) ON DELETE CASCADE,
    -- Jira project key, Linear team id, GitHub `owner/repo`, GitLab project path.
    target               text NOT NULL,
    target_name          text,
    -- Jira issue type (id or name).
    issue_type           text,
    -- Linear project id (optional).
    project              text,
    labels               jsonb NOT NULL DEFAULT '[]',
    -- Tracker -> rupta: resolve the issue when its ticket is done, unresolve when reopened.
    incoming_sync        boolean NOT NULL DEFAULT false,
    -- rupta -> tracker: transition/close/reopen the ticket on resolve/unresolve.
    outgoing_transition  boolean NOT NULL DEFAULT false,
    -- rupta -> tracker: comment on resolve, unresolve and regression.
    outgoing_comment     boolean NOT NULL DEFAULT false,
    updated_at           timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (project_id, integration_id)
);
CREATE INDEX project_integration_defaults_integration ON project_integration_defaults (integration_id);

CREATE TABLE issue_links (
    id               bigserial PRIMARY KEY,
    issue_id         bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    integration_id   bigint NOT NULL REFERENCES integrations (id) ON DELETE CASCADE,
    -- The tracker's stable id: Jira issue id, Linear issue UUID, GitHub
    -- `owner/repo#n` and GitLab `group/project#n` (lower case).
    external_id      text NOT NULL,
    -- What people call it: `PROJ-123`, `ENG-42`, `#17`.
    external_key     text NOT NULL,
    url              text NOT NULL,
    title            text,
    status           text,
    status_category  text NOT NULL DEFAULT 'open',   -- open, done
    assignee         text,
    created_by       bigint REFERENCES users (id) ON DELETE SET NULL,
    created_at       timestamptz NOT NULL DEFAULT now(),
    synced_at        timestamptz,
    -- Loop protection: the status category rupta last pushed to the
    -- tracker and the end of the quiet period in which webhooks about
    -- the ticket do not change the rupta issue.
    pushed_category  text,
    pushed_until     timestamptz,
    UNIQUE (issue_id, integration_id, external_id)
);
CREATE INDEX issue_links_external ON issue_links (integration_id, external_id);
CREATE INDEX issue_links_issue ON issue_links (issue_id);

-- Webhook deliveries already processed (by the tracker's delivery id),
-- so that redeliveries are answered without effect. Pruned after 7 days.
CREATE TABLE integration_deliveries (
    integration_id  bigint NOT NULL REFERENCES integrations (id) ON DELETE CASCADE,
    delivery_id     text NOT NULL,
    received_at     timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (integration_id, delivery_id)
);
CREATE INDEX integration_deliveries_received ON integration_deliveries (received_at);

-- Incidents opened in PagerDuty or Opsgenie for an issue through a
-- channel; resolved (closed) there when the rupta issue is resolved.
CREATE TABLE channel_incidents (
    issue_id     bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    channel_id   bigint NOT NULL REFERENCES notification_channels (id) ON DELETE CASCADE,
    dedup_key    text NOT NULL,
    opened_at    timestamptz NOT NULL DEFAULT now(),
    resolved_at  timestamptz,
    PRIMARY KEY (issue_id, channel_id)
);
