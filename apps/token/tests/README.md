# Token UI Basecamp E2E

`token-definition.mjs` drives the running Token UI through the Logos QML
inspector, the same path used by Basecamp UI tests.

The hermetic Basecamp runner follows the AMM and Logos Palace harnesses: it
uses the inspector-enabled portable Basecamp bundle, stages portable installs
under a fresh `--user-dir`, launches with `-platform offscreen`, waits for the
inspector, then stores screenshots from the same inspector connection.

The tests submit real transactions, so the runner needs the deployed program's
address and a prepared wallet. It uses the wallet `setup-amm-testnet.sh` restores
(`apps/amm/tests/testnet/.wallet`) unless `TOKEN_E2E_WALLET_HOME` points at
another one, and passes it to both Basecamp and the test. Since LEZ v0.2.5 a
program is addressed by the header account it was deployed to, so the token
module needs `TOKEN_PROGRAM_ID` (it reports `config_missing` without it). The
runner reads it from that wallet's `token-header` account, which the setup
script deploys to; set it to point elsewhere. `TOKEN_PROGRAM_BIN` is optional
and, when set, must be the bytecode deployed there.

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

The test also runs against the standalone app, which listens on the same
inspector port. Launch it against the test wallet and the deployed program:

```bash
export LEE_WALLET_HOME_DIR=$PWD/apps/amm/tests/testnet/.wallet
TOKEN_PROGRAM_ID=$(wallet account id --account-id token-header | grep -m1 -oE '[1-9A-HJ-NP-Za-km-z]{32,44}') \
  nix run .#token-ui
```

It detects the standalone window and skips
the Basecamp sidebar navigation. It switches to a fresh Create form itself,
whichever view the app was left on.

The tests need a connected wallet and a reachable sequencer. Each one creates
fresh public wallet accounts and submits a definition through
`token_module`:

- `token definition: fixed supply`: the fixed template, no mint authority.
- `token definition: self authority`: the same template with "Self authority"
  picked in the mint-authority dropdown, so the definition account is its own
  mint authority (`authority = Some(definition id)` on chain).
- `token definition: external authority`: "External authority" with a freshly
  generated account id entered in base58, so the module's base58-to-hex path is
  covered too (`authority = Some(that id)` on chain). The id is only stored, so it
  needn't exist on chain.
- `token definition: NFT collection`: the NFT template (64 printable copies)
  with a real `data:` URI and creators string. Checks the non-fungible
  definition, the master holding (`print_balance` = printable supply), and the
  metadata account (standard, URI, creators, `primary_sale_date` 0).
- `inspect: filter by token type`: in Inspect, with the search box cleared,
  clicks the Fungible, NFT and All filters. For each one, the rendered
  `tokenDefinitionRow`s must be exactly the page's definitions of that type
  (worked out from the unfiltered list, not the filter code), and All must show
  both kinds. If the wallet holds no chain-read fungible or NFT definition yet,
  it first creates one with the matching create test.

A successful submit only means the sequencer accepted the transaction, so each
create test then checks the result twice:

1. **App read path.** In Inspect it refreshes until `store.liveDefinitions`
   holds a `source: "network"` record for the new definition's account id (the
   draft the app adds on submit is `pending` and never matches). Those records
   come from `token_module`, which only decodes accounts owned by the configured
   Token Program. Name, type, supply, mint authority, metadata, and the holding's
   balance must match what was submitted.
2. **Independent read.** It reads the created accounts with
   `wallet account get --raw`, decodes the Borsh `TokenDefinition` and
   `TokenHolding` (plus `TokenMetadata` for the NFT) itself, and checks the same
   values, that the holding and metadata point at the definition, and that all
   of them share one program owner.

It uses the isolated wallet that `apps/amm/tests/testnet/setup-amm-testnet.sh`
restores (`apps/amm/tests/testnet/.wallet`), like the AMM tests; launch the app
against that wallet too. `LEE_WALLET_HOME_DIR` overrides it. The wallet needs a
funded public account: the fresh target accounts can't pay the sequencer's fee,
so the app picks a funded one as fee payer, and the tests fail early without it.
The `wallet` binary must match this branch's LEZ (v0.2.5-rc2);
`TOKEN_E2E_WALLET` overrides its path.

```bash
node apps/token/tests/token-definition.mjs          # all tests
node apps/token/tests/token-definition.mjs self     # only tests whose name contains "self"
```

`TOKEN_E2E_NAME` is optional and defaults to `Basecamp E2E <timestamp>`; the
other tests append ` (self)`, ` (external)` or ` (nft)`. Screenshot names are prefixed with the
authority mode.

Screenshots are written to `apps/token/tests/screenshots/` (git-ignored) after
each major step; override with `TOKEN_E2E_OUTPUT`. The Basecamp runner writes
to `.3esmit/projects/lez-programs/docs/token-basecamp-e2e/`. The inspector
listens on `localhost:3768`; override it with `QML_INSPECTOR_PORT` when another
test is using that port.
