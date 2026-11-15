# Getting started

From nothing to the first error in rupta in about five minutes: start
rupta with Docker Compose, log in, create a project, send a test error
with `curl` and with an SDK, and find it in the web app.

rupta receives data from the **Sentry SDKs**, the open-source client
libraries that Sentry publishes for each platform. There is no rupta SDK:
you install the Sentry SDK for your platform and give it the DSN of a
rupta project. Which SDKs and data types are tested is listed in the
[compatibility matrix](sdk-compatibility.md#compatibility-matrix).

> Sentry is a trademark of Functional Software, Inc. rupta is an
> independent project and is not affiliated with or endorsed by Sentry.

## What you need

- Docker with the Compose plugin, `git` and `openssl`.
- Port 8080 free on the host (change it with `RUPTA_PORT` in `.env`).
- For the SDK examples: Node.js, PHP with Composer or Python, whichever
  your application uses.

## Start rupta

```sh
git clone https://github.com/michaeldoehler/rupta && cd rupta
scripts/generate-env.sh http://localhost:8080
docker compose up -d
```

`generate-env.sh` writes `.env` with random secrets and prints the
password of the administrator `admin@example.com`. Keep it. Compose starts
rupta and a single-node Garage bucket that holds the durable copy of the
data ([deployment](deployment.md)). `docker compose ps` shows `rupta` as
`healthy` after a few seconds.

The URL you pass to `generate-env.sh` becomes `RUPTA_PUBLIC_URL`: DSNs,
links and single sign-on redirects are built from it. On a server, pass the
URL under which your applications and users reach rupta, for example
`https://errors.example.com`.

Only want a quick look, without the bucket? One container is enough:

```sh
docker run -d -p 8080:8080 -v rupta-data:/data \
  -e RUPTA_SECRET_KEY=$(openssl rand -hex 32) \
  ghcr.io/michaeldoehler/rupta:0.3.0
```

Then the first visitor of <http://localhost:8080> creates the
administrator account and the organization in a setup screen.

## Log in and create a project

1. Open <http://localhost:8080> and log in as `admin@example.com` with the
   printed password.
2. Go to **Settings → All projects → New project**. Give it a name and
   choose the platform of your application (it selects the setup
   instructions shown and the project's icon).
3. rupta opens the project's **Client keys & SDK** page. Copy the **DSN**.
   It looks like this:

   ```text
   http://3f1c2a9e8b7d4c6e9a0b1c2d3e4f5a6b@localhost:8080/1
          └──────── public key ───────┘ └─ host ─┘ └ project id
   ```

The public key only allows sending data to this project; it is safe to
ship in web pages and apps. Below the DSN, the page shows copy-paste setup
for 14 platforms.

## Send a test error with curl

Replace the key and the project id with the parts of your DSN:

```sh
KEY=3f1c2a9e8b7d4c6e9a0b1c2d3e4f5a6b
PROJECT=1
curl -X POST "http://localhost:8080/api/$PROJECT/envelope/" \
  -H "X-Sentry-Auth: Sentry sentry_version=7, sentry_key=$KEY, sentry_client=curl/1.0" \
  --data-binary @- <<'EOF'
{}
{"type":"event"}
{"level":"error","message":"Hello from curl","exception":{"values":[{"type":"TestError","value":"Hello from curl"}]}}
EOF
```

The answer is the event id, `{"id":"..."}`. This is the envelope format
the SDKs use: a header line, an item header and the event as JSON. A `401`
means the key is wrong; a `403` that it belongs to another project.

## Send errors from your application

Install the Sentry SDK for your platform and initialize it with the DSN.
Set `release` to your application's version so issues, regressions and
source maps can be tied to it. The JavaScript, Node.js, PHP and Python
examples below were run against rupta; the Laravel and Django examples
follow the SDKs' own setup and were not run.

### JavaScript in the browser

```sh
npm install @sentry/browser
```

```js
import * as Sentry from "@sentry/browser";

Sentry.init({
  dsn: "http://<public key>@localhost:8080/<project id>",
  release: "my-app@1.0.0",
  environment: "production",
  tracesSampleRate: 1.0, // optional: performance tracing
});
```

Uncaught errors and unhandled promise rejections are reported
automatically; `Sentry.captureException(error)` reports caught ones.
React, Vue, Angular, Svelte and Next.js have their own packages
(`@sentry/react`, `@sentry/nextjs`, …) with the same `init` options; the
project settings show their setup. Browsers send from the page's origin:
by default a project accepts every origin; restrict it under **Allowed
origins** on the project's **Security** settings page.

### Node.js

```sh
npm install @sentry/node
```

`instrument.mjs`:

```js
import * as Sentry from "@sentry/node";

Sentry.init({
  dsn: "http://<public key>@localhost:8080/<project id>",
  release: "my-app@1.0.0",
  environment: "production",
});
```

Load it before your application, so the SDK can instrument it:

```sh
node --import ./instrument.mjs app.mjs
# CommonJS: write instrument.js with require("@sentry/node") and run
# node --require ./instrument.js app.js
```

Uncaught exceptions are reported before the process exits. For caught
errors, call `Sentry.captureException(error)`; in short scripts, wait for
the send with `await Sentry.flush(2000)` before exiting.

### PHP

```sh
composer require sentry/sentry
```

```php
\Sentry\init([
    'dsn' => 'http://<public key>@localhost:8080/<project id>',
    'release' => 'my-app@1.0.0',
    'environment' => 'production',
]);

try {
    throw new \RuntimeException('My first rupta error');
} catch (\Throwable $e) {
    \Sentry\captureException($e);
}
```

### Laravel

```sh
composer require sentry/sentry-laravel
php artisan sentry:publish --dsn=http://<public key>@localhost:8080/<project id>
```

Or set `SENTRY_LARAVEL_DSN` in `.env`. In Laravel 11 and later, report
exceptions in `bootstrap/app.php`:

```php
use Sentry\Laravel\Integration;

->withExceptions(function (Exceptions $exceptions) {
    Integration::handles($exceptions);
})
```

`php artisan sentry:test` sends a test event.

### Python

```sh
pip install --upgrade sentry-sdk
```

```python
import sentry_sdk

sentry_sdk.init(
    dsn="http://<public key>@localhost:8080/<project id>",
    release="my-app@1.0.0",
    environment="production",
)

division_by_zero = 1 / 0
```

Uncaught exceptions are reported; `sentry_sdk.capture_exception()`
reports caught ones. The integrations for Flask, FastAPI, Celery and
others switch on by themselves when the framework is installed.

### Django

```sh
pip install --upgrade "sentry-sdk[django]"
```

In `settings.py`:

```python
import sentry_sdk

sentry_sdk.init(
    dsn="http://<public key>@localhost:8080/<project id>",
    release="my-app@1.0.0",
    environment="production",
)
```

Other platforms (Go, Java, .NET, Ruby, Rust, mobile) work the same way:
the setup is on the project's **Client keys & SDK** page.

**The DSN host must be reachable from the application.** `localhost`
works for an application on the same machine. An application in another
container or on another host needs the host name or address under which
it reaches rupta, which is why `RUPTA_PUBLIC_URL` should be that address.

## Find the error in the web app

Open **Issues**. Every error appears there within a second or two,
grouped into an **issue**: events with the same exception type and the
same in-app stack frames share one issue ([grouping](grouping.md)). Open
the issue to see the stack trace, breadcrumbs, request, tags and the
context panel; resolve, ignore or assign it from there. The
[user guide](user-guide.md) is a tour of the whole app.

Terms used throughout the documentation:

| Term | Meaning |
| --- | --- |
| organization | the top level: members, teams, projects, settings |
| project | one application or service; has its own DSN, settings and issues |
| DSN | the address an SDK sends to, with the project's public key |
| event | one error or message the SDK sent |
| issue | a group of similar events, with a status (unresolved, resolved, ignored) |

Nothing arrives? See [troubleshooting](operations.md#troubleshooting):
most often the DSN's host is not reachable from the application, or the
SDK exited before sending.

## Moving an existing application from Sentry

An application that already uses a Sentry SDK needs no code change:

1. Create a project in rupta and copy its DSN.
2. Replace the DSN in the configuration (often the `SENTRY_DSN`
   environment variable, `SENTRY_LARAVEL_DSN` in Laravel).
3. Redeploy.

Releases, environments, sample rates, `beforeSend` hooks, tracing, logs,
metrics, profiling, session replay and cron monitors keep working as
listed in the [compatibility matrix](sdk-compatibility.md#compatibility-matrix).
Existing issues and events stay in Sentry; rupta starts empty.

## Upload source maps

For readable stack traces from minified JavaScript, upload the source
maps of each build with `sentry-cli`. Create an organization token with the
scope `project:releases` under **Settings → API tokens** (in the
organization section), then in your
build:

```sh
export SENTRY_URL=http://localhost:8080   # rupta's public URL
export SENTRY_AUTH_TOKEN=rpt_...
export SENTRY_ORG=default                 # your organization's slug
export SENTRY_PROJECT=web                 # your project's slug

npm run build
npx @sentry/cli sourcemaps inject ./dist
npx @sentry/cli sourcemaps upload --release my-app@1.0.0 ./dist
```

Use the same `release` in `Sentry.init`. The organization created by
`generate-env.sh` has the slug `default`; slugs appear in the URLs of the
web app. Details, the supported commands and CI examples:
[source map upload](sourcemap-upload.md).

## Next steps

- [Deployment](deployment.md): TLS, Docker Swarm, a production bucket.
- [Configuration](configuration.md): every environment variable.
- [Alerts](alerts.md): email, Slack, Teams, webhooks, PagerDuty, Opsgenie.
- [Single sign-on](sso.md) and [authentication](authentication.md):
  invite your team.
- The [documentation index](README.md) lists all chapters.
