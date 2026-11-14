-- Issues (row store) and events (columnar).
CREATE TABLE issues (
    id                   bigserial PRIMARY KEY,
    project_id           bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    short_id             bigint NOT NULL,
    fingerprint_hash     text NOT NULL,
    grouping_version     text NOT NULL,
    title                text NOT NULL,
    culprit              text,
    type                 text NOT NULL DEFAULT 'error',
    level                text NOT NULL DEFAULT 'error',
    platform             text,
    status               text NOT NULL DEFAULT 'unresolved',
    substatus            text NOT NULL DEFAULT 'new',
    priority             text NOT NULL DEFAULT 'medium',
    first_seen           timestamptz NOT NULL,
    last_seen            timestamptz NOT NULL,
    times_seen           bigint NOT NULL DEFAULT 0,
    users_seen           bigint NOT NULL DEFAULT 0,
    first_release        text,
    last_release         text,
    resolved_in_release  text,
    resolved_at          timestamptz,
    ignore_until         timestamptz,
    ignore_count         bigint,
    assignee_user_id     bigint REFERENCES users (id) ON DELETE SET NULL,
    assignee_team_id     bigint REFERENCES teams (id) ON DELETE SET NULL,
    metadata             jsonb NOT NULL DEFAULT '{}',
    UNIQUE (project_id, fingerprint_hash),
    UNIQUE (project_id, short_id)
);
CREATE INDEX issues_project_last_seen ON issues (project_id, last_seen);
CREATE INDEX issues_project_status ON issues (project_id, status);

CREATE TABLE issue_activity (
    id          bigserial PRIMARY KEY,
    issue_id    bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    user_id     bigint REFERENCES users (id) ON DELETE SET NULL,
    kind        text NOT NULL,
    data        jsonb NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX issue_activity_issue ON issue_activity (issue_id, id);

-- Full event documents, zstd-compressed JSON. `day` drives retention.
CREATE TABLE event_payloads (
    project_id  bigint NOT NULL,
    event_id    uuid NOT NULL,
    issue_id    bigint,
    day         date NOT NULL,
    data        bytea NOT NULL,
    PRIMARY KEY (project_id, event_id)
);
CREATE INDEX event_payloads_day ON event_payloads (day);
CREATE INDEX event_payloads_issue ON event_payloads (issue_id);

-- One row per error event, for search, counts and tag distributions.
CREATE TABLE events (
    project_id       bigint NOT NULL,
    issue_id         bigint NOT NULL,
    event_id         uuid NOT NULL,
    timestamp        timestamptz NOT NULL,
    received         timestamptz NOT NULL,
    level            text,
    platform         text,
    environment      text,
    release          text,
    dist             text,
    transaction      text,
    message          text,
    exception_type   text,
    exception_value  text,
    user_id          text,
    user_email       text,
    user_ip          text,
    user_hash        bigint,
    os               text,
    browser          text,
    device           text,
    runtime          text,
    sdk              text,
    server_name      text,
    url              text,
    tags             jsonb,
    contexts         jsonb
) USING columnar WITH (order_by = 'project_id, timestamp');
