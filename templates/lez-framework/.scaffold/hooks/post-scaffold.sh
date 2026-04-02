#!/bin/bash
# Post-scaffold hook to generate Cargo.lock for guest program.
# This prevents getrandom 0.3.x breakage on risc0 bare-metal target.

set -e

echo "Generating Cargo.lock for guest program..."

cd methods/guest
cargo generate-lockfile --quiet
cd ../..

echo "Guest Cargo.lock generated successfully."