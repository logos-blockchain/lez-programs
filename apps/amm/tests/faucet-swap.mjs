// ---------------------------------------------------------------------------
// AMM UI test — a BRAND-NEW account funds itself via the token-mint-authority
// faucet, then swaps.
//
// The story this proves end-to-end:
//   1. "Token A Holder 2" (the setup's holder2 / holder2-a-holding pair) starts
//      with NO token A, so it is absent from the swap's sell-account picker.
//   2. We submit a FaucetMint through the token-mint-authority program (via spel,
//      out of band from the UI — the UI has no faucet feature) which mints token A
//      to holder2's holding. This is the whole point of setting every test token's
//      mint_authority to the faucet PDA in setup-amm-testnet.sh.
//   3. Because the app already loaded its holdings snapshot BEFORE the mint, the new
//      holding does not appear on its own — the UI must be REFRESHED. We call
//      backend.refreshBalances() (re-scan the wallet from the sequencer) followed by
//      swapPage.refreshHoldings() (re-pull tokenHoldings() into the card), after
//      which holder2's holding shows up in the sell picker.
//   4. We select holder2's freshly-funded holding and swap token A -> token B,
//      verifying the A/B pool reserves changed ON-CHAIN.
//
// Prereqs (same isolated setup as swap.mjs — see apps/amm/README.md and
// apps/amm/tests/testnet/setup-amm-testnet.sh):
//   * setup-amm-testnet.sh has run: programs deployed, A/B pool seeded, the faucet
//     deployed, and apps/amm/tests/testnet/faucet.json written.
//   * the app is running against the isolated test wallet + test token config.
//   * `spel` is on PATH and LEE_WALLET_HOME_DIR points at the SAME isolated wallet
//     the app uses (defaults to apps/amm/tests/testnet/.wallet if unset), so the
//     faucet mint is signed by the same keys.
// ---------------------------------------------------------------------------

import { resolve } from "node:path";
import { writeFile } from "node:fs/promises";
import { readFileSync } from "node:fs";
import { execFileSync } from "node:child_process";

// Locate the test framework (see swap.mjs for the rationale / override env var).
const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

const SELL_AMOUNT = "100"; // same token A sell size swap.mjs uses against this pool

// Repo root is three levels up from apps/amm/tests/. The manifest stores bin/IDL
// paths repo-relative, so spel is invoked with cwd = repoRoot.
const REPO_ROOT = new URL("../../..", import.meta.url).pathname;
const MANIFEST_PATH = new URL("./testnet/faucet.json", import.meta.url).pathname;

// The isolated test wallet (matches setup-amm-testnet.sh's TEST_WALLET_HOME). spel
// must sign the FaucetMint with the same keys the app uses.
const WALLET_HOME =
  process.env.LEE_WALLET_HOME_DIR ||
  new URL("./testnet/.wallet", import.meta.url).pathname;
// The isolated wallet stores its keys in the clear (see setup), so spel signs
// non-interactively; feed the test password on stdin anyway in case a build of
// spel prompts, so the test never hangs.
const WALLET_PASSWORD = process.env.TEST_WALLET_PASSWORD ?? "test";

// The swap's account model exposes each holding's id as hex (the FFI does
// hex::encode(account_id), and the backend takes *Hex holding ids), while the
// manifest — and the wallet — speak base58. Decode base58 -> 64-char lowercase hex
// so we can match/select holder2's holding in the selector. Both encode the SAME
// 32 bytes big-endian; padStart(64) preserves leading zero bytes (holder2's id has
// one, which is why its base58 starts with '1').
const B58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
function base58ToHex(s) {
  let num = 0n;
  for (const ch of s) {
    const idx = B58_ALPHABET.indexOf(ch);
    if (idx < 0) throw new Error(`invalid base58 character '${ch}' in "${s}"`);
    num = num * 58n + BigInt(idx);
  }
  const hex = num.toString(16).padStart(64, "0");
  if (hex.length !== 64) throw new Error(`"${s}" did not decode to 32 bytes`);
  return hex;
}

