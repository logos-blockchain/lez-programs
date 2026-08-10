// ---------------------------------------------------------------------------
// AMM UI test — create a NEW pool (A/C) through the Liquidity view.
//
// Drives the running AMM UI through the QML inspector (logos-qt-mcp), the same
// way swap.mjs does. It selects the A/C pair (which the setup script leaves
// UNSEEDED — only A/B is created), lets the form auto-fill the minimum opening
// deposit, submits the create, and verifies the pool now exists ON-CHAIN.
//
// Prereqs in the running app (see apps/amm/tests/README.md):
//   * launched against the isolated test wallet + TOKENS_CONFIG that
//     testnet/setup-amm-testnet.sh writes (TKA, TKB, TKC)
//   * an open wallet + reachable local sequencer
//   * the A/C pool must NOT exist yet (setup only seeds A/B)
// ---------------------------------------------------------------------------

import { resolve } from "node:path";
import { readFile, writeFile } from "node:fs/promises";

const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

// The token config the app was launched with (same file the setup script writes).
const TOKENS_CONFIG =
  process.env.TOKENS_CONFIG ||
  new URL("./testnet/amm-tokens.json", import.meta.url).pathname;

// --- small helpers (mirrors swap.mjs) --------------------------------------

const ignore = async (fn) => { try { return await fn(); } catch { /* best effort */ } };

async function idByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0)
    throw new Error(`no object with objectName="${name}" (is the app on the Liquidity tab?)`);
  return res.matches[0].id;
}

async function prop(app, id, name) {
  const props = (await app.getProperties(id)).properties || [];
  const p = props.find((x) => x.name === name);
  return p ? p.value : undefined;
}

async function setProp(app, id, property, value) {
  await app.inspector.send("setProperty", { objectId: id, property, value });
}

async function evaluate(app, id, expression) {
  await app.inspector.send("evaluate", { expression, objectId: id });
}

// The NewPositionForm's create/pool state — explains WHY the CTA isn't ready.
async function formState(app, formId) {
  const props = (await app.getProperties(formId)).properties || [];
  const get = (n) => { const p = props.find((x) => x.name === n); return p ? p.value : undefined; };
  return {
    activePool: get("activePool"),
    missingPool: get("missingPool"),
    canConfirm: get("canConfirm"),
    amountA: get("amountA"),
    amountB: get("amountB"),
    submitError: get("submitError"),
    transactionId: get("transactionId"),
    selectedHoldingAId: get("selectedHoldingAId"),
    selectedHoldingBId: get("selectedHoldingBId"),
  };
}

// Pick the funding account for a create-pool side. The selector auto-selects a
// single holding, but choose it explicitly (robust to multi-account wallets):
// wait for the holdings to populate, then select the first match. canConfirm now
// requires both A/B holdings before a pool can be created.
async function selectAccount(app, selectorObjectName) {
  // The selector lives in a Loader that instantiates only once the pool is known
  // to be missing, so it may render a frame after missingPool flips — wait for it.
  let id;
  await app.waitFor(
    async () => { id = await idByObjectName(app, selectorObjectName); },
    { timeout: 10000, interval: 300, description: `${selectorObjectName} to render` },
  );
  await app.waitFor(
    async () => { if ((await prop(app, id, "hasFunds")) !== true) throw new Error("no matching holdings yet"); },
    { timeout: 10000, interval: 300, description: `${selectorObjectName} holdings to load` },
  );
  await evaluate(app, id, "setSelection(accountIdFor(matchingAccounts[0]), false)");
  await app.waitFor(
    async () => { if (!(await prop(app, id, "selectedAccountId"))) throw new Error("holding not selected yet"); },
    { timeout: 5000, interval: 200, description: `${selectorObjectName} holding selected` },
  );
}

async function saveShot(app, name) {
  const shot = await ignore(() => app.screenshot());
  if (shot && shot.image) {
    const path = new URL(`./${name}.png`, import.meta.url).pathname;
    await writeFile(path, Buffer.from(shot.image, "base64"));
    console.log(`    screenshot -> ${path}`);
  }
}

// --- Swap-card token picker helpers (shared shape with swap.mjs), used only to
//     read the A/C pool's on-chain state for the final assertion. ------------

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

async function pickToken(app, index) {
  const res = await app.findByProperty("objectName", "tokenListItem");
  const items = (res && res.matches) || [];
  if (items.length <= index)
    throw new Error(`token #${index + 1} not found — only ${items.length} in the list`);
  await app.inspector.send("click", { objectId: items[index].id });
  await app.waitFor(
    async () => { if (await pickerOpen(app)) throw new Error("picker still open"); },
    { timeout: 5000, interval: 200, description: `select token #${index + 1}` },
  );
}

