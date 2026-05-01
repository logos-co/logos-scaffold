#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
DOGFOOD_ROOT="${DOGFOOD_ROOT:-${REPO_ROOT}/.dogfood}"
IMAGE="${LOGOS_DOGFOOD_IMAGE:-logos-scaffold-dogfood:amd64}"
SOURCE_MOUNT="/workspace/logos-scaffold-source"
DOCKER_SOCKET="${DOCKER_SOCKET:-/var/run/docker.sock}"
MODE="${1:-interactive}"
export DOCKER_HOST="${DOCKER_HOST:-unix://${DOCKER_SOCKET}}"

if [ "${MODE}" != "interactive" ] && [ "${MODE}" != "--run-scenarios" ]; then
  echo "usage: $0 [--run-scenarios]" >&2
  exit 2
fi

if ! command -v docker >/dev/null 2>&1; then
  echo "error: docker is not available on PATH" >&2
  exit 1
fi

if [ ! -S "${DOCKER_SOCKET}" ]; then
  echo "error: Docker socket is not available at ${DOCKER_SOCKET}; start Docker and retry" >&2
  exit 1
fi

docker info >/dev/null

mkdir -p \
  "${DOGFOOD_ROOT}/artifacts" \
  "${DOGFOOD_ROOT}/cache/cargo" \
  "${DOGFOOD_ROOT}/work"

docker build \
  --platform linux/amd64 \
  --tag "${IMAGE}" \
  --file "${REPO_ROOT}/dogfood/Dockerfile" \
  "${REPO_ROOT}"

if [ "${MODE}" = "--run-scenarios" ]; then
  cat <<EOF
Starting logos-scaffold dogfood lab in noninteractive scenario mode.

Source checkout: ${SOURCE_MOUNT} (read-only)
Dogfood root:    ${DOGFOOD_ROOT} (read-write, same path inside container)
EOF

  exec docker run --rm \
    --platform linux/amd64 \
    --mount "type=bind,src=${REPO_ROOT},dst=${SOURCE_MOUNT},readonly" \
    --mount "type=bind,src=${DOGFOOD_ROOT},dst=${DOGFOOD_ROOT}" \
    --mount "type=bind,src=${DOCKER_SOCKET},dst=/var/run/docker.sock" \
    --env "REPO_SOURCE=${SOURCE_MOUNT}" \
    --env "DOGFOOD_ROOT=${DOGFOOD_ROOT}" \
    --env "CARGO_HOME=${DOGFOOD_ROOT}/cache/cargo" \
    --env "DOCKER_DEFAULT_PLATFORM=linux/amd64" \
    --env "DOCKER_HOST=unix:///var/run/docker.sock" \
    --workdir "${DOGFOOD_ROOT}" \
    "${IMAGE}" \
    /bin/bash -lc '"$REPO_SOURCE/dogfood/run-scenarios.sh"'
fi

cat <<EOF
Starting logos-scaffold dogfood lab.

Source checkout: ${SOURCE_MOUNT} (read-only)
Dogfood root:    ${DOGFOOD_ROOT} (read-write, same path inside container)

Inside the container, start with:
  cd "\$DOGFOOD_ROOT"
  sed -n '1,220p' "\$REPO_SOURCE/dogfood/AGENT.md"
EOF

docker run --rm -it \
  --platform linux/amd64 \
  --mount "type=bind,src=${REPO_ROOT},dst=${SOURCE_MOUNT},readonly" \
  --mount "type=bind,src=${DOGFOOD_ROOT},dst=${DOGFOOD_ROOT}" \
  --mount "type=bind,src=${DOCKER_SOCKET},dst=/var/run/docker.sock" \
  --env "REPO_SOURCE=${SOURCE_MOUNT}" \
  --env "DOGFOOD_ROOT=${DOGFOOD_ROOT}" \
  --env "CARGO_HOME=${DOGFOOD_ROOT}/cache/cargo" \
  --env "DOCKER_DEFAULT_PLATFORM=linux/amd64" \
  --env "DOCKER_HOST=unix:///var/run/docker.sock" \
  --workdir "${DOGFOOD_ROOT}" \
  "${IMAGE}"
