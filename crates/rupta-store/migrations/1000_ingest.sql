-- Transactions (performance events) as sent by the SDKs. Kept for later
-- releases; version 0.1 stores them but has no UI for them.
CREATE TABLE transactions (
    project_id       bigint NOT NULL,
    event_id         uuid NOT NULL,
    trace_id         text,
    span_id          text,
    parent_span_id   text,
    name             text,
    op               text,
    status           text,
    timestamp        timestamptz NOT NULL,
    start_timestamp  timestamptz,
    duration_ms      double precision,
    received         timestamptz NOT NULL,
    environment      text,
    release          text,
    platform         text,
    sdk              text,
    user_hash        bigint,
    span_count       integer,
    tags             jsonb,
    measurements     jsonb
) USING columnar WITH (order_by = 'project_id, timestamp');

-- Lookup of user feedback by the event it refers to (linking to issues).
CREATE INDEX user_feedback_event ON user_feedback (project_id, event_id);
