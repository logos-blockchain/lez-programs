# AMM UI on macOS

The AMM flake exposes native packages for Apple Silicon and Intel macOS. Nix
provides the Rust, C++, Qt, and Apple SDK build dependencies. The only required
host toolchain is Apple's Metal compiler, which Apple distributes through
Xcode rather than through a redistributable package.

The standalone build needs no input overrides, temporary source pins, or
`DYLD_LIBRARY_PATH`.

## One-time prerequisites

### Nix

Install Nix in multi-user mode, make sure its daemon is available, and enable
flakes:

```sh
mkdir -p ~/.config/nix
printf '%s\n' 'experimental-features = nix-command flakes' >> ~/.config/nix/nix.conf
nix --version
nix store info
```

Nix normally has `sandbox = false` on macOS. This build must retain that
setting because RISC Zero invokes Apple's host-installed Metal toolchain. The
preflight script below reports an actionable error if the daemon is configured
otherwise.

### Xcode and the Metal Toolchain

Apple Command Line Tools alone do not include `metal` and `metallib`. Install
the newest full Xcode release supported by the installed macOS from the App
Store or [Apple Developer Downloads](https://developer.apple.com/download/all/).
This requires an Apple account and cannot be automated by the repository.

After installation:

```sh
sudo xcode-select --switch /Applications/Xcode.app/Contents/Developer
sudo xcodebuild -license accept
sudo xcodebuild -runFirstLaunch
xcodebuild -downloadComponent MetalToolchain
xcrun --sdk macosx --find metal
xcrun --sdk macosx --find metallib
```

The last two commands must print executable paths. Do not use
`RISC0_SKIP_BUILD_KERNELS=1`: the pinned dependency graph requires the real
Metal kernels.

### Disk space and preflight

Leave at least 60 GiB free before downloading Xcode. Once Xcode is installed,
keep at least 40 GiB free for the first Nix build. Later builds reuse the Nix
store.

Run the complete machine check from `apps/amm`:

```sh
bash scripts/check-macos-prerequisites.sh
```

## Build and run

From `apps/amm`:

```sh
nix build .
nix run .
```

The first cold build is large because it includes Qt, the wallet, the AMM
client, and the RISC Zero Metal kernels. The first start opens without a
wallet; select **Connect** to create or open `~/.lee/wallet/`.

## Why the flake is structured this way

Nix 2.25 rejects committed lock entries for unlocked relative flake inputs such
as `path:../shared/wallet` and `path:../..`. Newer Nix versions may accept that
layout, which is why it can appear to work on one developer's Mac and fail on
another.

The portable layout avoids both relative flake inputs:

- the shared wallet is passed to CMake as an ordinary Nix source path;
- the repository-root AMM client package is imported directly with the same
  locked `nixpkgs` and Crane inputs;
- `flake.lock` contains only immutable upstream sources.

The app output uses the builder's full-library QML package layout. The current
`qt-plugin` layout copies the UI plugin and replica factory but omits sibling
external libraries such as `libamm_client`. The module metadata still selects
the C++ backend, so this packaging choice does not change process isolation or
runtime behavior.

Do not replace these with a pull-request URL, self-reference, or local
`path:` flake input. A normal `nix flake lock`, `nix build`, and `nix run`
must work without overrides.

## Known-good validation host

This configuration has been exercised on:

- Apple Silicon (`arm64`)
- macOS 26.5.2
- Xcode 26.6 (build 17F113)
- Metal Toolchain 32023.883
- Nix 2.25.3

The flake also evaluates its complete package and app outputs for
`x86_64-darwin`; an Intel Mac is still required for native runtime validation.

## Recovery

### Nix is unavailable

Open a new terminal after installing Nix. For the standard multi-user
installation, check the store and daemon before trying to bootstrap it again:

```sh
test -x /nix/var/nix/profiles/default/bin/nix
nix store info
sudo launchctl print system/org.nixos.nix-daemon
```

`launchctl bootstrap` can report an I/O error when a service is already loaded
or in an inconsistent state; that message alone does not prove the daemon is
stopped.

### `xcrun` cannot find `metal`

The selected developer directory points at Command Line Tools, or Xcode lacks
the separately downloaded Metal Toolchain component. Repeat the Xcode setup
and verification commands above.

Apple does not distribute the Metal compiler as a standalone redistributable
SDK, so Nix cannot install this host component.

### The build reports that the macOS sandbox blocks Metal

Confirm the active setting:

```sh
nix config show sandbox
```

For this native macOS build it must be `false`. If a machine-wide policy set it
to `true`, update that Nix installation's daemon configuration and restart the
daemon before retrying.

### The first build runs out of space

Remove only disposable build outputs, then collect dead Nix store paths:

```sh
rm -rf /private/tmp/amm-build
nix-store --gc
```

Do not remove `/nix/store` directly.

### A lock error mentions an unlocked input

Restore the committed app lock and verify it without rewriting it:

```sh
git restore flake.lock
nix flake metadata --no-update-lock-file .
```

If the error still names `path:../shared/wallet` or `path:../..`, the checkout
does not contain the portable flake fix described above.