// --- manifest ---------------------------------------------------------------

function loadManifest() {
  let raw;
  try {
    raw = readFileSync(MANIFEST_PATH, "utf8");
  } catch {
    throw new Error(
      `faucet manifest not found at ${MANIFEST_PATH} — run apps/amm/tests/testnet/setup-amm-testnet.sh first`,
    );
  }
  const m = JSON.parse(raw);
  for (const key of [
    "tokenBin", "tokenIdl", "faucetBin", "faucetIdl", "recipient", "mintAllowance",
    "userHolding", "tokenDefinition", "mintAuthority", "clock",
  ]) {
    if (!m[key]) throw new Error(`faucet manifest is missing "${key}"`);
  }
  return m;
}

// --- small helpers over the raw inspector commands (mirrors swap.mjs) --------

const ignore = async (fn) => { try { return await fn(); } catch { /* best effort */ } };

async function idByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0)
    throw new Error(`no object with objectName="${name}"`);
  return res.matches[0].id;
}

async function prop(app, id, name) {
  const props = (await app.getProperties(id)).properties || [];
  const p = props.find((x) => x.name === name);
  return p ? p.value : undefined;
}

async function pickerOpen(app) {
  const id = await idByObjectName(app, "tokenSelectorModal");
  return (await prop(app, id, "visible")) === true;
}

async function openPicker(app, buttonObjectName) {
  const btnId = await idByObjectName(app, buttonObjectName);
  await app.inspector.send("click", { objectId: btnId });
  await app.waitFor(
    async () => { if (!(await pickerOpen(app))) throw new Error("picker not open"); },
    { timeout: 5000, interval: 200, description: `open ${buttonObjectName}` },
  );
}

async function tokenItems(app) {
  const res = await app.findByProperty("objectName", "tokenListItem");
  return (res && res.matches) || [];
}

async function pickToken(app, index) {
  const items = await tokenItems(app);
  if (items.length <= index)
    throw new Error(`token #${index + 1} not found — only ${items.length} token(s) in the list`);
  const id = items[index].id;
  const symbol = await prop(app, id, "tokenSymbol");
  await app.inspector.send("click", { objectId: id });
  await app.waitFor(
    async () => { if (await pickerOpen(app)) throw new Error("picker still open"); },
    { timeout: 5000, interval: 200, description: `select token #${index + 1}` },
  );
  return symbol;
}

// Pick the funding account by highest balance (swap.mjs's selectAccount, used here
// for the BUY side only — the SELL side is pinned to holder2 explicitly below).
async function selectAccountByBalance(app, selectorObjectName) {
  const id = await idByObjectName(app, selectorObjectName);
  await app.waitFor(
    async () => { if ((await prop(app, id, "hasFunds")) !== true) throw new Error("no matching holdings yet"); },
    { timeout: 10000, interval: 300, description: `${selectorObjectName} holdings to load` },
  );
  await app.inspector.send("evaluate", {
    expression:
      "(function(){var r=matchingAccounts,b=r[0],bb=String(valueFor(b, 'balanceRaw')||'0');"
      + "for(var i=1;i<r.length;++i){var v=String(valueFor(r[i], 'balanceRaw')||'0');"
      + "if(v.length>bb.length||(v.length===bb.length&&v>bb)){b=r[i];bb=v;}}"
      + "setSelection(accountIdFor(b),false);})()",
    objectId: id,
  });
  await app.waitFor(
    async () => { if (!(await prop(app, id, "selectedAccountId"))) throw new Error("holding not selected yet"); },
    { timeout: 5000, interval: 200, description: `${selectorObjectName} holding selected` },
  );
}

