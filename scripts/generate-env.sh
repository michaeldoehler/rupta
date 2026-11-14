#!/usr/bin/env bash
# Writes a .env for `docker compose up` with fresh random secrets.
# Usage: scripts/generate-env.sh [public-url]   (default http://localhost:8080)
set -euo pipefail
cd "$(dirname "$0")/.."
if [ -e .env ]; then
    echo ".env exists; remove it first if you want new secrets" >&2
    exit 1
fi
url="${1:-http://localhost:8080}"
password="$(openssl rand -base64 18)"
sed -e "s|^RUPTA_PUBLIC_URL=.*|RUPTA_PUBLIC_URL=$url|" \
    -e "s|^RUPTA_SECRET_KEY=.*|RUPTA_SECRET_KEY=$(openssl rand -hex 32)|" \
    -e "s|^RUPTA_ADMIN_EMAIL=.*|RUPTA_ADMIN_EMAIL=admin@example.com|" \
    -e "s|^RUPTA_ADMIN_PASSWORD=.*|RUPTA_ADMIN_PASSWORD=$password|" \
    -e "s|^RUPTA_S3_ACCESS_KEY_ID=.*|RUPTA_S3_ACCESS_KEY_ID=GK$(openssl rand -hex 12)|" \
    -e "s|^RUPTA_S3_SECRET_ACCESS_KEY=.*|RUPTA_S3_SECRET_ACCESS_KEY=$(openssl rand -hex 32)|" \
    -e "s|^GARAGE_RPC_SECRET=.*|GARAGE_RPC_SECRET=$(openssl rand -hex 32)|" \
    -e "s|^GARAGE_ADMIN_TOKEN=.*|GARAGE_ADMIN_TOKEN=$(openssl rand -hex 32)|" \
    .env.example > .env
chmod 600 .env
echo "Wrote .env. Log in at $url as admin@example.com with password: $password"
