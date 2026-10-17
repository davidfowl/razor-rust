#!/usr/bin/env bash
set -euo pipefail

# Run from the workspace root after building the server example.
server="${1:-target/debug/examples/server}"
tmp=$(mktemp -d)
pid=""
cleanup() {
    if [[ -n "$pid" ]]; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    rm -f "$tmp/server.log" "$tmp/headers" "$tmp/body"
    rmdir "$tmp"
}
trap cleanup EXIT

BIND_ADDRESS=127.0.0.1:0 "$server" > "$tmp/server.log" 2>&1 &
pid=$!
address=""
for ((attempt = 0; attempt < 100; attempt++)); do
    address=$(sed -n 's/^LISTENING //p' "$tmp/server.log")
    if [[ -n "$address" ]]; then
        break
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
        cat "$tmp/server.log" >&2
        exit 1
    fi
    sleep 0.1
done
if [[ -z "$address" ]]; then
    echo "Server did not become ready" >&2
    cat "$tmp/server.log" >&2
    exit 1
fi

request() {
    local route="$1" expected_status="$2" expected_type="$3"
    local status content_type
    status=$(curl --silent --show-error --max-time 10 \
        --dump-header "$tmp/headers" --output "$tmp/body" \
        --write-out '%{http_code}' "$address$route")
    content_type=$(tr -d '\r' < "$tmp/headers" |
        sed -n 's/^[Cc]ontent-[Tt]ype: //p')
    [[ "$status" == "$expected_status" ]]
    [[ "$content_type" == "$expected_type" ]]
    printf 'GET %s: %s %s\n' "$route" "$status" "$content_type"
    cat "$tmp/body"
}

request / 200 'text/html; charset=utf-8'
diff -u crates/razor-slices-axum/tests/fixtures/greeting.html "$tmp/body"
request /error 500 'text/plain; charset=utf-8'
cmp "$tmp/body" <(printf '%s' 'Internal Server Error')
printf '\n'
grep -F 'ERROR razor_slices_axum: slice response rendering failed error=deliberate example rendering failure error_kind=Other' "$tmp/server.log"
