# AMM UI

A QML UI application for the Automated Market Maker (AMM) program.

See the [Logos QML UI App Tutorial](https://github.com/logos-co/logos-tutorial/blob/master/tutorial-qml-ui-app.md) for more information.

## Setup

This project requires Nix with experimental features enabled. If you haven't already, enable them permanently:

```bash
mkdir -p ~/.config/nix && echo "experimental-features = nix-command flakes" >> ~/.config/nix/nix.conf
```

## Running the UI

Start the UI with:

```bash
nix run .
```

This builds and runs the application in development mode.

## Updating Dependencies

To update the pinned versions of dependencies in `flake.lock`:

```bash
nix flake update
```
