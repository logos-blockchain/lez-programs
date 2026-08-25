# Token UI Basecamp E2E

`token-definition.mjs` drives the running Token UI through the Logos QML
inspector, the same path used by Basecamp UI tests.

The hermetic Basecamp runner follows the AMM and Logos Palace harnesses: it
uses the inspector-enabled portable Basecamp bundle, stages portable installs
under a fresh `--user-dir`, launches with `-platform offscreen`, waits for the
inspector, isolates the wallet home under the run directory, then stores
screenshots from the same inspector connection.

Run the full Basecamp flow:

```bash
apps/token/tests/run-basecamp-e2e.sh
```

The runner builds these inputs when not supplied through environment
overrides:

- `logos-qt-mcp` test framework
- inspector-enabled Basecamp bundle
- `lez_core` portable core module
- `token_module` portable core module
- `token_ui` portable UI plugin

Override already-built inputs with `LOGOS_QT_MCP`, `TOKEN_BASECAMP_BUNDLE`,
`TOKEN_WALLET_INSTALL`, `TOKEN_MODULE_INSTALL`, or `TOKEN_UI_INSTALL`.

Build only the inspector framework:

```bash
nix build .#test-framework -o apps/token/result-mcp
```

For manual runs, stage the portable install outputs into a Basecamp user
directory: `.#install-portable` for `token_module`,
`.#token-ui-install-portable` for `token_ui`, and the matching portable
`lez_core` install. Launch the inspector-enabled bundle with
`--user-dir <path> -platform offscreen`.

Run the non-mutating visual flow:

```bash
node apps/token/tests/token-definition.mjs
```

Run the live round trip against an open wallet and reachable sequencer:

```bash
TOKEN_E2E_LIVE=1 \
TOKEN_E2E_NAME="Basecamp Token E2E" \
node apps/token/tests/token-definition.mjs
```

The live path creates fresh public wallet accounts, submits a fixed-supply
definition through `token_module`, switches to Inspect, and waits for the
definition to be read back from the connected wallet.

Screenshots are written to `.3esmit/projects/lez-programs/docs/token-basecamp-e2e/`
after each major step. The inspector listens on `localhost:3768`; override it with
`QML_INSPECTOR_PORT` when another test is using that port.
