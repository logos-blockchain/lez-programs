# syntax=docker/dockerfile:1.7

ARG RISC0_DOCKER_CONTAINER_TAG=r0.1.88.0
FROM risczero/risc0-guest-builder:${RISC0_DOCKER_CONTAINER_TAG} AS build
ARG RISC0_BUILD_CACHE_ID=lez-programs-risc0-guests

WORKDIR /src
COPY . .

ENV CARGO_TARGET_DIR=/src/target/risc0-guests
ENV RISC0_FEATURE_bigint2=""
ENV CC_riscv32im_risc0_zkvm_elf=/root/.risc0/cpp/bin/riscv32-unknown-elf-gcc
ENV CFLAGS_riscv32im_risc0_zkvm_elf="-march=rv32im -nostdlib"

RUN --mount=type=cache,id=${RISC0_BUILD_CACHE_ID}-cargo-git,sharing=locked,target=/root/.cargo/git \
    --mount=type=cache,id=${RISC0_BUILD_CACHE_ID}-cargo-registry,sharing=locked,target=/root/.cargo/registry \
    --mount=type=cache,id=${RISC0_BUILD_CACHE_ID}-target,sharing=locked,target=/src/target/risc0-guests <<'EOF'
set -eu

target_triple="riscv32im-risc0-zkvm-elf"
programs="amm ata stablecoin token twap_oracle"
unit_separator="$(printf '\037')"
guest_rustflags="-C${unit_separator}passes=lower-atomic${unit_separator}-C${unit_separator}link-arg=-Ttext=0x00200800${unit_separator}-C${unit_separator}link-arg=--fatal-warnings${unit_separator}-C${unit_separator}panic=abort${unit_separator}--cfg${unit_separator}getrandom_backend=\"custom\""
export CARGO_ENCODED_RUSTFLAGS="${guest_rustflags}"

for program in ${programs}; do
    manifest="programs/${program}/methods/guest/Cargo.toml"
    echo "==> Building ${program}"
    cargo +risc0 build --release --locked --target "${target_triple}" --manifest-path "${manifest}"
done

mkdir -p /guest-output
unset CARGO_ENCODED_RUSTFLAGS
cargo +risc0 build --locked -p risc0-packager
packager="${CARGO_TARGET_DIR}/debug/risc0-packager"

for program in ${programs}; do
    elf="${CARGO_TARGET_DIR}/${target_triple}/release/${program}"
    "${packager}" "${elf}" "/guest-output/${program}.bin"
done
EOF

FROM scratch AS export
COPY --from=build /guest-output /
