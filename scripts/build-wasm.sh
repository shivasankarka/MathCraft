#!/usr/bin/env bash
# Build the browser module and put it next to the page: crates/wasm/web/{index.html,mathcraft_wasm.wasm,...}.
# Needs the target once: rustup target add wasm32-unknown-unknown
# Serve it with any static server, for example: python3 -m http.server -d crates/wasm/web
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown -p mathcraft-wasm
cp target/wasm32-unknown-unknown/release/mathcraft_wasm.wasm crates/wasm/web/
# smaller, when binaryen is installed
if command -v wasm-opt >/dev/null; then
  wasm-opt -Oz crates/wasm/web/mathcraft_wasm.wasm -o crates/wasm/web/mathcraft_wasm.wasm
fi
echo "crates/wasm/web/mathcraft_wasm.wasm ($(wc -c < crates/wasm/web/mathcraft_wasm.wasm) bytes)"
