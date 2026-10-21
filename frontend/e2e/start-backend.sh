#!/bin/sh
set -eu
cd "$(dirname "$0")/../../backend"
cargo build --quiet
home=$(mktemp -d)
printf 'e2e-password\n' | ARCMAN_HOME="$home" ./target/debug/arcman set-password
ARCMAN_HOME="$home" ARCMAN_FXTWITTER_BASE_URL=http://127.0.0.1:3191 exec ./target/debug/arcman serve --addr 127.0.0.1:3190
