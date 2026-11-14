#!/bin/sh
# Prepares a fresh single-node Garage for rupta through the admin API:
# assigns the node a layout, creates the bucket and imports the access key
# from the environment. Safe to run again: existing objects are kept.
set -eu
api="${GARAGE_ADMIN_URL:-http://garage:3903}"
auth="Authorization: Bearer ${GARAGE_ADMIN_TOKEN:?}"
json="Content-Type: application/json"
bucket="${RUPTA_BUCKET:-rupta}"
key_id="${RUPTA_S3_ACCESS_KEY_ID:?}"
secret="${RUPTA_S3_SECRET_ACCESS_KEY:?}"

call() { curl -fsS -H "$auth" -H "$json" "$@"; }

echo "waiting for garage"
i=0
until status=$(call "$api/v2/GetClusterStatus" 2>/dev/null); do
    i=$((i + 1)); [ "$i" -gt 60 ] && { echo "garage did not answer" >&2; exit 1; }
    sleep 1
done

if echo "$status" | grep -q '"layoutVersion": *0'; then
    node=$(echo "$status" | grep -o '"id": *"[0-9a-f]*"' | head -1 | sed 's/.*"\([0-9a-f]*\)"$/\1/')
    echo "assigning layout to node $node"
    call -X POST "$api/v2/UpdateClusterLayout" \
        -d "{\"roles\":[{\"id\":\"$node\",\"zone\":\"dc1\",\"capacity\":${GARAGE_CAPACITY_BYTES:-100000000000},\"tags\":[]}]}" > /dev/null
    call -X POST "$api/v2/ApplyClusterLayout" -d '{"version":1}' > /dev/null
fi

curl -sS -o /dev/null -H "$auth" -H "$json" -X POST "$api/v2/CreateBucket" -d "{\"globalAlias\":\"$bucket\"}" || true
curl -sS -o /dev/null -H "$auth" -H "$json" -X POST "$api/v2/ImportKey" \
    -d "{\"accessKeyId\":\"$key_id\",\"secretAccessKey\":\"$secret\",\"name\":\"rupta\"}" || true
bucket_id=$(call "$api/v2/GetBucketInfo?globalAlias=$bucket" | grep -o '"id": *"[0-9a-f]*"' | head -1 | sed 's/.*"\([0-9a-f]*\)"$/\1/')
call -X POST "$api/v2/AllowBucketKey" \
    -d "{\"bucketId\":\"$bucket_id\",\"accessKeyId\":\"$key_id\",\"permissions\":{\"read\":true,\"write\":true,\"owner\":true}}" > /dev/null
echo "garage is ready: bucket $bucket, key $key_id"
