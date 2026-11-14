-- Tables written by ingestion and read or managed by the API.

CREATE TABLE releases (
    id             bigserial PRIMARY KEY,
    org_id         bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    version        text NOT NULL,
    ref            text,
    url            text,
    date_released  timestamptz,
    first_event    timestamptz,
    last_event     timestamptz,
    created_at     timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, version)
);

CREATE TABLE release_projects (
    release_id  bigint NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    project_id  bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    new_issues  bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (release_id, project_id)
);

CREATE TABLE deploys (
    id             bigserial PRIMARY KEY,
    release_id     bigint NOT NULL REFERENCES releases (id) ON DELETE CASCADE,
    environment    text NOT NULL,
    name           text,
    url            text,
    date_started   timestamptz,
    date_finished  timestamptz NOT NULL DEFAULT now()
);

-- Release health: session counts per project, release, environment and hour.
CREATE TABLE session_counts (
    project_id   bigint NOT NULL,
    release      text NOT NULL,
    environment  text NOT NULL,
    hour         timestamptz NOT NULL,
    total        bigint NOT NULL DEFAULT 0,
    errored      bigint NOT NULL DEFAULT 0,
    crashed      bigint NOT NULL DEFAULT 0,
    abnormal     bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (project_id, release, environment, hour)
);

-- Accepted and dropped items, for usage statistics (client reports and
-- server-side decisions).
CREATE TABLE outcomes (
    project_id  bigint NOT NULL,
    hour        timestamptz NOT NULL,
    category    text NOT NULL,      -- error, transaction, attachment, session, replay, profile, ...
    outcome     text NOT NULL,      -- accepted, filtered, rate_limited, invalid, client_discard, dropped_unsupported
    reason      text NOT NULL DEFAULT '',
    quantity    bigint NOT NULL DEFAULT 0,
    PRIMARY KEY (project_id, hour, category, outcome, reason)
);

CREATE TABLE user_feedback (
    id          bigserial PRIMARY KEY,
    project_id  bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    event_id    uuid,
    issue_id    bigint,
    name        text,
    email       text,
    message     text NOT NULL,
    url         text,
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX user_feedback_project ON user_feedback (project_id, id);
CREATE INDEX user_feedback_issue ON user_feedback (issue_id);

CREATE TABLE attachments (
    id            bigserial PRIMARY KEY,
    project_id    bigint NOT NULL,
    event_id      uuid NOT NULL,
    name          text NOT NULL,
    content_type  text,
    size          bigint NOT NULL,
    object_key    text NOT NULL,
    day           date NOT NULL,
    created_at    timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX attachments_event ON attachments (project_id, event_id);
CREATE INDEX attachments_day ON attachments (day);

-- Cron monitors and their check-ins.
CREATE TABLE monitors (
    id               bigserial PRIMARY KEY,
    project_id       bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    slug             text NOT NULL,
    name             text NOT NULL,
    schedule         jsonb NOT NULL DEFAULT '{}',   -- {type: crontab|interval, value, unit}
    timezone         text NOT NULL DEFAULT 'UTC',
    checkin_margin   integer,                       -- minutes
    max_runtime      integer,                       -- minutes
    status           text NOT NULL DEFAULT 'active', -- active, disabled
    health           text NOT NULL DEFAULT 'unknown', -- ok, missed, error, timeout, unknown
    last_checkin     timestamptz,
    next_checkin     timestamptz,
    created_at       timestamptz NOT NULL DEFAULT now(),
    UNIQUE (project_id, slug)
);

CREATE TABLE checkins (
    id           uuid PRIMARY KEY,
    monitor_id   bigint NOT NULL REFERENCES monitors (id) ON DELETE CASCADE,
    status       text NOT NULL,                     -- in_progress, ok, error, missed, timeout
    environment  text,
    duration_ms  bigint,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX checkins_monitor ON checkins (monitor_id, created_at);

-- Source map artifact bundles as uploaded by the upload CLI: the zip is
-- stored in the object store under `object_key`; the index maps debug
-- ids and release/dist/url to files inside a bundle.
CREATE TABLE artifact_bundles (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    checksum    text NOT NULL,                      -- SHA-1 of the zip
    release     text,
    dist        text,
    object_key  text NOT NULL,
    file_count  integer NOT NULL DEFAULT 0,
    size        bigint NOT NULL DEFAULT 0,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, checksum)
);

CREATE TABLE artifact_bundle_projects (
    bundle_id   bigint NOT NULL REFERENCES artifact_bundles (id) ON DELETE CASCADE,
    project_id  bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    PRIMARY KEY (bundle_id, project_id)
);

CREATE TABLE artifact_index (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL,
    bundle_id   bigint NOT NULL REFERENCES artifact_bundles (id) ON DELETE CASCADE,
    debug_id    uuid,                               -- from the file's debug-id header
    release     text,
    dist        text,
    url         text,                               -- e.g. ~/static/js/app.js
    file_path   text NOT NULL,                      -- path inside the zip
    file_type   text NOT NULL                       -- minified_source, source_map, source
);
CREATE INDEX artifact_index_debug_id ON artifact_index (org_id, debug_id);
CREATE INDEX artifact_index_url ON artifact_index (org_id, release, url);

CREATE TABLE file_chunks (
    checksum    text PRIMARY KEY,                   -- SHA-1 of the chunk
    object_key  text NOT NULL,
    size        bigint NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now()
);

-- Alerting.
CREATE TABLE notification_channels (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    kind        text NOT NULL,                      -- email, slack, webhook, teams
    name        text NOT NULL,
    config      jsonb NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE alert_rules (
    id                 bigserial PRIMARY KEY,
    project_id         bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    name               text NOT NULL,
    enabled            boolean NOT NULL DEFAULT true,
    conditions         jsonb NOT NULL DEFAULT '[]',
    filters            jsonb NOT NULL DEFAULT '[]',
    actions            jsonb NOT NULL DEFAULT '[]',
    frequency_minutes  integer NOT NULL DEFAULT 30,
    environment        text,
    created_by         bigint REFERENCES users (id) ON DELETE SET NULL,
    created_at         timestamptz NOT NULL DEFAULT now(),
    updated_at         timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE alert_fires (
    rule_id   bigint NOT NULL REFERENCES alert_rules (id) ON DELETE CASCADE,
    issue_id  bigint NOT NULL,
    fired_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (rule_id, issue_id, fired_at)
);

CREATE TABLE saved_searches (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id     bigint REFERENCES users (id) ON DELETE CASCADE,  -- NULL: shared with the org
    name        text NOT NULL,
    query       text NOT NULL,
    sort        text NOT NULL DEFAULT 'last_seen',
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE issue_bookmarks (
    issue_id  bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    user_id   bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (issue_id, user_id)
);

CREATE TABLE issue_seen (
    issue_id  bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    user_id   bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    seen_at   timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (issue_id, user_id)
);
