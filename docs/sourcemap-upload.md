# Uploading source maps with the CLI

rupta accepts source maps from the official upload CLI, `sentry-cli`
(npm package `@sentry/cli`), so existing build pipelines keep working. How
the uploaded maps are used to symbolicate stack traces is described in
[source maps](sourcemaps.md).

## Setup

1. Create an **organization token** in *Organization settings → API
   tokens* with the scope `project:releases` (add `org:read` if you want
   `sentry-cli info` to show the scopes). Personal tokens with the same
   scope work too.
2. Point the CLI at rupta, either with flags (`--url`, `--auth-token`) or
   environment variables:

   ```sh
   export SENTRY_URL=https://errors.example.com
   export SENTRY_AUTH_TOKEN=rpt_...
   export SENTRY_ORG=acme
   export SENTRY_PROJECT=web
   ```

   The CLI prints the warning *Unrecognized auth token format* because rupta
   tokens start with `rpt_`; it is harmless.

## Commands that work

Verified with `sentry-cli` 3.8.2 against a running server; the recorded
requests are replayed by `crates/rupta-api/tests/cli_replay.rs`.

| Command | Notes |
| --- | --- |
| `sentry-cli sourcemaps inject ./dist` | Local only: adds debug ids to files and maps. Recommended. |
| `sentry-cli sourcemaps upload --release <version> [--dist <dist>] ./dist` | Uploads an artifact bundle. Creates the release if it does not exist. |
| `sentry-cli releases new <version>` | Needs `--project` (or `SENTRY_PROJECT`). |
| `sentry-cli releases finalize <version>` | Sets the release date. |
| `sentry-cli releases info <version>` | |
| `sentry-cli releases list` | The newest 100 releases. |
| `sentry-cli deploys new --release <version> -e <environment> [-n <name>] [-u <url>]` | |
| `sentry-cli deploys list --release <version>` | |
| `sentry-cli info` | Shows the token's scopes. |

Bundlers that call the CLI or the same HTTP API (the official Vite, webpack,
Rollup and esbuild plugins) use the same requests and should work as well;
they were not tested.

Not supported (the server answers `404` with `not supported by rupta`, or `405` for an unsupported method on a supported path):
`releases set-commits` and anything about commits and repositories,
`releases delete`, `releases files ...` (the legacy release-file upload),
`debug-files` (native symbols), `send-event`, `monitors run` (use an SDK for
check-ins), `react-native` and other platform commands.

## Example: Vite build in CI

```sh
npm run build                                   # writes dist/ with *.js.map
npx @sentry/cli sourcemaps inject ./dist
npx @sentry/cli sourcemaps upload --release "web@$GIT_SHA" ./dist
npx @sentry/cli releases finalize "web@$GIT_SHA"
npx @sentry/cli deploys new --release "web@$GIT_SHA" -e production
rm dist/**/*.map                                # optional: do not ship maps
```

Set the same release in the SDK (`release: "web@" + GIT_SHA`) so events and
maps match. With debug ids injected, matching works without a release too.

## What the server does

- `GET /api/0/organizations/{org}/chunk-upload/` tells the CLI to upload in
  chunks of 8 MiB, up to 64 chunks and 32 MiB per request, SHA-1 checksums,
  gzip allowed; the largest bundle is 512 MiB.
- `POST /api/0/organizations/{org}/artifactbundle/assemble/` first reports
  which chunks are missing; the CLI uploads them
  (`POST .../chunk-upload/`, multipart fields `file` or `file_gzip`, each
  named by the SHA-1 of its uncompressed content, which is checked); chunks
  are stored in the object store under `chunks/{sha1}` and recorded in
  `file_chunks`.
- The second assemble call joins the chunks in order, checks the SHA-1 of
  the result, reads `manifest.json` from the zip, stores the bundle at
  `artifact-bundles/{org id}/{sha1}.zip` and fills `artifact_bundles`,
  `artifact_bundle_projects` and `artifact_index` (one row per file with its
  URL such as `~/assets/app.js`, its type, debug id, release and dist).
  Assembly is synchronous: the answer is `ok`, `not_found` (with
  `missingChunks`) or `error` (with `detail`). Uploading the same bundle
  again only links the projects.
- Releases (`POST /api/0/organizations/{org}/releases/`,
  `PUT .../releases/{version}/`) and deploys
  (`POST .../releases/{version}/deploys/`) write the `releases`,
  `release_projects` and `deploys` tables that the web app shows.

Errors of this API have the form `{"detail": "..."}`.

## Limits

- Uploaded chunks are kept until they are cleaned up by maintenance; there
  is no automatic removal of chunks or old bundles yet.
- Commits, repositories and suspect commits are not supported.
- Release versions may not contain `/` (the CLI's path format would break),
  and may not be `latest`, `.` or `..`.
