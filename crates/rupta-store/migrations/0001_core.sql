-- Organizations, users and access.
CREATE TABLE organizations (
    id          bigserial PRIMARY KEY,
    slug        text NOT NULL UNIQUE,
    name        text NOT NULL,
    settings    jsonb NOT NULL DEFAULT '{}',
    created_at  timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE users (
    id             bigserial PRIMARY KEY,
    email          text NOT NULL UNIQUE,
    name           text NOT NULL DEFAULT '',
    password_hash  text,
    avatar_url     text,
    is_superuser   boolean NOT NULL DEFAULT false,
    disabled       boolean NOT NULL DEFAULT false,
    preferences    jsonb NOT NULL DEFAULT '{}',
    created_at     timestamptz NOT NULL DEFAULT now(),
    last_login_at  timestamptz
);

CREATE TABLE identities (
    id          bigserial PRIMARY KEY,
    user_id     bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    provider    text NOT NULL,
    subject     text NOT NULL,
    email       text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (provider, subject)
);

CREATE TABLE web_sessions (
    id          text PRIMARY KEY,           -- SHA-256 of the session token
    user_id     bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    csrf_token  text NOT NULL,
    ip          text,
    user_agent  text,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL
);
CREATE INDEX web_sessions_user ON web_sessions (user_id);

CREATE TABLE members (
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    user_id     bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (org_id, user_id)
);
CREATE INDEX members_user ON members (user_id);

CREATE TABLE invitations (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    email       text NOT NULL,
    role        text NOT NULL,
    token_hash  text NOT NULL UNIQUE,
    invited_by  bigint REFERENCES users (id) ON DELETE SET NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    expires_at  timestamptz NOT NULL,
    UNIQUE (org_id, email)
);

CREATE TABLE teams (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    slug        text NOT NULL,
    name        text NOT NULL,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);

CREATE TABLE team_members (
    team_id     bigint NOT NULL REFERENCES teams (id) ON DELETE CASCADE,
    user_id     bigint NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    PRIMARY KEY (team_id, user_id)
);

CREATE TABLE projects (
    id          bigserial PRIMARY KEY,
    org_id      bigint NOT NULL REFERENCES organizations (id) ON DELETE CASCADE,
    slug        text NOT NULL,
    name        text NOT NULL,
    platform    text,
    team_id     bigint REFERENCES teams (id) ON DELETE SET NULL,
    settings    jsonb NOT NULL DEFAULT '{}',
    next_short_id bigint NOT NULL DEFAULT 1,
    created_at  timestamptz NOT NULL DEFAULT now(),
    UNIQUE (org_id, slug)
);

CREATE TABLE project_keys (
    id                  bigserial PRIMARY KEY,
    project_id          bigint NOT NULL REFERENCES projects (id) ON DELETE CASCADE,
    public_key          text NOT NULL UNIQUE,
    secret_key          text NOT NULL,
    label               text NOT NULL DEFAULT 'Default',
    is_active           boolean NOT NULL DEFAULT true,
    rate_limit_per_min  integer,
    created_at          timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX project_keys_project ON project_keys (project_id);

CREATE TABLE api_tokens (
    id            bigserial PRIMARY KEY,
    token_hash    text NOT NULL UNIQUE,     -- SHA-256 of the token
    token_hint    text NOT NULL,            -- last characters, for display
    name          text NOT NULL,
    user_id       bigint REFERENCES users (id) ON DELETE CASCADE,
    org_id        bigint REFERENCES organizations (id) ON DELETE CASCADE,
    scopes        jsonb NOT NULL DEFAULT '[]',
    created_at    timestamptz NOT NULL DEFAULT now(),
    last_used_at  timestamptz,
    expires_at    timestamptz
);

CREATE TABLE audit_log (
    id              bigserial PRIMARY KEY,
    org_id          bigint,
    actor_user_id   bigint,
    actor_token_id  bigint,
    action          text NOT NULL,
    target_type     text,
    target_id       text,
    data            jsonb NOT NULL DEFAULT '{}',
    ip              text,
    created_at      timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX audit_log_org ON audit_log (org_id, id);

CREATE TABLE sso_configs (
    org_id              bigint PRIMARY KEY REFERENCES organizations (id) ON DELETE CASCADE,
    provider_name       text NOT NULL,
    issuer              text NOT NULL,
    client_id           text NOT NULL,
    client_secret_enc   text NOT NULL,
    scopes              text NOT NULL DEFAULT 'openid email profile',
    role_claim          text,
    role_mapping        jsonb NOT NULL DEFAULT '{}',
    default_role        text NOT NULL DEFAULT 'member',
    jit_provisioning    boolean NOT NULL DEFAULT true,
    enforce             boolean NOT NULL DEFAULT false,
    updated_at          timestamptz NOT NULL DEFAULT now()
);