// Probe whether a specific account id is currently selectable in a selector, WITHOUT
// leaving a valid selection behind for the caller to reason about: set the selection
// to accountId, read whether the selector considers it valid (i.e. present in
// matchingAccounts). Used to show the before/after of the refresh.
async function probeSelectable(app, selectorObjectName, accountId) {
  const id = await idByObjectName(app, selectorObjectName);
  await app.inspector.send("evaluate", {
    expression: `setSelection(${JSON.stringify(accountId)}, false)`,
    objectId: id,
  });
  const valid = await prop(app, id, "selectionValid");
  const selected = String((await prop(app, id, "selectedAccountId")) || "");
  return valid === true && selected === accountId;
}

// Select a specific holding by account id and wait until the selector accepts it
// (present in matchingAccounts). Re-issues setSelection each poll because the account
// list can still be settling after a refresh.
async function selectHoldingById(app, selectorObjectName, accountId) {
  const id = await idByObjectName(app, selectorObjectName);
  await app.waitFor(
    async () => {
      await app.inspector.send("evaluate", {
        expression: `setSelection(${JSON.stringify(accountId)}, false)`,
        objectId: id,
      });
      const valid = await prop(app, id, "selectionValid");
      const selected = String((await prop(app, id, "selectedAccountId")) || "");
      if (valid !== true || selected !== accountId)
        throw new Error("holding not selectable yet");
    },
    { timeout: 20000, interval: 600, description: `select holding ${accountId} in ${selectorObjectName}` },
  );
}

// Re-pull the wallet's holdings into the running UI: (1) refreshBalances() re-scans
// the wallet from the sequencer so tokenHoldings() will see the new on-chain state,
// (2) refreshHoldings() re-invokes tokenHoldings() and feeds the swap card's account
// selectors. Both are needed: refreshBalances alone does not re-fetch the card's
// holdings snapshot. `logos` and refreshHoldings() are both in scope on swapPage.
async function refreshUiHoldings(app) {
  const pageId = await idByObjectName(app, "swapPage");
  await ignore(() => app.inspector.send("evaluate", {
    expression: "logos.module('amm_ui').refreshBalances()", objectId: pageId,
  }));
  await ignore(() => app.inspector.send("evaluate", {
    expression: "refreshHoldings()", objectId: pageId,
  }));
}

async function setSellAmount(app, amount) {
  const cardId = await idByObjectName(app, "swapCard");
  await app.inspector.send("setProperty", { objectId: cardId, property: "editingSide", value: "sell" });
  await app.inspector.send("setProperty", { objectId: cardId, property: "sellInput", value: String(amount) });
  await app.inspector.send("evaluate", { expression: "requestQuoteIn()", objectId: cardId });
}

async function cardState(app) {
  const id = await idByObjectName(app, "swapCard");
  const props = (await app.getProperties(id)).properties || [];
  const get = (n) => { const p = props.find((x) => x.name === n); return p ? p.value : undefined; };
  return {
    poolResolved: get("poolResolved"),
    poolExists: get("poolExists"),
    poolError: get("poolError"),
    swapError: get("swapError"),
    canSubmit: get("canSubmit"),
    submitButtonText: get("submitButtonText"),
    sellHolding: get("sellHolding"),
    buyHolding: get("buyHolding"),
  };
}

async function poolReserves(app) {
  const id = await idByObjectName(app, "swapCard");
  return { a: await prop(app, id, "poolReserveA"), b: await prop(app, id, "poolReserveB") };
}

async function saveShot(app, name) {
  const shot = await ignore(() => app.screenshot());
  if (shot && shot.image) {
    const path = new URL(`./${name}.png`, import.meta.url).pathname;
    await writeFile(path, Buffer.from(shot.image, "base64"));
    console.log(`    screenshot -> ${path}`);
  }
}

// --- spel transactions (out of band from the UI) ----------------------------