// --- the test ---------------------------------------------------------------

test("amm liquidity: create the A/C pool", async (app) => {
  // Resolve the A and C token-definition ids from the launched token config.
  const tokens = JSON.parse(await readFile(TOKENS_CONFIG, "utf8"));
  const bySymbol = (s) => {
    const t = tokens.find((x) => (x.symbol || "").toUpperCase() === s);
    if (!t) throw new Error(`token ${s} not in ${TOKENS_CONFIG} — run the setup script`);
    return t.definitionId;
  };
  const tokenA = bySymbol("TKA");
  const tokenC = bySymbol("TKC");
  console.log(`    create pool A(${tokenA.slice(0, 6)}…) / C(${tokenC.slice(0, 6)}…)`);

  // 1. Switch to the Liquidity tab and wait for the form to render.
  await app.waitFor(
    async () => { await app.expectTexts(["Trade", "Liquidity"]); },
    { timeout: 20000, interval: 500, description: "nav bar to load" },
  );
  await ignore(() => app.click("Liquidity"));
  // waitFor resolves when the condition stops throwing — it does NOT return the
  // callback's value, so fetch the id with a direct call afterwards.
  await app.waitFor(
    async () => { await idByObjectName(app, "newPositionForm"); },
    { timeout: 10000, interval: 300, description: "liquidity form to render" },
  );
  const formId = await idByObjectName(app, "newPositionForm");

  // 2. Select the A/C pair through selectToken() — the same entry point the token picker
  //    uses — so the form runs its normal selection logic and resetPairDraft(), clearing any
  //    persisted amounts/price/minimums from a reused app session and firing a fresh quote.
  //    Setting selectedToken*Id directly would bypass that reset and could assert against
  //    stale draft state. Both tokens are already in the config, so no async token resolution
  //    is needed. The first missing-pool quote returns the minimum opening deposit, which
  //    applyQuoteSideEffects auto-fills — so canConfirm becomes ready.
  await evaluate(app, formId, `selectToken("A", "${tokenA}")`);
  await evaluate(app, formId, `selectToken("B", "${tokenC}")`);

  // Sanity: the selection must stick — the ids have to be present in the token
  // config the app was launched with, or tokenById can't resolve them.
  await app.waitFor(
    async () => {
      const a = await prop(app, formId, "selectedTokenAId");
      const b = await prop(app, formId, "selectedTokenBId");
      if (!a || !b) throw new Error(`pair not selected (A=${a} B=${b})`);
    },
    { timeout: 5000, interval: 300, description: "A/C pair selected" },
  );
  await evaluate(app, formId, "requestQuote(true)");

  // 3. Wait for the missing-pool quote (which makes the per-side account selectors
  //    render), pick the funding account for each side, then wait for a submittable
  //    create quote — canConfirm needs the funded minimum deposit AND both holdings.
  try {
    await app.waitFor(
      async () => {
        const s = await formState(app, formId);
        if (s.activePool === true)
          throw new Error("A/C pool already exists — reset the testnet (only A/B should be seeded)");
        if (!s.missingPool) throw new Error("pool status not resolved yet");
      },
      { timeout: 20000, interval: 500, description: "missing-pool quote" },
    );
    await selectAccount(app, "newPositionAccountSelectorA");
    await selectAccount(app, "newPositionAccountSelectorB");
    // The live app window persists across runs, so the form may carry leftover deposit
    // amounts and a stale "Position submitted" transactionId from a prior create — both
    // block a clean run (mismatched amounts keep canConfirm false; a stale txId would make
    // step 5's "submitted" wait pass instantly). resetPairDraft() clears the amounts/price
    // and re-quotes (price-only), so applyQuoteSideEffects re-fills the fresh minimum
    // deposit, and its draftChanged() clears the stale transactionId. Holdings are kept.
    await evaluate(app, formId, "resetPairDraft()");
    await app.waitFor(
      async () => {
        const s = await formState(app, formId);
        if (!s.canConfirm) throw new Error("create CTA not ready yet");
      },
      { timeout: 20000, interval: 500, description: "create CTA ready" },
    );
  } catch (e) {
    await saveShot(app, "create-pool-cta-not-ready");
    throw new Error(`${e.message}. Form state: ${JSON.stringify(await formState(app, formId))}`);
  }
  await saveShot(app, "create-pool-filled");
  console.log(`    minimum deposit: A=${(await formState(app, formId)).amountA} C=${(await formState(app, formId)).amountB}`);

  // 4. Submit -> confirmation dialog -> confirm.
  const dialogId = await idByObjectName(app, "liquidityConfirmDialog");
  const submitId = await idByObjectName(app, "newPositionSubmitButton");
  await app.inspector.send("click", { objectId: submitId });

  // QtQuick Controls Buttons don't reliably take the inspector's synthetic click,
  // so if the dialog didn't open, emit the form's confirmationRequested signal
  // directly (exactly what the button's onClicked does).
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("not open"); },
      { timeout: 4000, interval: 300, description: "confirm dialog open" },
    );
  } catch {
    console.log("    submit click didn't take — emitting confirmationRequested via evaluate");
    await ignore(() => evaluate(app, formId, "confirmationRequested(submissionSnapshot())"));
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("dialog not open"); },
      { timeout: 8000, interval: 300, description: "confirm dialog open (after evaluate)" },
    );
  }

  const confirmId = await idByObjectName(app, "transactionConfirmButton");
  await app.inspector.send("click", { objectId: confirmId });
  // QtQuick Buttons don't always take the synthetic click; fall back to invoking
  // the dialog's confirm() slot directly (fires onConfirmed -> flow.confirm()).
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) === true) throw new Error("still open"); },
      { timeout: 3000, interval: 300, description: "confirm click registered" },
    );
  } catch {
    console.log("    confirm button click didn't take — invoking confirm() via evaluate");
    await ignore(() => evaluate(app, dialogId, "confirm()"));
  }

  // 5. Wait for the create to submit. It's fully async — createAccountPublic (mint
  //    the LP holding) then createPool then the tx submit — so transactionId lands
  //    a few seconds after confirm(), not immediately.
  try {
    await app.waitFor(
      async () => {
        const s = await formState(app, formId);
        if (!s.transactionId) throw new Error("create not submitted yet");
      },
      { timeout: 30000, interval: 1000, description: "create to submit (transactionId set)" },
    );
  } catch {
    await saveShot(app, "create-pool-result");
    throw new Error(`create did not submit. Form state: ${JSON.stringify(await formState(app, formId))}`);
  }
  const final = await formState(app, formId);
  console.log(`    create submitted: tx ${final.transactionId}`);

  // 6. Verify ON-CHAIN via the Swap card's resolvePool — a plain hex pool read
  //    against the sequencer (createPool has no tx poll / poolStatus, so we don't
  //    lean on the liquidity form's legacy quote). Selecting A/C on the Trade tab
  //    must now report the pool as existing with reserves.
  await ignore(() => app.click("Trade"));
  await app.waitFor(
    async () => { await app.expectTexts(["Sell", "Buy"]); },
    { timeout: 10000, interval: 500, description: "swap card to load" },
  );
  await openPicker(app, "swapSellTokenButton");
  await pickToken(app, 0); // TKA
  await openPicker(app, "swapBuyTokenButton");
  await pickToken(app, 2); // TKC
  const swapId = await idByObjectName(app, "swapCard");

  try {
    await app.waitFor(
      async () => {
        // Force a fresh read each poll — the create block may not be applied yet.
        await ignore(() => evaluate(app, swapId, "doResolvePool()"));
        await new Promise((r) => setTimeout(r, 800));
        if ((await prop(app, swapId, "poolExists")) !== true)
          throw new Error("A/C pool not found yet");
      },
      { timeout: 40000, interval: 1500, description: "A/C pool to exist on-chain" },
    );
  } catch {
    await saveShot(app, "create-pool-result");
    throw new Error(
      `A/C pool not found on-chain after the create (tx ${final.transactionId}).\n` +
      `  swap card: poolExists=${await prop(app, swapId, "poolExists")} ` +
      `reserveA=${await prop(app, swapId, "poolReserveA")} ` +
      `reserveB=${await prop(app, swapId, "poolReserveB")}`,
    );
  }
  const rA = await prop(app, swapId, "poolReserveA");
  const rB = await prop(app, swapId, "poolReserveB");
  console.log(`    A/C pool exists on-chain  ✓  reserves A=${rA} B=${rB}  (tx ${final.transactionId})`);
  await saveShot(app, "create-pool-result");
});

run();

// How to run (from scratch, interactive + CI): see the "Running the UI tests"
// section in apps/amm/tests/README.md — same flow as swap.mjs.
