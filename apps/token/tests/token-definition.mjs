// Token UI end-to-end test, standalone (`nix run .#token-ui`) or in Basecamp.
//
// Needs a connected wallet and a reachable sequencer: creates fresh wallet
// accounts, submits a fixed-supply definition through token_module, and checks
// it appears in the Inspect view.

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdir, writeFile } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { randomBytes } from "node:crypto";

const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

const evidenceDir = process.env.TOKEN_E2E_OUTPUT
  ? resolve(process.env.TOKEN_E2E_OUTPUT)
  : fileURLToPath(new URL("./screenshots/", import.meta.url));
const tokenName = process.env.TOKEN_E2E_NAME || `Basecamp E2E ${Date.now()}`;
// The isolated wallet apps/amm/tests/testnet/setup-amm-testnet.sh restores; the
// app must be launched against it too.
const walletHome =
  process.env.LEE_WALLET_HOME_DIR ||
  fileURLToPath(new URL("../../amm/tests/testnet/.wallet", import.meta.url));
const walletBin = process.env.TOKEN_E2E_WALLET || "wallet";

function runWallet(args) {
  return execFileSync(walletBin, args, {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    env: { ...process.env, LEE_WALLET_HOME_DIR: walletHome },
  });
}

async function idByObjectName(app, name) {
  const result = await app.findByProperty("objectName", name);
  if (result.error || !result.matches || result.matches.length === 0)
    throw new Error(`no object with objectName="${name}"`);
  return result.matches[0].id;
}

async function prop(app, objectId, property) {
  const result = await app.getProperties(objectId);
  const properties = result.properties || [];
  const found = properties.find((item) => item.name === property);
  return found ? found.value : undefined;
}

async function setProp(app, objectId, property, value) {
  const result = await app.inspector.send("setProperty", {
    objectId,
    property,
    value,
  });
  if (result.error)
    throw new Error(`setProperty ${property}: ${result.error}`);
}

// The inspector clicks by posting mouse events at the item's window position, so
// an item scrolled out of view gets missed. Scroll its nearest Flickable first.
// Evaluated with the item as scope, so `parent`, `height` and `mapToItem` are its own.
const REVEAL_EXPRESSION = `(function () {
  var flick = parent;
  while (flick && (flick.contentY === undefined || flick.contentItem === undefined))
    flick = flick.parent;
  if (!flick)
    return -1;
  var y = mapToItem(flick.contentItem, 0, 0).y;
  if (y < flick.contentY || y + height > flick.contentY + flick.height)
    flick.contentY = Math.max(0, Math.min(y - flick.height / 3, flick.contentHeight - flick.height));
  return flick.contentY;
})()`;

async function revealAndClick(app, objectId, name) {
  const revealed = await app.inspector.send("evaluate", { objectId, expression: REVEAL_EXPRESSION });
  if (revealed.error)
    throw new Error(`scroll ${name} into view: ${revealed.error}`);
  const result = await app.inspector.send("click", { objectId });
  if (result.error)
    throw new Error(`click ${name}: ${result.error}`);
}

async function clickObject(app, name) {
  const objectId = await idByObjectName(app, name);
  await revealAndClick(app, objectId, name);
  return objectId;
}

async function clickFirstVisible(app, names) {
  for (const name of names) {
    const result = await app.findByProperty("objectName", name);
    for (const match of result.matches || []) {
      if ((await prop(app, match.id, "visible")) !== true)
        continue;
      if ((await prop(app, match.id, "enabled")) === false)
        continue;
      await revealAndClick(app, match.id, name);
      return match.id;
    }
  }
  throw new Error(`no visible enabled object: ${names.join(", ")}`);
}

async function waitForProperty(app, objectId, property, predicate, description, timeout = 10000) {
  await app.waitFor(
    async () => {
      const value = await prop(app, objectId, property);
      if (!predicate(value))
        throw new Error(`${property}=${JSON.stringify(value)}`);
    },
    { timeout, interval: 300, description },
  );
}

