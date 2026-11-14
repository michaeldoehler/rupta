#!/usr/bin/env bash
# Checks the Prometheus files in deploy/prometheus with promtool: the
# configuration, the alert rules and their unit tests.
# Uses `promtool` from the PATH, or downloads Prometheus 3.16.0 into
# target/promtool (checksum verified).
set -euo pipefail
cd "$(dirname "$0")/.."
version=3.16.0
sha256=68727009472a2c02e1ff7f062fd5b3e637285bffc8fc2d340c76fb449d6e0f34
promtool=$(command -v promtool || true)
if [ -z "$promtool" ]; then
    dir=target/promtool
    promtool=$dir/promtool
    if [ ! -x "$promtool" ]; then
        if [ "$(uname -sm)" != "Linux x86_64" ]; then
            echo "install promtool (Prometheus $version) and put it on the PATH" >&2
            exit 1
        fi
        mkdir -p "$dir"
        archive=$dir/prometheus.tar.gz
        curl -fsSL -o "$archive" \
            "https://github.com/prometheus/prometheus/releases/download/v$version/prometheus-$version.linux-amd64.tar.gz"
        echo "$sha256  $archive" | sha256sum -c -
        tar -xzf "$archive" -C "$dir" --strip-components=1 "prometheus-$version.linux-amd64/promtool"
        rm "$archive"
    fi
fi
"$promtool" --version | head -1
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
cp deploy/prometheus/prometheus.yml deploy/prometheus/rupta-alerts.yml "$tmp/"
echo "example-token" > "$tmp/rupta-metrics-token"
"$promtool" check config "$tmp/prometheus.yml"
"$promtool" check rules deploy/prometheus/rupta-alerts.yml
"$promtool" test rules deploy/prometheus/rupta-alerts.test.yml
