#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)"
risc0_tag="${RISC0_DOCKER_CONTAINER_TAG:-r0.1.88.0}"
cache_id="${RISC0_BUILD_CACHE_ID:-lez-programs-risc0-guests}"
network_mode="${RISC0_DOCKER_BUILD_NETWORK:-}"
staging_dir="$(mktemp -d)"

cleanup() {
  rm -rf "${staging_dir}"
}
trap cleanup EXIT

network_args=()
if [[ -n "${network_mode}" ]]; then
  network_args=(--network "${network_mode}")
fi

DOCKER_BUILDKIT=1 docker build ${network_args[@]+"${network_args[@]}"} \
  --build-arg "RISC0_DOCKER_CONTAINER_TAG=${risc0_tag}" \
  --build-arg "RISC0_BUILD_CACHE_ID=${cache_id}" \
  --output="${staging_dir}" \
  -f "${repo_root}/scripts/build-guests.Dockerfile" \
  "${repo_root}"

out_dir="${repo_root}/target/guest"
rm -rf "${out_dir}"
mkdir -p "${out_dir}"

for program in amm ata stablecoin token twap_oracle; do
  rm -rf "${repo_root}/programs/${program}/methods/guest/target"
  install -m 0644 "${staging_dir}/${program}.bin" "${out_dir}/${program}.bin"
done