async function saveShot(app, name) {
  const shot = await app.screenshot();
  if (!shot || !shot.image)
    throw new Error(`screenshot unavailable for ${name}`);
  await mkdir(evidenceDir, { recursive: true });
  const path = resolve(evidenceDir, `${name}.png`);
  await writeFile(path, Buffer.from(shot.image, "base64"));
  console.log(`    screenshot -> ${path}`);
}

const BASE58 = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

function hexToBase58(hex) {
  const bytes = Buffer.from(hex, "hex");
  let value = BigInt(`0x${hex || "0"}`);
  let out = "";
  while (value > 0n) {
    out = BASE58[Number(value % 58n)] + out;
    value /= 58n;
  }
  for (const byte of bytes) {
    if (byte !== 0) break;
    out = `1${out}`;
  }
  return out;
}

// Minimal Borsh reader for the token_core account layouts.
function borshReader(buffer) {
  let offset = 0;
  const take = (length) => {
    if (offset + length > buffer.length)
      throw new Error(`account data too short at byte ${offset}`);
    const slice = buffer.subarray(offset, offset + length);
    offset += length;
    return slice;
  };
  return {
    u8: () => take(1)[0],
    u128: () => {
      const bytes = take(16);
      return (bytes.readBigUInt64LE(8) << 64n) | bytes.readBigUInt64LE(0);
    },
    u64: () => take(8).readBigUInt64LE(0),
    accountId: () => take(32).toString("hex"),
    string: () => take(take(4).readUInt32LE(0)).toString("utf8"),
    option: (read) => (take(1)[0] === 1 ? read() : null),
    done: () => offset === buffer.length,
  };
}

function decodeFungibleDefinition(data) {
  const reader = borshReader(data);
  const tag = reader.u8();
  if (tag !== 0)
    throw new Error(`definition variant ${tag} is not Fungible`);
  const definition = {
    name: reader.string(),
    totalSupply: reader.u128(),
    metadataId: reader.option(reader.accountId),
    authority: reader.option(reader.accountId),
  };
  if (!reader.done())
    throw new Error("trailing bytes after TokenDefinition");
  return definition;
}

function decodeFungibleHolding(data) {
  const reader = borshReader(data);
  const tag = reader.u8();
  if (tag !== 0)
    throw new Error(`holding variant ${tag} is not Fungible`);
  const holding = { definitionId: reader.accountId(), balance: reader.u128() };
  if (!reader.done())
    throw new Error("trailing bytes after TokenHolding");
  return holding;
}

function decodeNonFungibleDefinition(data) {
  const reader = borshReader(data);
  const tag = reader.u8();
  if (tag !== 1)
    throw new Error(`definition variant ${tag} is not NonFungible`);
  const definition = {
    name: reader.string(),
    printableSupply: reader.u128(),
    metadataId: reader.accountId(),
  };
  if (!reader.done())
    throw new Error("trailing bytes after TokenDefinition");
  return definition;
}

function decodeNftMasterHolding(data) {
  const reader = borshReader(data);
  const tag = reader.u8();
  if (tag !== 1)
    throw new Error(`holding variant ${tag} is not NftMaster`);
  const holding = { definitionId: reader.accountId(), printBalance: reader.u128() };
  if (!reader.done())
    throw new Error("trailing bytes after TokenHolding");
  return holding;
}

const METADATA_STANDARDS = ["Simple", "Expanded"];

function decodeMetadata(data) {
  const reader = borshReader(data);
  const metadata = {
    definitionId: reader.accountId(),
    standard: METADATA_STANDARDS[reader.u8()],
    uri: reader.string(),
    creators: reader.string(),
    primarySaleDate: reader.u64(),
  };
  if (!reader.done())
    throw new Error("trailing bytes after TokenMetadata");
  return metadata;
}

