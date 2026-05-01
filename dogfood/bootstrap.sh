#!/usr/bin/env bash
set -euo pipefail

: "${DOGFOOD_ROOT:?DOGFOOD_ROOT must be set}"

CIRCUITS_VERSION="${LOGOS_BLOCKCHAIN_CIRCUITS_VERSION:-v0.4.2}"
CIRCUITS_DIR="${LOGOS_BLOCKCHAIN_CIRCUITS:-${DOGFOOD_ROOT}/cache/logos-blockchain-circuits}"
export LOGOS_BLOCKCHAIN_CIRCUITS="${CIRCUITS_DIR}"

install_circuits() {
  local parent tmp_dir
  parent="$(dirname "${CIRCUITS_DIR}")"
  mkdir -p "${parent}"
  tmp_dir="$(mktemp -d "${parent}/logos-blockchain-circuits.XXXXXX")"
  local setup_url="${LOGOS_BLOCKCHAIN_CIRCUITS_SETUP_URL:-https://raw.githubusercontent.com/logos-blockchain/logos-blockchain/main/scripts/setup-logos-blockchain-circuits.sh}"
  echo "Installing logos-blockchain-circuits ${CIRCUITS_VERSION} into ${CIRCUITS_DIR}"
  curl -fsSL "${setup_url}" | bash -s -- "${CIRCUITS_VERSION}" "${tmp_dir}"
  rm -rf "${CIRCUITS_DIR}"
  mv "${tmp_dir}" "${CIRCUITS_DIR}"
}

circuits_complete() {
  [ -f "${CIRCUITS_DIR}/VERSION" ] \
    && [ -x "${CIRCUITS_DIR}/prover" ] \
    && [ -x "${CIRCUITS_DIR}/zksign/witness_generator" ]
}

circuits_version_matches() {
  [ -f "${CIRCUITS_DIR}/VERSION" ] && [ "$(tr -d '[:space:]' < "${CIRCUITS_DIR}/VERSION")" = "${CIRCUITS_VERSION}" ]
}

if ! circuits_complete || ! circuits_version_matches; then
  install_circuits
fi

if ! circuits_complete; then
  echo "error: logos-blockchain-circuits is incomplete at ${CIRCUITS_DIR}" >&2
  exit 1
fi

actual_version="$(tr -d '[:space:]' < "${CIRCUITS_DIR}/VERSION")"
if [ "${actual_version}" != "${CIRCUITS_VERSION}" ]; then
  echo "error: circuits version is ${actual_version}; expected ${CIRCUITS_VERSION}" >&2
  exit 1
fi

set +e
"${CIRCUITS_DIR}/prover" >/tmp/logos-scaffold-prover-smoke.out 2>/tmp/logos-scaffold-prover-smoke.err
prover_status=$?
set -e

if [ "${prover_status}" -eq 132 ] || grep -qi "illegal instruction" /tmp/logos-scaffold-prover-smoke.err; then
  echo "error: circuits prover cannot execute in this Docker CPU environment" >&2
  cat /tmp/logos-scaffold-prover-smoke.err >&2
  exit 132
fi

echo "logos-blockchain-circuits ready at ${CIRCUITS_DIR} (version ${actual_version}, prover smoke status ${prover_status})"