// Run a spel program instruction against the isolated test wallet. Returns the
// combined stdout+stderr. On a non-zero exit, throws UNLESS the output matches one
// of `tolerate` (a list of regexes for expected, benign failures on re-runs), in
// which case it returns the output with `.tolerated = <matched index>` attached.
function runSpel(args, { tolerate = [] } = {}) {
  console.log(`    $ spel ${args.join(" ")}`);
  const env = { ...process.env, LEE_WALLET_HOME_DIR: WALLET_HOME };
  try {
    return execFileSync("spel", args, {
      cwd: REPO_ROOT, env, input: `${WALLET_PASSWORD}\n`, encoding: "utf8",
    });
  } catch (err) {
    const out = `${err.stdout || ""}${err.stderr || ""}`;
    const idx = tolerate.findIndex((re) => re.test(out));
    if (idx >= 0) { const r = new String(out); r.tolerated = idx; return r; }
    throw new Error(`spel ${args[args.indexOf("--") + 1]} failed:\n${out}`);
  }
}

const confirmed = (out) =>
  /Transaction confirmed/i.test(out) && /included in a block/i.test(out);

// Is holder2's token A holding already an initialized TokenHolding on-chain? Read-only
// `spel inspect`: it exits non-zero when the account is absent/uninitialized (empty
// data can't decode as a TokenHolding). To avoid mistaking a default/empty decode for
// a real holding, also require the output to reference token A's definition id (in
// either encoding) — a decoded token A holding carries it.
function holdingInitialized(m) {
  let out;
  try {
    out = execFileSync(
      "spel",
      ["--idl", m.tokenIdl, "inspect", m.userHolding, "--type", "TokenHolding"],
      { cwd: REPO_ROOT, env: { ...process.env, LEE_WALLET_HOME_DIR: WALLET_HOME }, encoding: "utf8" },
    );
  } catch {
    return false; // non-zero exit -> not initialized (or unreachable; init will surface it)
  }
  return out.includes(m.tokenDefinition) || new RegExp(base58ToHex(m.tokenDefinition), "i").test(out);
}

// Create holder2's token A holding (a zeroized TokenHolding), unless it already exists.
// The faucet only mints into an EXISTING holding — it doesn't sign user_holding, so it
// cannot create a fresh one — so this must run first. `initialize_account` DOES sign
// account_to_initialize, so the fresh-account Claim::Authorized succeeds here. On a
// re-run we skip it (already initialized); the tolerate list still guards a race.
function initializeHolding(m) {
  if (holdingInitialized(m)) {
    console.log("    holder2 token A holding already initialized — skipping init");
    return "exists";
  }
  const out = runSpel(
    [
      "--idl", m.tokenIdl, "--program", m.tokenBin, "--", "initialize-account",
      "--definition-account", m.tokenDefinition,
      "--account-to-initialize", m.userHolding,
    ],
    { tolerate: [/Uninitialized accounts can be initialized/i, /already/i] },
  );
  if (out.tolerated !== undefined) {
    console.log("    holder2 token A holding already initialized (race) — continuing");
    return "exists";
  }
  console.log(confirmed(out) ? "    ✅ initialized holder2 token A holding" : "    initialize_account exited 0");
  return "created";
}

// Has the faucet already minted to holder2? The faucet claims a MintAllowance PDA on
// the FIRST mint and rewrites it thereafter, so the account existing == a mint has
// happened (and, within 24h, the cooldown is active). Read-only `spel inspect`: exits
// non-zero when the allowance is absent; when present, confirm it's holder2's record
// (it carries recipient_id) before treating it as "already minted".
function mintAlreadyDone(m) {
  let out;
  try {
    out = execFileSync(
      "spel",
      ["--idl", m.faucetIdl, "inspect", m.mintAllowance, "--type", "MintAllowance"],
      { cwd: REPO_ROOT, env: { ...process.env, LEE_WALLET_HOME_DIR: WALLET_HOME }, encoding: "utf8" },
    );
  } catch {
    return false; // no allowance account -> no prior mint
  }
  return out.includes(m.recipient) || new RegExp(base58ToHex(m.recipient), "i").test(out);
}

