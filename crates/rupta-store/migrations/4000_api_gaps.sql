-- Resolution in a release, ignore conditions on users, escalation
-- detection, distinct users for release health, artifact clean-up.

-- How `resolved_in_release` is meant: `release` (resolved in that
-- version) or `next_release` (resolved in the release after the one
-- recorded in `resolved_in_release`, the latest at resolution time).
ALTER TABLE issues ADD COLUMN resolved_in_mode text;

-- Ignore until more users are affected. Without a window: the
-- `users_seen` value at which the issue reappears (absolute threshold).
-- With a window (minutes): the number of distinct users within it.
ALTER TABLE issues ADD COLUMN ignore_user_count bigint;
ALTER TABLE issues ADD COLUMN ignore_user_window integer;

-- Escalation: when the issue became `escalating` and the hourly event
-- average of the 7 days before, against which it is measured.
ALTER TABLE issues ADD COLUMN escalated_at timestamptz;
ALTER TABLE issues ADD COLUMN escalation_baseline double precision;
CREATE INDEX issues_status_substatus ON issues (status, substatus);

-- Distinct users of sessions per project, release, environment and hour:
-- one row per user and hour (and a second one once the user's session
-- crashed). Counted with count(DISTINCT user_hash).
CREATE TABLE session_users (
    project_id   bigint NOT NULL,
    release      text NOT NULL,
    environment  text NOT NULL,
    hour         timestamptz NOT NULL,
    user_hash    bigint NOT NULL,
    crashed      boolean NOT NULL
) USING columnar WITH (order_by = 'project_id, hour');

CREATE INDEX file_chunks_created ON file_chunks (created_at);
CREATE INDEX artifact_index_bundle ON artifact_index (bundle_id);
