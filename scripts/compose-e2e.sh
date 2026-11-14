#!/usr/bin/env bash
# End-to-end test of the Docker Compose stack: builds the image, starts
# rupta with Garage, sends an event, deletes rupta's volume and checks that
# the restarted container restores the data from the bucket.
#
# Usage: scripts/compose-e2e.sh        (needs docker with compose, curl, python3)
set -euo pipefail
cd "$(dirname "$0")/.."
export COMPOSE_PROJECT_NAME=rupta-e2e
IMAGE_VERSION=${RUPTA_VERSION:-}
PORT=${RUPTA_E2E_PORT:-18080}
B=http://127.0.0.1:$PORT
WORK=$(mktemp -d)
trap 'docker compose logs rupta > "$WORK/rupta.log" 2>&1 || true; docker compose down -v > /dev/null 2>&1 || true; rm -f .env.e2e' EXIT

if [ -e .env ]; then echo ".env exists; this test writes its own and refuses to overwrite yours" >&2; exit 1; fi
scripts/generate-env.sh "$B" > /dev/null
mv .env .env.e2e
sed -i "s|^RUPTA_PORT=.*|RUPTA_PORT=$PORT|" .env.e2e
compose() { docker compose --env-file .env.e2e "$@"; }
set -a; . ./.env.e2e; set +a

json() { python3 -c "import json,sys; d=json.load(sys.stdin); print(eval(sys.argv[1]))" "$1"; }
wait_healthy() {
  for _ in $(seq 120); do
    [ "$(docker inspect -f '{{.State.Health.Status}}' "$(compose ps -q rupta)" 2>/dev/null)" = healthy ] && return 0
    sleep 2
  done
  compose logs rupta | tail -50; return 1
}
login() {
  curl -sf -c "$WORK/cookies" -H 'content-type: application/json' \
    -d "{\"email\":\"$RUPTA_ADMIN_EMAIL\",\"password\":\"$RUPTA_ADMIN_PASSWORD\"}" "$B/api/v1/auth/login"
}

# RUPTA_E2E_NO_BUILD=1 uses an existing image (tag it as
# ghcr.io/michaeldoehler/rupta:$RUPTA_VERSION) instead of building one.
if [ "${RUPTA_E2E_NO_BUILD:-}" = 1 ]; then
  sed -i "s|^RUPTA_VERSION=.*|RUPTA_VERSION=${IMAGE_VERSION:?set RUPTA_VERSION to the tag of the existing image}|" .env.e2e
  export RUPTA_VERSION=$IMAGE_VERSION
  compose up -d --no-build
else
  compose up -d --build
fi
wait_healthy
echo "rupta is healthy: $(curl -s "$B/readyz")"

session=$(login)
csrf=$(echo "$session" | json "d['csrf_token']"); org=$(echo "$session" | json "d['orgs'][0]['slug']")
curl -sf -b "$WORK/cookies" -H "x-csrf-token: $csrf" -H 'content-type: application/json' \
  -d '{"name":"Web","slug":"web","platform":"javascript"}' "$B/api/v1/orgs/$org/projects" > /dev/null
dsn=$(curl -sf -b "$WORK/cookies" "$B/api/v1/projects/$org/web/keys" | json "(d['data'] if isinstance(d,dict) else d)[0]['dsn']")
key=$(echo "$dsn" | sed -E 's#^https?://([^@]+)@.*#\1#'); project=$(echo "$dsn" | sed -E 's#.*/([0-9]+)$#\1#')
curl -sf -H "x-sentry-auth: Sentry sentry_version=7, sentry_key=$key" -H 'content-type: application/json' \
  -d '{"message":"compose e2e","level":"error","platform":"javascript"}' "$B/api/$project/store/" > /dev/null
count=0
for _ in $(seq 30); do
  count=$(curl -sf -b "$WORK/cookies" "$B/api/v1/orgs/$org/issues?query=" | json "len(d['data'])")
  [ "$count" -ge 1 ] && break; sleep 1
done
test "$count" -ge 1 || { echo "the event did not arrive" >&2; exit 1; }
echo "event stored; removing rupta's volume"

compose stop rupta
compose rm -f rupta
docker volume rm "${COMPOSE_PROJECT_NAME}_rupta-data" > /dev/null
compose up -d rupta
wait_healthy
login > /dev/null
restored=$(curl -sf -b "$WORK/cookies" "$B/api/v1/orgs/$org/issues?query=" | json "d['data'][0]['title']")
test "$restored" = "compose e2e" || { echo "restore failed: $restored" >&2; exit 1; }
echo "compose e2e passed: the issue was restored from Garage"