// Submit the FaucetMint into holder2's (now existing) token A holding, UNLESS a mint
// was already done (the allowance PDA exists → the 24h per-(recipient, token) cooldown
// is/was active). Returns:
//   "confirmed" — the mint landed on-chain (the normal, clean-run case),
//   "cooldown"  — skipped (or blocked) because holder2 was already funded by a prior run.
function faucetMint(m) {
  if (mintAlreadyDone(m)) {
    console.log("    faucet already minted to holder2 (allowance exists) — skipping mint (cooldown)");
    return "cooldown";
  }
  const out = runSpel(
    [
      "--idl", m.faucetIdl, "--program", m.faucetBin, "--", "faucet-mint",
      "--recipient", m.recipient,
      "--mint-allowance", m.mintAllowance,
      "--user-holding", m.userHolding,
      "--token-definition", m.tokenDefinition,
      "--mint-authority", m.mintAuthority,
      "--clock", m.clock,
    ],
    { tolerate: [/cooldown has not elapsed/i] },
  );
  if (out.tolerated !== undefined) {
    console.log("    faucet cooldown active — holder2 was funded by a previous run; continuing");
    return "cooldown";
  }
  console.log(confirmed(out) ? "    ✅ faucet mint confirmed on-chain" : "    faucet mint exited 0 (no marker)");
  return "confirmed";
}

// --- the test ---------------------------------------------------------------

