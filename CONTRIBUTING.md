# Contributing to rupta

Thank you for helping. This page explains how the repository is organized
and what a change needs before it is merged.

## Layout

| Path | Content |
| --- | --- |
| `crates/rupta-core` | domain types, event normalization, grouping, data scrubbing, search language |
| `crates/rupta-store` | storage on the embedded squad database, schema migrations |
| `crates/rupta-ingest` | SDK protocol endpoints, processing pipeline, source map symbolication |
| `crates/rupta-auth` | accounts, sessions, single sign-on, API tokens, roles |
| `crates/rupta-api` | REST API, upload CLI endpoints, alerts and notifications |
| `crates/rupta-server` | the `rupta` binary: configuration, wiring, web app embedding |
| `web/` | the web app (React, TypeScript, Tailwind CSS, shadcn/ui) |
| `deploy/` | Docker Compose and Swarm files |
| `docs/` | documentation |

## Development setup

Requirements: Rust (stable, minimum 1.89), a C compiler and libclang (the
embedded database compiles its SQL parser from C), Node.js 22 and pnpm 10.

```sh
# backend on :8080 with a local database
RUPTA_SECRET_KEY=$(openssl rand -hex 32) RUPTA_DATA_DIR=./data cargo run -p rupta-server

# web app with hot reload on :5173, proxying /api to :8080
pnpm --dir web install
pnpm --dir web dev

# web app on mock data, without a backend
VITE_MOCK=1 pnpm --dir web dev
```

## Checks

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm --dir web lint && pnpm --dir web typecheck && pnpm --dir web test
scripts/sdk-smoke.sh        # real SDKs against a local server
```

## What a change needs

- **A test that fails without it.** Protocol changes need a fixture captured
  from a real SDK.
- **Compatibility.** Events sent by the official open-source SDKs must keep
  working. Changes to stored data come with a new numbered migration in
  `crates/rupta-store/migrations`; released migrations are never edited.
- **Documentation** in `docs/` for every user-visible change, and an entry
  in `CHANGELOG.md` under "Unreleased".
- **Plain commit messages**: an imperative subject line ("Add …", "Fix …")
  and a body that explains why.

## License

By contributing you agree that your contribution is licensed under the MIT
license, like the rest of the project.
