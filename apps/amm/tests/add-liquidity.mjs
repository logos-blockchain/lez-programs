// ---------------------------------------------------------------------------
// AMM UI test — ADD liquidity to the existing A/B pool through the Liquidity view.
//
// Drives the running AMM UI through the QML inspector (logos-qt-mcp), the same
// way create-pool.mjs does. The setup script seeds the A/B pool (10000/10000), so
// this selects A/B (an ACTIVE pool), asserts the CTA stays DISABLED until deposit
// amounts are entered, enters a deposit (token B ratio-fills), submits the add,
// and verifies the pool reserves grew ON-CHAIN.
//
// Prereqs in the running app (see apps/amm/tests/README.md):
//   * launched against the isolated test wallet + TOKENS_CONFIG (TKA, TKB, TKC)
//   * an open wallet + reachable local sequencer
//   * the A/B pool seeded (testnet/setup-amm-testnet.sh)
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

// Deposit added for token A (raw base units — the form treats amounts as raw). The
// seeded A/B pool is 10000/10000, so 1000 mints a nonzero LP and is trivially funded.
const DEPOSIT_A = "1000";

// --- small helpers (mirror create-pool.mjs) --------------------------------

const ignore = async (fn) => { try { return await fn(); } catch { /* best effort */ } };

async function idByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0)
    throw new Error(`no object with objectName="${name}" (is the app on the right tab?)`);
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

// The NewPositionForm's add/pool state — explains WHY the CTA isn't ready.
async function formState(app, formId) {
  const props = (await app.getProperties(formId)).properties || [];
  const get = (n) => { const p = props.find((x) => x.name === n); return p ? p.value : undefined; };
  return {
    activePool: get("activePool"),
    quoteStale: get("quoteStale"),
    quoteLoading: get("quoteLoading"),
    canConfirm: get("canConfirm"),
    amountA: get("amountA"),
    amountB: get("amountB"),
    submitError: get("submitError"),
    transactionId: get("transactionId"),
    selectedHoldingAId: get("selectedHoldingAId"),
    selectedHoldingBId: get("selectedHoldingBId"),
  };
}