test("amm faucet-swap: mint token A to a fresh account, refresh, then swap", async (app) => {
  const m = loadManifest();
  // The selector matches holdings by hex id (see base58ToHex); the manifest is base58.
  const userHoldingHex = base58ToHex(m.userHolding);
  console.log(`    recipient=${m.recipient}`);
  console.log(`    holding=${m.userHolding} (hex ${userHoldingHex}) token A def ${m.tokenDefinition}`);

  // 1. Wait for the swap card, make the Trade tab explicit.
  await app.waitFor(
    async () => { await app.expectTexts(["Sell", "Buy"]); },
    { timeout: 20000, interval: 500, description: "swap card to load" },
  );
  await ignore(() => app.click("Trade"));

  // 2. Sell = token A (index 0), Buy = token B (index 1) — the TOKENS_CONFIG order.
  await openPicker(app, "swapSellTokenButton");
  const sell = await pickToken(app, 0);
  await openPicker(app, "swapBuyTokenButton");
  const buy = await pickToken(app, 1);
  console.log(`    sell ${sell} -> buy ${buy}`);

  // 3. BEFORE the mint: is holder2's holding already selectable? On a clean run it is
  //    NOT (no token A yet) — which is exactly why a refresh is needed after minting.
  const before = await probeSelectable(app, "swapSellAccountSelector", userHoldingHex);
  console.log(`    holder2 holding selectable before mint: ${before}`);

  // 4. Create holder2's token A holding, then mint into it via the faucet — both
  //    out of band from the UI. The initialize step is required because the faucet
  //    only mints into an EXISTING holding (see initializeHolding()).
  initializeHolding(m);
  const mintResult = faucetMint(m);
  if (mintResult === "cooldown" && !before) {
    // Cooldown but the account was NOT funded — inconsistent; fail loudly.
    throw new Error("faucet reports cooldown but holder2 had no token A — check on-chain state");
  }

  // 5. Refresh the UI so the newly-minted holding shows up, then wait for it to
  //    become selectable. This is the crux: without the refresh the card keeps its
  //    stale pre-mint snapshot and holder2 never appears.
  await app.waitFor(
    async () => {
      await refreshUiHoldings(app);
      if (!(await probeSelectable(app, "swapSellAccountSelector", userHoldingHex)))
        throw new Error("holder2 holding not selectable yet");
    },
    { timeout: 40000, interval: 1500, description: "holder2 holding to appear after refresh" },
  );
  if (!before)
    console.log("    ✓ refresh surfaced holder2's newly-minted holding (absent before the mint)");

  // 6. Pin the SELL side to holder2's holding; pick the BUY account by balance.
  await selectHoldingById(app, "swapSellAccountSelector", userHoldingHex);
  await selectAccountByBalance(app, "swapBuyAccountSelector");
  const sellHolding = await prop(app, await idByObjectName(app, "swapCard"), "sellHolding");
  if (String(sellHolding) !== userHoldingHex)
    throw new Error(`sell holding is ${sellHolding}, expected holder2's ${userHoldingHex}`);
  console.log(`    swap will sell from holder2 holding ${sellHolding}`);

  // 7. Enter the sell amount and wait for the CTA to become a live "Swap".
  await setSellAmount(app, SELL_AMOUNT);
  await app.expectTexts([SELL_AMOUNT]);
  try {
    await app.waitFor(
      async () => { if (!(await cardState(app)).canSubmit) throw new Error("not submittable yet"); },
      { timeout: 15000, interval: 500, description: "pool resolve / CTA ready" },
    );
  } catch {
    await saveShot(app, "faucet-swap-cta-not-ready");
    throw new Error(`CTA never became submittable. Card state: ${JSON.stringify(await cardState(app))}`);
  }
  await saveShot(app, "faucet-swap-filled");

  const reservesBefore = await poolReserves(app);
  console.log(`    pool reserves before: A=${reservesBefore.a} B=${reservesBefore.b}`);

  // 8. Submit -> confirm dialog -> confirm (by objectId, with the same confirm()
  //    fallback swap.mjs uses for QtQuick Controls buttons).
  const submitId = await idByObjectName(app, "swapSubmitButton");
  await app.inspector.send("click", { objectId: submitId });
  const dialogId = await idByObjectName(app, "swapConfirmDialog");
  await app.waitFor(
    async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("dialog not open"); },
    { timeout: 8000, interval: 300, description: "confirm dialog open" },
  );
  const confirmId = await idByObjectName(app, "transactionConfirmButton");
  await app.inspector.send("click", { objectId: confirmId });
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) === true) throw new Error("still open"); },
      { timeout: 3000, interval: 300, description: "confirm click registered" },
    );
  } catch {
    console.log("    confirm button click didn't take — invoking confirm() via evaluate");
    await ignore(() => app.inspector.send("evaluate", { expression: "confirm()", objectId: dialogId }));
  }

  // 9. Verify the swap hit the chain: the card re-resolves the pool, so reserves move.
  const cardId = await idByObjectName(app, "swapCard");
  let after = reservesBefore;
  try {
    await app.waitFor(
      async () => {
        await ignore(() => app.inspector.send("evaluate", { expression: "doResolvePool()", objectId: cardId }));
        await new Promise((r) => setTimeout(r, 800));
        after = await poolReserves(app);
        if (after.a === reservesBefore.a && after.b === reservesBefore.b)
          throw new Error("reserves unchanged");
      },
      { timeout: 40000, interval: 1200, description: "pool reserves to change on-chain" },
    );
  } catch {
    await saveShot(app, "faucet-swap-result");
    const s = await cardState(app);
    const inProgress = await prop(app, cardId, "swapInProgress");
    throw new Error(
      `pool reserves did not change after the swap.\n` +
      `  before: A=${reservesBefore.a} B=${reservesBefore.b}\n` +
      `  after:  A=${after.a} B=${after.b}\n` +
      `  swapInProgress=${inProgress} swapError=${JSON.stringify(s.swapError)}`,
    );
  }
  console.log(`    pool reserves after:  A=${after.a} B=${after.b}  ✓ changed on-chain`);
  await saveShot(app, "faucet-swap-result");
});

run();

// How to run: bring up the isolated setup + app exactly as for swap.mjs (see
// apps/amm/README.md), ensure `spel` is on PATH and LEE_WALLET_HOME_DIR points at
// apps/amm/tests/testnet/.wallet, then: node apps/amm/tests/faucet-swap.mjs
