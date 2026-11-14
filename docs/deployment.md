# Deployment

rupta is one process with one container image. It needs a volume for its
working copy and, for durability, an S3-compatible bucket. This chapter
covers Docker Compose, Docker Swarm, the bucket, TLS and sizing.

## Docker Compose (single host)

The repository's `compose.yaml` starts rupta with a single-node
[Garage](https://garagehq.deuxfleurs.fr) as durable storage:

```sh
git clone https://github.com/michaeldoehler/rupta && cd rupta
scripts/generate-env.sh https://errors.example.com   # writes .env with random secrets
docker compose up -d
docker compose logs -f rupta
```

`generate-env.sh` prints the administrator's password. Open the public URL,
log in, create a project and copy its DSN into your application's SDK
configuration (see [SDK compatibility](sdk-compatibility.md)).

What runs:

| Service | Purpose |
| --- | --- |
| `rupta` | the server on port 8080 (`RUPTA_PORT` on the host) |
| `garage` | single-node Garage 2.5 holding the durable copy |
| `garage-init` | one-shot: assigns Garage's layout, creates the bucket and imports the access key from `.env`; safe to rerun |

Data lives in three named volumes: `rupta-data` (working copy and cache),
`garage-meta` and `garage-data` (the bucket). Losing `rupta-data` loses
nothing: rupta restores itself from the bucket at the next start.

**Local storage only.** For a quick evaluation without Garage:

```sh
docker run -d --name rupta -p 8080:8080 -v rupta-data:/data \
  -e RUPTA_SECRET_KEY=$(openssl rand -hex 32) \
  ghcr.io/michaeldoehler/rupta:0.1.0
```

The first visitor completes the setup screen. Without a bucket, the volume
is the only copy of the data.

## Docker Swarm

`deploy/stack.yml` is a production template: rupta with Docker secrets,
connected to an existing Garage cluster on an external overlay network.

```sh
printf '%s' "$(openssl rand -hex 32)" | docker secret create rupta_secret_key -
printf '%s' 'GK…'                     | docker secret create rupta_s3_access_key_id -
printf '%s' '…'                       | docker secret create rupta_s3_secret_access_key -
printf '%s' '…'                       | docker secret create rupta_oidc_client_secret -
docker stack deploy -c deploy/stack.yml rupta
```

The settings that matter:

- **Exactly one replica.** A database has one writer. `replicas: 1` with
  `update_config.order: stop-first` and `rollback_config.order: stop-first`
  ensures Swarm never runs two tasks at once. If it ever does (a network
  partition, a manual mistake), the storage layer fences the older writer:
  its writes fail and nothing it acknowledged is lost, but requests to it
  fail until it is stopped.
- **Local volume as cache.** `rupta-data` is a local volume on whichever
  node runs the task. When the task moves to another node, it downloads
  the newest snapshot from the bucket and replays the log written since;
  start-up then takes longer (`start_period: 120s` in the health check).
  Don't use a network file system for the volume.
- **Secrets as files.** Every secret variable has a `_FILE` form; the stack
  mounts Docker secrets under `/run/secrets/`.
- **`stop_grace_period: 60s`** gives rupta time to write queued events and
  upload the last log records on shutdown.
- **Health check** `rupta healthcheck` calls `/readyz`, which fails when the
  database is not writable or another writer has taken over.

Put a reverse proxy (Traefik, Caddy, nginx) in front for TLS and set
`RUPTA_PUBLIC_URL` to the external `https://` URL and
`RUPTA_TRUSTED_PROXIES` to the proxy's network so client IPs are correct.

## The bucket: Garage or any S3-compatible store

rupta keeps its durable copy in a bucket through the embedded database's
remote storage. Requirements:

- `PUT`, `GET`, `LIST` with read-after-write consistency. Garage in its
  default `consistency_mode = "consistent"`, AWS S3 and MinIO qualify. Do
  not run a Garage cluster with `consistency_mode` set to `degraded` or
  `dangerous` for rupta: the writer fencing relies on consistent listings.
  Conditional writes are not needed.
- One prefix per rupta instance (`s3://rupta/production`, `s3://rupta/staging`).
- For production, a Garage cluster with `replication_factor = 3` across
  failure domains. The single-node Garage of the Compose stack protects
  against losing the rupta volume, not against losing the host.

Credentials: an access key with read and write permission on the bucket,
given as `RUPTA_S3_ACCESS_KEY_ID` and `RUPTA_S3_SECRET_ACCESS_KEY`.

`RUPTA_REMOTE_SYNC=sync` (default) acknowledges a database write only after
it is in the bucket. Ingestion batches events (up to 1,000 per write, at
least every 50 ms), so the round trip is paid per batch, not per event.

## Single sign-on with Zitadel

See [single sign-on](sso.md) for the step-by-step Zitadel setup. In short:
create a Web application in a Zitadel project with redirect URI
`https://errors.example.com/auth/sso/oidc/callback`, define project roles
(`rupta-owner`, `rupta-admin`, `rupta-member`, `rupta-viewer`), enable
"assert roles on authentication", and set:

```sh
RUPTA_OIDC_ISSUER=https://auth.example.com
RUPTA_OIDC_CLIENT_ID=…
RUPTA_OIDC_CLIENT_SECRET=…        # or _FILE
RUPTA_OIDC_NAME=Zitadel
RUPTA_OIDC_ROLE_MAPPING=rupta-owner:owner,rupta-admin:admin,rupta-member:member,rupta-viewer:viewer
RUPTA_LOCAL_LOGIN=false           # optional: SSO only
```

## Sizing

Rough guidance for a start; measure with your own traffic.

| Load | CPU | Memory | Notes |
| --- | --- | --- | --- |
| up to ~100 events/s | 1 core | 1 GiB | |
| ~1,000 events/s | 2–4 cores | 2–4 GiB | `RUPTA_CACHE_MB=512` |
| beyond | 4+ cores | 4+ GiB | see [ingestion](ingestion.md) for measured throughput |

Disk on the rupta volume grows with retained events. Events are stored
compressed; plan for roughly 1–3 KB per event plus attachments.

## Limits

- One writer per instance: rupta does not scale out horizontally. Run
  separate instances for separate tenants or regions.
- No automatic failover beyond Swarm rescheduling the task; a move to a
  new node costs a restore from the bucket.
- The Compose stack's Garage is a single node.
