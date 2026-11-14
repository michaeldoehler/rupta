-- Traces, logs, metrics, profiles, session replays and dashboards.

-- Web vitals (p75 per transaction) and profile links as columns of their
-- own, so aggregations over them run in the column engine.
ALTER TABLE transactions ADD COLUMN lcp double precision;
ALTER TABLE transactions ADD COLUMN fcp double precision;
ALTER TABLE transactions ADD COLUMN cls double precision;
ALTER TABLE transactions ADD COLUMN inp double precision;
ALTER TABLE transactions ADD COLUMN ttfb double precision;
ALTER TABLE transactions ADD COLUMN profile_id text;
ALTER TABLE transactions ADD COLUMN profiler_id text;

-- Error events by trace (trace view) and span.
ALTER TABLE events ADD COLUMN trace_id text;
ALTER TABLE events ADD COLUMN span_id text;

-- One row per span: the spans inside transactions (and the transaction's
-- own root span) and standalone spans. `segment_id` is the span id of
-- the transaction (segment) the span belongs to.
CREATE TABLE spans (
    project_id         bigint NOT NULL,
    trace_id           text NOT NULL,
    span_id            text NOT NULL,
    parent_span_id     text,
    segment_id         text,
    is_segment         boolean NOT NULL,
    transaction        text,
    op                 text,
    description        text,
    status             text,
    timestamp          timestamptz NOT NULL,
    duration_ms        double precision,
    environment        text,
    release            text,
    platform           text,
    sdk                text,
    received           timestamptz NOT NULL,
    data               jsonb,
    tags               jsonb,
    lcp                double precision,
    fcp                double precision,
    cls                double precision,
    inp                double precision,
    ttfb               double precision
) USING columnar WITH (order_by = 'project_id, timestamp');

-- Structured logs. Attributes are flattened to plain JSON values.
CREATE TABLE logs (
    project_id       bigint NOT NULL,
    id               uuid NOT NULL,
    timestamp        timestamptz NOT NULL,
    received         timestamptz NOT NULL,
    trace_id         text,
    span_id          text,
    level            text NOT NULL,
    severity_number  integer,
    body             text NOT NULL,
    environment      text,
    release          text,
    sdk              text,
    attributes       jsonb
) USING columnar WITH (order_by = 'project_id, timestamp');

-- Metric samples: one row per value (a statsd distribution with three
-- values is three rows). Set members are stored as hashes.
CREATE TABLE metrics (
    project_id   bigint NOT NULL,
    timestamp    timestamptz NOT NULL,
    received     timestamptz NOT NULL,
    name         text NOT NULL,
    type         text NOT NULL,
    unit         text,
    value        double precision NOT NULL,
    trace_id     text,
    span_id      text,
    environment  text,
    release      text,
    attributes   jsonb
) USING columnar WITH (order_by = 'project_id, timestamp');

-- Profiles (`kind` transaction) and continuous profiling chunks (`kind`
-- chunk, `profile_id` is the chunk id). The document is in the object
-- store under `object_key`.
CREATE TABLE profiles (
    project_id        bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    profile_id        uuid NOT NULL,
    kind              text NOT NULL,
    transaction_name  text,
    transaction_id    uuid,
    trace_id          text,
    profiler_id       text,
    platform          text,
    release           text,
    environment       text,
    started           timestamptz NOT NULL,
    duration_ms       double precision,
    samples           integer NOT NULL DEFAULT 0,
    size              bigint NOT NULL DEFAULT 0,
    object_key        text NOT NULL,
    day               date NOT NULL,
    received          timestamptz NOT NULL,
    PRIMARY KEY (project_id, profile_id)
);
CREATE INDEX profiles_project_started ON profiles (project_id, started);
CREATE INDEX profiles_transaction ON profiles (project_id, transaction_name, started);
CREATE INDEX profiles_profiler ON profiles (project_id, profiler_id, started);
CREATE INDEX profiles_day ON profiles (day);

-- Session replays: one row per replay, updated by each segment.
CREATE TABLE replays (
    project_id     bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    replay_id      uuid NOT NULL,
    started        timestamptz NOT NULL,
    finished       timestamptz NOT NULL,
    segment_count  integer NOT NULL DEFAULT 0,
    replay_type    text,
    platform       text,
    sdk            text,
    environment    text,
    release        text,
    dist           text,
    urls           jsonb NOT NULL DEFAULT '[]',
    error_ids      jsonb NOT NULL DEFAULT '[]',
    trace_ids      jsonb NOT NULL DEFAULT '[]',
    user_id        text,
    user_email     text,
    user_name      text,
    user_ip        text,
    browser        text,
    os             text,
    device         text,
    count_errors   integer NOT NULL DEFAULT 0,
    count_clicks   integer NOT NULL DEFAULT 0,
    size           bigint NOT NULL DEFAULT 0,
    day            date NOT NULL,
    PRIMARY KEY (project_id, replay_id)
);
CREATE INDEX replays_project_started ON replays (project_id, started);
CREATE INDEX replays_day ON replays (day);

-- Recording segments (zstd-compressed rrweb event arrays in the object store).
CREATE TABLE replay_segments (
    project_id  bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    replay_id   uuid NOT NULL,
    segment_id  integer NOT NULL,
    object_key  text NOT NULL,
    size        bigint NOT NULL,
    day         date NOT NULL,
    received    timestamptz NOT NULL,
    PRIMARY KEY (project_id, replay_id, segment_id)
);
CREATE INDEX replay_segments_day ON replay_segments (day);

-- Error events recorded during a replay (from the event's replay context).
CREATE TABLE issue_replays (
    project_id  bigint NOT NULL,
    event_id    uuid NOT NULL,
    issue_id    bigint NOT NULL REFERENCES issues (id) ON DELETE CASCADE,
    replay_id   uuid NOT NULL,
    timestamp   timestamptz NOT NULL,
    PRIMARY KEY (project_id, event_id)
);
CREATE INDEX issue_replays_issue ON issue_replays (issue_id, timestamp);
CREATE INDEX issue_replays_replay ON issue_replays (project_id, replay_id);

-- Dashboards of metric, transaction, log and issue widgets.
CREATE TABLE dashboards (
    id           bigserial PRIMARY KEY,
    org_id       bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    title        text NOT NULL,
    description  text,
    widgets      jsonb NOT NULL DEFAULT '[]',
    created_by   bigint REFERENCES users (id) ON DELETE SET NULL,
    created_at   timestamptz NOT NULL DEFAULT now(),
    updated_at   timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX dashboards_org ON dashboards (org_id, id);