// Pick the funding account for an add-liquidity side (identical to create-pool.mjs).
async function selectAccount(app, selectorObjectName) {
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

// --- swap-card helpers: read the A/B pool's on-chain reserves (shared with swap.mjs) ---

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

// Read the A/B pool's canonical reserveA via the swap card's resolvePool (a live
// sequencer query — the same genuine chain-state read create-pool.mjs uses). Selects
// A(0)/B(1) on the Trade tab and forces a fresh resolve. Returns { swapId, reserveA }.
async function readReserveA(app) {
  await ignore(() => app.click("Trade"));
  await app.waitFor(
    async () => { await app.expectTexts(["Sell", "Buy"]); },
    { timeout: 10000, interval: 500, description: "swap card to load" },
  );
  await openPicker(app, "swapSellTokenButton");
  await pickToken(app, 0); // TKA
  await openPicker(app, "swapBuyTokenButton");
  await pickToken(app, 1); // TKB
  const swapId = await idByObjectName(app, "swapCard");
  let reserveA;
  await app.waitFor(
    async () => {
      await ignore(() => evaluate(app, swapId, "doResolvePool()"));
      await new Promise((r) => setTimeout(r, 800));
      if ((await prop(app, swapId, "poolExists")) !== true)
        throw new Error("A/B pool not resolved yet");
      reserveA = await prop(app, swapId, "poolReserveA");
    },
    { timeout: 20000, interval: 1000, description: "A/B pool reserves" },
  );
  return { swapId, reserveA };
}

// --- the test ---------------------------------------------------------------

test("amm liquidity: add to the A/B pool", async (app) => {
  const tokens = JSON.parse(await readFile(TOKENS_CONFIG, "utf8"));
  const bySymbol = (s) => {
    const t = tokens.find((x) => (x.symbol || "").toUpperCase() === s);
    if (!t) throw new Error(`token ${s} not in ${TOKENS_CONFIG} — run the setup script`);
    return t.definitionId;
  };
  const tokenA = bySymbol("TKA");
  const tokenB = bySymbol("TKB");
  console.log(`    add liquidity to A(${tokenA.slice(0, 6)}…) / B(${tokenB.slice(0, 6)}…)`);

  // 0. Baseline: the A/B pool's on-chain reserveA before adding.
  const before = await readReserveA(app);
  console.log(`    A/B reserveA before: ${before.reserveA}`);

  // 1. Open the create-pool view (Pool > Create pool = tab 2, sub 1). Driving the
  //    navbar's select() fires tabChanged, which also resets the form. (The old
  //    "Liquidity" tab is now an entry under the "Pool" dropdown.)
  const navBarId = await idByObjectName(app, "navBar");
  await evaluate(app, navBarId, "select(2, 1)");
  await app.waitFor(
    async () => { await idByObjectName(app, "newPositionForm"); },
    { timeout: 10000, interval: 300, description: "liquidity form to render" },
  );
  const formId = await idByObjectName(app, "newPositionForm");

  // 2. Select the A/B pair (existing pool) and kick a quote.
  await setProp(app, formId, "selectedTokenAId", tokenA);
  await setProp(app, formId, "selectedTokenBId", tokenB);
  await app.waitFor(
    async () => {
      const a = await prop(app, formId, "selectedTokenAId");
      const b = await prop(app, formId, "selectedTokenBId");
      if (!a || !b) throw new Error(`pair not selected (A=${a} B=${b})`);
    },
    { timeout: 5000, interval: 300, description: "A/B pair selected" },
  );
  await evaluate(app, formId, "requestQuote(true)");

  // 3. Wait for the active-pool quote (the pool exists).
  await app.waitFor(
    async () => {
      const s = await formState(app, formId);
      if (s.activePool !== true)
        throw new Error(`pool not active yet (activePool=${s.activePool})`);
    },
    { timeout: 20000, interval: 500, description: "active-pool quote" },
  );

  // 4. Pick the funding account for each side (add mode shows the selectors).
  await selectAccount(app, "newPositionAccountSelectorA");
  await selectAccount(app, "newPositionAccountSelectorB");

  // 5. The CTA must be DISABLED when no deposit amounts are entered — even though the pool is
  //    active. canConfirm gates on the entered amounts (+ holdings), not on any quote-side
  //    flag. resetPairDraft() clears the amount fields (the live app window persists across
  //    runs, so they may carry leftover amounts) and, because the pair is treated as changed,
  //    re-resolves the pool. Wait for that active-pool quote to FULLY settle (activePool back
  //    to true and the quote no longer stale/loading) so the reserves are reloaded before the
  //    step-6 ratio-fill needs them. At that point amounts are still empty → canConfirm false.
  await evaluate(app, formId, "resetPairDraft()");
  await app.waitFor(
    async () => {
      const s = await formState(app, formId);
      if (s.activePool !== true || s.quoteStale === true || s.quoteLoading === true)
        throw new Error(`reset quote not settled (activePool=${s.activePool} `
          + `stale=${s.quoteStale} loading=${s.quoteLoading})`);
      if (s.amountA || s.amountB)
        throw new Error(`deposit amounts not cleared (A=${s.amountA} B=${s.amountB})`);
      if (s.canConfirm)
        throw new Error("Add-liquidity CTA is enabled with no deposit amounts entered");
    },
    { timeout: 20000, interval: 500, description: "CTA disabled, pool re-resolved" },
  );
  console.log("    CTA correctly disabled with no amounts entered  ✓");

  // 6. Enter a deposit for token A; token B ratio-fills. Then wait for a submittable quote.
  await evaluate(app, formId, `finishActiveAmount("A", "${DEPOSIT_A}")`);
  try {
    await app.waitFor(
      async () => {
        const s = await formState(app, formId);
        if (!s.canConfirm) throw new Error("add CTA not ready yet");
      },
      { timeout: 20000, interval: 500, description: "add CTA ready" },
    );
  } catch (e) {
    await saveShot(app, "add-liquidity-cta-not-ready");
    throw new Error(`${e.message}. Form state: ${JSON.stringify(await formState(app, formId))}`);
  }
  const filled = await formState(app, formId);
  console.log(`    deposit: A=${filled.amountA} B=${filled.amountB}`);
  await saveShot(app, "add-liquidity-filled");

  // 7. Submit -> confirmation dialog -> confirm.
  const dialogId = await idByObjectName(app, "liquidityConfirmDialog");
  const submitId = await idByObjectName(app, "newPositionSubmitButton");
  await app.inspector.send("click", { objectId: submitId });

  // QtQuick Buttons don't reliably take the inspector's synthetic click, so if the
  // dialog didn't open, emit the form's confirmationRequested directly.
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
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) === true) throw new Error("still open"); },
      { timeout: 3000, interval: 300, description: "confirm click registered" },
    );
  } catch {
    console.log("    confirm button click didn't take — invoking confirm() via evaluate");
    await ignore(() => evaluate(app, dialogId, "confirm()"));
  }

  // 8. Wait for the add to submit. Fully async — createAccountPublic (mint the fresh LP
  //    holding) then addLiquidity then the tx submit — so transactionId lands a few
  //    seconds after confirm().
  try {
    await app.waitFor(
      async () => {
        const s = await formState(app, formId);
        if (!s.transactionId) throw new Error("add not submitted yet");
      },
      { timeout: 30000, interval: 1000, description: "add to submit (transactionId set)" },
    );
  } catch {
    await saveShot(app, "add-liquidity-result");
    throw new Error(`add did not submit. Form state: ${JSON.stringify(await formState(app, formId))}`);
  }
  const final = await formState(app, formId);
  console.log(`    add submitted: tx ${final.transactionId}`);

  // 9. Verify ON-CHAIN: the pool's reserveA must have grown by the deposit.
  const after = await readReserveA(app);
  try {
    await app.waitFor(
      async () => {
        // Force a fresh read each poll — the add block may not be applied yet.
        await ignore(() => evaluate(app, after.swapId, "doResolvePool()"));
        await new Promise((r) => setTimeout(r, 800));
        const now = await prop(app, after.swapId, "poolReserveA");
        if (!(BigInt(now) > BigInt(before.reserveA)))
          throw new Error(`reserveA not grown yet (before=${before.reserveA} now=${now})`);
      },
      { timeout: 40000, interval: 1500, description: "A/B reserveA to grow on-chain" },
    );
  } catch {
    await saveShot(app, "add-liquidity-result");
    throw new Error(
      `A/B reserveA did not grow after the add (tx ${final.transactionId}).\n` +
      `  before=${before.reserveA} after=${await prop(app, after.swapId, "poolReserveA")}`,
    );
  }
  const grownReserveA = await prop(app, after.swapId, "poolReserveA");
  console.log(`    A/B reserveA after:  ${grownReserveA}  ✓ grew on-chain  (tx ${final.transactionId})`);
  await saveShot(app, "add-liquidity-result");
});

run();

// How to run (from scratch, interactive + CI): see the "Running the UI tests"
// section in apps/amm/tests/README.md — same flow as create-pool.mjs.