// The app reports ids in base58 or hex depending on the field.
function sameAccount(value, hex) {
  const id = String(value || "");
  return id.toLowerCase() === hex || id === hexToBase58(hex);
}

function expectEqual(actual, expected, what) {
  if (actual !== expected)
    throw new Error(`${what}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
}

// Read an account through the `wallet` CLI, independent of the app's read path.
function walletAccount(accountHex) {
  const output = runWallet(["account", "get", "--raw", "--account-id", `Public/${hexToBase58(accountHex)}`]);
  const start = output.indexOf("{");
  if (start < 0)
    throw new Error(`chain: account ${accountHex} is not initialized (${output.trim()})`);
  // The wallet prints status lines around the JSON (e.g. "Stored statistics at …").
  const account = JSON.parse(output.slice(start, output.lastIndexOf("}") + 1));
  return { owner: account.program_owner, data: Buffer.from(account.data, "hex") };
}

// Chain-backed record for `definitionHex` from the Inspect view. Drafts added on
// submit have source "pending" and never match.
async function networkDefinition(app, definitionHex) {
  const pageId = await idByObjectName(app, "tokenManagePage");
  const result = await app.inspector.send("evaluate", {
    objectId: pageId,
    expression: "JSON.stringify(store ? store.liveDefinitions : [])",
  });
  if (result.error)
    throw new Error(`evaluate liveDefinitions: ${result.error}`);
  const records = JSON.parse(result.result || "[]");
  return records.find(
    (record) =>
      record.source === "network"
      && String(record.definitionHex || "").toLowerCase() === definitionHex,
  );
}

function verifyFungibleRecord(record, expected) {
  expectEqual(record.name, expected.name, "app: definition name");
  expectEqual(record.type, "fungible", "app: definition type");
  expectEqual(record.rawSupply, expected.supply, "app: total supply");
  expectEqual(record.authorityMode, expected.authorityMode, "app: mint authority mode");
  if (expected.authorityMode === "self")
    expectEqual(record.authority, record.id, "app: mint authority is the definition");
  if (expected.authorityMode === "external" && !sameAccount(record.authority, expected.authorityHex))
    throw new Error(`app: mint authority ${record.authority} is not ${expected.authorityHex}`);
  expectEqual(record.metadataId || "", "", "app: metadata id");
  const holding = findHolding(record, expected.holdingHex);
  expectEqual(holding.role, "fungible", "app: holding kind");
  expectEqual(holding.rawBalance, expected.supply, "app: holding balance");
}

function verifyFungibleOnChain(expected) {
  const definitionAccount = walletAccount(expected.definitionHex);
  const holdingAccount = walletAccount(expected.holdingHex);
  expectEqual(holdingAccount.owner, definitionAccount.owner, "chain: holding and definition owner");

  const definition = decodeFungibleDefinition(definitionAccount.data);
  expectEqual(definition.name, expected.name, "chain: definition name");
  expectEqual(definition.totalSupply, BigInt(expected.supply), "chain: total supply");
  expectEqual(definition.metadataId, null, "chain: metadata id");
  expectEqual(
    definition.authority,
    expected.authorityMode === "fixed" ? null : expected.authorityHex,
    "chain: mint authority",
  );

  const holding = decodeFungibleHolding(holdingAccount.data);
  expectEqual(holding.definitionId, expected.definitionHex, "chain: holding definition id");
  expectEqual(holding.balance, BigInt(expected.supply), "chain: holding balance");
  return definitionAccount.owner;
}

function findHolding(record, holdingHex) {
  const holding = (record.holdings || []).find((item) => sameAccount(item.id, holdingHex));
  if (!holding)
    throw new Error(`app: holding ${holdingHex} not listed under the definition`);
  return holding;
}

function verifyNftRecord(record, expected) {
  expectEqual(record.name, expected.name, "app: definition name");
  expectEqual(record.type, "nonFungible", "app: definition type");
  expectEqual(record.printableCopies, expected.supply, "app: printable supply");
  if (!sameAccount(record.metadataId, expected.metadataHex))
    throw new Error(`app: metadata id ${record.metadataId} is not ${expected.metadataHex}`);
  const metadata = record.metadata || {};
  expectEqual(metadata.standard, expected.standard, "app: metadata standard");
  expectEqual(metadata.uri, expected.uri, "app: metadata uri");
  expectEqual(metadata.creators, expected.creators, "app: metadata creators");
  const holding = findHolding(record, expected.holdingHex);
  expectEqual(holding.role, "nftMaster", "app: holding kind");
  expectEqual(holding.printBalance, expected.supply, "app: master print balance");
}

function verifyNftOnChain(expected) {
  const definitionAccount = walletAccount(expected.definitionHex);
  const holdingAccount = walletAccount(expected.holdingHex);
  const metadataAccount = walletAccount(expected.metadataHex);
  expectEqual(holdingAccount.owner, definitionAccount.owner, "chain: holding and definition owner");
  expectEqual(metadataAccount.owner, definitionAccount.owner, "chain: metadata and definition owner");

  const definition = decodeNonFungibleDefinition(definitionAccount.data);
  expectEqual(definition.name, expected.name, "chain: definition name");
  expectEqual(definition.printableSupply, BigInt(expected.supply), "chain: printable supply");
  expectEqual(definition.metadataId, expected.metadataHex, "chain: metadata id");

  const holding = decodeNftMasterHolding(holdingAccount.data);
  expectEqual(holding.definitionId, expected.definitionHex, "chain: holding definition id");
  expectEqual(holding.printBalance, BigInt(expected.supply), "chain: master print balance");

  const metadata = decodeMetadata(metadataAccount.data);
  expectEqual(metadata.definitionId, expected.definitionHex, "chain: metadata definition id");
  expectEqual(metadata.standard, expected.standard, "chain: metadata standard");
  expectEqual(metadata.uri, expected.uri, "chain: metadata uri");
  expectEqual(metadata.creators, expected.creators, "chain: metadata creators");
  expectEqual(metadata.primarySaleDate, 0n, "chain: primary sale date");
  return definitionAccount.owner;
}

async function selectTab(app, tabName, pageName) {
  await clickObject(app, tabName);
  const pageId = await idByObjectName(app, pageName);
  await waitForProperty(app, pageId, "visible", (value) => value === true, `${pageName} to show`);
  return pageId;
}

// Start from a fresh create form whatever view an earlier run left open: the
// create page resets to step 0 when it becomes visible, so pass through Inspect.
async function openFreshCreateView(app) {
  await selectTab(app, "tokenInspectTab", "tokenManagePage");
  const createPageId = await selectTab(app, "tokenCreateTab", "tokenCreatePage");
  await waitForProperty(app, createPageId, "step", (value) => value === 0, "create form at step 0");
  return createPageId;
}

// Pick a mint-authority option the way a user does: open the dropdown and click it.
async function chooseAuthority(app, createPageId, index, label) {
  await clickObject(app, "tokenAuthoritySelector");
  await app.waitFor(
    async () => {
      const result = await app.findByProperty("text", label);
      if (!result.matches || result.matches.length === 0)
        throw new Error("authority options not shown");
    },
    { timeout: 3000, interval: 150, description: "authority dropdown to open" },
  );
  await app.click(label, { exact: true });
  await waitForProperty(app, createPageId, "authorityMode", (value) => value === index, `authority "${label}"`);
}

const AUTHORITY_OPTIONS = {
  fixed: { index: 0, label: "Fixed supply — no future minting" },
  self: { index: 1, label: "Self authority — definition account mints" },
  external: { index: 2, label: "External authority — another account mints" },
};

async function chooseTemplate(app, templateName, expectedName) {
  await clickObject(app, "tokenExamplesButton");
  await app.waitFor(
    async () => {
      const menuId = await idByObjectName(app, "tokenExamplesMenu");
      if ((await prop(app, menuId, "visible")) !== true)
        throw new Error("template menu is closed");
    },
    { timeout: 3000, interval: 150, description: "template menu to open" },
  );
  await clickObject(app, templateName);
  const nameFieldId = await idByObjectName(app, "tokenNameField");
  await waitForProperty(app, nameFieldId, "text", (value) => value === expectedName, `${templateName} values`);
  return nameFieldId;
}

// Fields without an objectName, found by their unique placeholder.
async function setFieldByPlaceholder(app, placeholder, value) {
  const result = await app.findByProperty("placeholderText", placeholder);
  if (result.error || !result.matches || result.matches.length === 0)
    throw new Error(`field "${placeholder}" not found`);
  await setProp(app, result.matches[0].id, "text", value);
}

async function isStandalone(app) {
  const result = await app.findByProperty("objectName", "tokenCreatePage");
  return !result.error && (result.matches || []).length > 0;
}

async function openTokenApp(app) {
  // `nix run .#token-ui` opens the app directly; Basecamp needs the sidebar.
  if (await isStandalone(app)) {
    await saveShot(app, "standalone-launch");
    return;
  }
  await app.waitFor(
    async () => { await app.expectTexts(["Applications", "Settings"]); },
    { timeout: 60000, interval: 500, description: "Basecamp shell to load" },
  );
  await saveShot(app, "basecamp-launch");

  const labels = ["Token", "token_ui", "Logos Token"];
  await app.waitFor(
    async () => {
      for (const label of labels) {
        const result = await app.findByProperty("text", label);
        if (result.matches && result.matches.length > 0) {
          await app.click(label);
          return;
        }
      }
      throw new Error(`Token app not visible in Basecamp sidebar (${labels.join(", ")})`);
    },
    { timeout: 30000, interval: 500, description: "Token app in Basecamp sidebar" },
  );
}

// Fail before touching the UI if the wallet CLI can't read the test wallet:
// otherwise a definition is created on chain and only the final check fails.
function preflightWallet() {
  try {
    runWallet(["account", "get", "--raw", "--account-id", `Public/${hexToBase58("00".repeat(32))}`]);
  } catch (error) {
    const detail = String(error.stderr || error.stdout || error.message).trim();
    throw new Error(`${walletBin} can't read ${walletHome} (run setup-amm-testnet.sh first): ${detail}`);
  }
}

const KIND_TABS = [
  { tab: "tokenFungibleTab", text: "Fungible" },
  { tab: "tokenNonFungibleTab", text: "Non-fungible" },
];

// The kind tab survives the create page's reset, so select it every time.
async function selectKind(app, createPageId, kind) {
  await clickObject(app, KIND_TABS[kind].tab);
  await waitForProperty(app, createPageId, "tokenKind", (value) => value === kind, `${KIND_TABS[kind].text} tab`);
}

// Shared create flow. `label` prefixes the log lines and screenshots; `kind` is
// 0 for fungible, 1 for non-fungible.
async function startCreate(app, label, kind) {
  const shot = (step) => saveShot(app, `${label}-${step}`);
  let phaseStart = Date.now();
  const phase = (what) => {
    const now = Date.now();
    console.log(`    ${label}: ${what} in ${((now - phaseStart) / 1000).toFixed(1)}s`);
    phaseStart = now;
  };
  preflightWallet();
  phase("wallet preflight");
  await openTokenApp(app);
  const createPageId = await openFreshCreateView(app);
  await app.waitFor(
    async () => { await app.expectTexts(["Create definition", "Fungible", "Use template"]); },
    { timeout: 20000, interval: 400, description: "token create view to load" },
  );
  await selectKind(app, createPageId, kind);
  phase("fresh create view");
  await shot("create-initial");
  return { createPageId, shot, phase };
}

// Continue to the account step, create fresh wallet accounts for `fields`
// (target field objectNames), and return their ids as lowercase hex.
async function createTargetAccounts(flow, app, fields) {
  const { createPageId, shot, phase } = flow;
  await clickFirstVisible(app, ["tokenContinueButton", "tokenSummaryContinueButton"]);
  await waitForProperty(app, createPageId, "step", (value) => value === 1, "account-target step");

  const createAccountsButtonId = await idByObjectName(app, "tokenCreateAccountsButton");
  await waitForProperty(
    app,
    createAccountsButtonId,
    "enabled",
    (value) => value === true,
    "connected wallet for fresh accounts",
    5000,
  );
  await clickObject(app, "tokenCreateAccountsButton");

  const ids = [];
  for (const field of fields) {
    const fieldId = await idByObjectName(app, field);
    await waitForProperty(app, fieldId, "text", (value) => typeof value === "string" && value.length > 0, field, 20000);
    const hex = String(await prop(app, fieldId, "text")).toLowerCase();
    if (!/^[0-9a-f]{64}$/.test(hex))
      throw new Error(`${field} is not a 32-byte hex id: ${hex}`);
    ids.push(hex);
  }
  phase("target accounts created");
  await shot("account-targets-ready");
  return ids;
}

async function reviewAndSubmit(flow, app) {
  const { createPageId, shot, phase } = flow;
  await clickObject(app, "tokenReviewButton");
  await waitForProperty(app, createPageId, "step", (value) => value === 2, "definition review");
  await shot("definition-review");

  const prepareButtonId = await idByObjectName(app, "tokenPrepareButton");
  await waitForProperty(app, prepareButtonId, "enabled", (value) => value === true, "create definition action");
  await clickObject(app, "tokenPrepareButton");
  await app.waitFor(
    async () => {
      const prepared = await prop(app, createPageId, "prepared");
      const error = await prop(app, createPageId, "errorMessage");
      if (prepared !== true && !error)
        throw new Error("transaction is still pending");
      if (error)
        throw new Error(`Token Program rejected request: ${error}`);
    },
    { timeout: 60000, interval: 500, description: "Token Program submission" },
  );
  phase("definition submitted");
  await shot("definition-submitted");
}

async function readBack(flow, app, definitionHex) {
  const { shot, phase } = flow;
  await selectTab(app, "tokenInspectTab", "tokenManagePage");
  await app.waitFor(
    async () => { await app.expectTexts(["Inspect token definitions", "Definition index"]); },
    { timeout: 10000, interval: 300, description: "Inspect view to load" },
  );
  // The type filter persists too; show all so the screenshot includes the new definition.
  await clickObject(app, "tokenAllFilterButton");
  const managePageId = await idByObjectName(app, "tokenManagePage");
  await waitForProperty(app, managePageId, "typeFilter", (value) => value === "all", "Inspect filter on All");
  let record;
  await app.waitFor(
    async () => {
      try { await clickObject(app, "tokenRefreshButton"); } catch { /* refresh may be busy */ }
      record = await networkDefinition(app, definitionHex);
      if (!record)
        throw new Error(`definition ${definitionHex} not read from the chain yet`);
    },
    { timeout: 60000, interval: 1200, description: "created definition read back from the chain" },
  );
  phase("read back by the app");
  await shot("inspect-definition");
  return record;
}

function supplyOf(value, what) {
  const supply = String(value);
  if (!/^[1-9][0-9]*$/.test(supply))
    throw new Error(`${what} is not a positive integer: ${supply}`);
  return supply;
}

async function createFungibleAndVerify(app, authorityMode) {
  const name = authorityMode === "fixed" ? tokenName : `${tokenName} (${authorityMode})`;
  const flow = await startCreate(app, authorityMode, 0);
  const { createPageId, phase } = flow;

  const nameFieldId = await chooseTemplate(app, "tokenFixedTemplate", "Fixed supply token");
  await setProp(app, nameFieldId, "text", name);
  const authority = AUTHORITY_OPTIONS[authorityMode];
  if (authority.index !== 0)
    await chooseAuthority(app, createPageId, authority.index, authority.label);
  // A fresh id: the authority is only stored, so it needn't exist on chain.
  const externalAuthorityHex = randomBytes(32).toString("hex");
  if (authorityMode === "external") {
    await setFieldByPlaceholder(app, "External authority account ID", hexToBase58(externalAuthorityHex));
    await waitForProperty(app, createPageId, "validExternalAuthority", (value) => value === true, "external authority accepted");
  }
  await flow.shot("create-template");

  const [definitionHex, holdingHex] = await createTargetAccounts(flow, app, [
    "tokenDefinitionTargetField",
    "tokenHoldingTargetField",
  ]);
  const expected = {
    name,
    authorityMode,
    supply: supplyOf(await prop(app, await idByObjectName(app, "tokenSupplyField"), "text"), "fixed template supply"),
    definitionHex,
    holdingHex,
    authorityHex: { fixed: null, self: definitionHex, external: externalAuthorityHex }[authorityMode],
  };

  await reviewAndSubmit(flow, app);
  const record = await readBack(flow, app, definitionHex);
  verifyFungibleRecord(record, expected);
  console.log(`    ${authorityMode}: app read back ${definitionHex}: ${record.name}, supply ${record.rawSupply}`);

  const owner = verifyFungibleOnChain(expected);
  phase("read back by the wallet CLI");
  console.log(`    ${authorityMode}: wallet CLI read back both accounts; owner ${owner}`);
}

async function createNftAndVerify(app) {
  const name = `${tokenName} (nft)`;
  const uri = `data:application/json;base64,${Buffer.from(JSON.stringify({ name, description: "Token UI e2e NFT" })).toString("base64")}`;
  const creators = "Token UI e2e";
  const flow = await startCreate(app, "nft", 1);
  const { createPageId, phase } = flow;

  const nameFieldId = await chooseTemplate(app, "tokenNftTemplate", "NFT collection");
  await waitForProperty(app, createPageId, "tokenKind", (value) => value === 1, "template kept non-fungible");
  await setProp(app, nameFieldId, "text", name);
  await setFieldByPlaceholder(app, "data:application/json;base64,... or external URI", uri);
  await setFieldByPlaceholder(app, "Creator account or attribution string", creators);
  const standard = METADATA_STANDARDS[await prop(app, createPageId, "metadataStandard")];
  await flow.shot("create-template");

  const [definitionHex, holdingHex, metadataHex] = await createTargetAccounts(flow, app, [
    "tokenDefinitionTargetField",
    "tokenHoldingTargetField",
    "tokenMetadataTargetField",
  ]);
  const expected = {
    name,
    supply: supplyOf(await prop(app, await idByObjectName(app, "tokenSupplyField"), "text"), "NFT template printable supply"),
    standard,
    uri,
    creators,
    definitionHex,
    holdingHex,
    metadataHex,
  };

  await reviewAndSubmit(flow, app);
  const record = await readBack(flow, app, definitionHex);
  verifyNftRecord(record, expected);
  console.log(`    nft: app read back ${definitionHex}: ${record.name}, ${record.printableCopies} printable`);

  const owner = verifyNftOnChain(expected);
  phase("read back by the wallet CLI");
  console.log(`    nft: wallet CLI read back definition, master holding and metadata; owner ${owner}`);
}

async function evaluateJson(app, objectId, expression, what) {
  const result = await app.inspector.send("evaluate", { objectId, expression: `JSON.stringify(${expression})` });
  if (result.error)
    throw new Error(`${what}: ${result.error}`);
  return JSON.parse(result.result ?? "null");
}

// Inspect with no search text, the All filter, and a finished chain read.
async function openInspect(app) {
  const pageId = await selectTab(app, "tokenInspectTab", "tokenManagePage");
  await setProp(app, await idByObjectName(app, "tokenSearchField"), "text", "");
  await clickObject(app, "tokenAllFilterButton");
  await waitForProperty(app, pageId, "typeFilter", (value) => value === "all", "Inspect filter on All");
  await waitForProperty(app, pageId, "loading", (value) => value === false, "earlier read to finish", 60000);
  await clickObject(app, "tokenRefreshButton");
  await app.waitFor(
    async () => {
      const done = await evaluateJson(app, pageId, "store.liveDefinitionsLoaded && !loading", "read state");
      if (!done)
        throw new Error("definitions are still loading");
    },
    { timeout: 60000, interval: 500, description: "live definitions to load" },
  );
  return pageId;
}

// Every definition the page knows, before the type filter is applied.
function pageDefinitions(app, pageId) {
  return evaluateJson(
    app,
    pageId,
    "definitions.map(function (d) { return { id: d.id, type: d.type, source: d.source }; })",
    "page definitions",
  );
}

async function renderedRows(app) {
  const result = await app.findByProperty("objectName", "tokenDefinitionRow");
  const rows = [];
  for (const match of result.matches || []) {
    if ((await prop(app, match.id, "visible")) !== true)
      continue;
    rows.push(await evaluateJson(app, match.id, "({ id: modelData.id, type: modelData.type })", "row data"));
  }
  return rows;
}

const rowKey = (row) => `${row.type}:${row.id}`;

const TYPE_FILTERS = [
  { button: "tokenFungibleFilterButton", value: "fungible", types: ["fungible"] },
  { button: "tokenNftFilterButton", value: "nonFungible", types: ["nonFungible"] },
  { button: "tokenAllFilterButton", value: "all", types: ["fungible", "nonFungible"] },
];

async function filterByType(app) {
  preflightWallet();
  await openTokenApp(app);
  let pageId = await openInspect(app);
  let definitions = await pageDefinitions(app, pageId);
  const hasNetwork = (type) => definitions.some((d) => d.source === "network" && d.type === type);
  // Self-contained: create whichever kind the wallet doesn't hold yet.
  if (!hasNetwork("fungible") || !hasNetwork("nonFungible")) {
    if (!hasNetwork("fungible"))
      await createFungibleAndVerify(app, "fixed");
    if (!hasNetwork("nonFungible"))
      await createNftAndVerify(app);
    pageId = await openInspect(app);
    definitions = await pageDefinitions(app, pageId);
  }

  for (const filter of TYPE_FILTERS) {
    await clickObject(app, filter.button);
    await waitForProperty(app, pageId, "typeFilter", (value) => value === filter.value, `${filter.value} filter`);
    const expected = definitions.filter((d) => filter.types.includes(d.type)).map(rowKey).sort();
    if (expected.length === 0)
      throw new Error(`no ${filter.value} definitions to filter for`);
    let rows = [];
    await app.waitFor(
      async () => {
        rows = await renderedRows(app);
        const shown = rows.map(rowKey).sort();
        if (JSON.stringify(shown) !== JSON.stringify(expected))
          throw new Error(`${filter.value}: showing ${shown.length} rows, expected ${expected.length}`);
      },
      { timeout: 10000, interval: 300, description: `${filter.value} rows` },
    );
    const stray = rows.filter((row) => !filter.types.includes(row.type));
    if (stray.length > 0)
      throw new Error(`${filter.value} filter shows ${stray.map(rowKey).join(", ")}`);
    for (const type of filter.types) {
      if (!rows.some((row) => row.type === type))
        throw new Error(`${filter.value} filter shows no ${type} definitions`);
    }
    await saveShot(app, `filter-${filter.value}`);
    console.log(`    filter ${filter.value}: ${rows.length} rows`);
  }
}

test("token definition: fixed supply", (app) => createFungibleAndVerify(app, "fixed"));
test("token definition: self authority", (app) => createFungibleAndVerify(app, "self"));
test("token definition: external authority", (app) => createFungibleAndVerify(app, "external"));
test("token definition: NFT collection", (app) => createNftAndVerify(app));
test("inspect: filter by token type", (app) => filterByType(app));

run();
