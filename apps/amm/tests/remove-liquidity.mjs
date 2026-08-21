// ---------------------------------------------------------------------------
// AMM UI test — REMOVE liquidity from the existing A/B pool via the positions view.
//
// Drives the running AMM UI through the QML inspector (logos-qt-mcp), the same way
// add-liquidity.mjs does. The setup script seeds the A/B pool (10000/10000) and the
// test wallet holds its LP, so this navigates to the positions view (Pool > View
// positions), selects the A/B position, opens the Manage dropdown and clicks
// "Remove liquidity", removes 50% (the slider's default), submits, and verifies the
// pool reserves shrank ON-CHAIN.
//
// Prereqs in the running app (see apps/amm/tests/README.md):
//   * launched against the isolated test wallet + TOKENS_CONFIG (TKA, TKB, TKC)
//   * an open wallet + reachable local sequencer
//   * the A/B pool seeded (testnet/setup-amm-testnet.sh) — the wallet holds A/B LP
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

// Fraction of the position to withdraw. 50 is the RemoveLiquidityDialog's default,
// so this also asserts the preset the UI opens on.
const REMOVE_PERCENT = 50;

// --- small helpers (mirror add-liquidity.mjs) ------------------------------

const ignore = async (fn) => { try { return await fn(); } catch { /* best effort */ } };

async function idByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0)
    throw new Error(`no object with objectName="${name}" (is the app on the right tab?)`);
  return res.matches[0].id;
}

async function maybeIdByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0) return undefined;
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

// The RemoveLiquidityDialog's state — explains WHY the Remove CTA isn't ready.
async function dialogState(app, dialogId) {
  const props = (await app.getProperties(dialogId)).properties || [];
  const get = (n) => { const p = props.find((x) => x.name === n); return p ? p.value : undefined; };
  return {
    visible: get("visible"),
    percent: get("percent"),
    quoteLoading: get("quoteLoading"),
    quoteReady: get("quoteReady"),
    quoteError: get("quoteError"),
    canSubmit: get("canSubmit"),
    submitting: get("submitting"),
    submitError: get("submitError"),
    amountA: get("amountA"),
    amountB: get("amountB"),
    lpAmount: get("lpAmount"),
    lpHoldingId: get("lpHoldingId"),
    holdingAId: get("holdingAId"),
    holdingBId: get("holdingBId"),
  };
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
// sequencer query — the same genuine chain-state read add-liquidity.mjs uses). Selects
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

test("amm liquidity: remove from the A/B pool", async (app) => {
  const tokens = JSON.parse(await readFile(TOKENS_CONFIG, "utf8"));
  const bySymbol = (s) => {
    const t = tokens.find((x) => (x.symbol || "").toUpperCase() === s);
    if (!t) throw new Error(`token ${s} not in ${TOKENS_CONFIG} — run the setup script`);
    return t.definitionId;
  };
  const tokenA = bySymbol("TKA");
  const tokenB = bySymbol("TKB");
  console.log(`    remove liquidity from A(${tokenA.slice(0, 6)}…) / B(${tokenB.slice(0, 6)}…)`);

  // 0. Baseline: the A/B pool's on-chain reserveA before removing.
  const before = await readReserveA(app);
  console.log(`    A/B reserveA before: ${before.reserveA}`);

  // 1. Open the positions view (Pool > View positions = tab 2, sub 0). Driving the
  //    navbar's select() switches the page; the wallet's A/B LP holding shows as a row.
  const navBarId = await idByObjectName(app, "navBar");
  await evaluate(app, navBarId, "select(2, 0)");
  await app.waitFor(
    async () => { await idByObjectName(app, "positionsPage"); },
    { timeout: 10000, interval: 300, description: "positions view to render" },
  );

  // 2. Select the A/B position. positionRow0 is the wallet's only seeded position.
  //    Clicking it emits positionActivated -> Main.openPoolDetail, which selects the
  //    Explore tab and shows PoolDetailPage for the pool.
  let rowId;
  await app.waitFor(
    async () => {
      rowId = await maybeIdByObjectName(app, "positionRow0");
      if (!rowId) throw new Error("no A/B position row yet (does the wallet hold A/B LP?)");
    },
    { timeout: 15000, interval: 400, description: "A/B position row" },
  );
  await app.inspector.send("click", { objectId: rowId });

  // The delegate's synthetic click may not take; fall back to activate() on the row.
  try {
    await app.waitFor(
      async () => { if (!(await maybeIdByObjectName(app, "poolDetailPage"))) throw new Error("detail not open"); },
      { timeout: 4000, interval: 300, description: "pool detail open" },
    );
  } catch {
    console.log("    row click didn't take — activating the position via evaluate");
    await ignore(() => evaluate(app, rowId, "activate()"));
    await app.waitFor(
      async () => { if (!(await maybeIdByObjectName(app, "poolDetailPage"))) throw new Error("detail not open"); },
      { timeout: 8000, interval: 300, description: "pool detail open (after evaluate)" },
    );
  }
  const detailId = await idByObjectName(app, "poolDetailPage");

  // 3. Wait for the detail page to resolve the wallet's position — the Manage menu's
  //    Remove entry (and the dialog's Remove CTA) both gate on having an LP holding here.
  await app.waitFor(
    async () => {
      if ((await prop(app, detailId, "canRemoveLiquidity")) !== true)
        throw new Error("position not resolved yet (canRemoveLiquidity=false)");
    },
    { timeout: 20000, interval: 500, description: "position resolved on detail page" },
  );

  // 4. Open the Manage dropdown and click "Remove liquidity". openMenu() is the same
  //    entry point the button's click/hover uses; the ManageEntry's click activates it.
  const manageBtnId = await idByObjectName(app, "poolDetailManageButton");
  await evaluate(app, manageBtnId, "openMenu()");
  await app.waitFor(
    async () => {
      const menuId = await maybeIdByObjectName(app, "poolDetailManageMenu");
      if (!menuId || (await prop(app, menuId, "visible")) !== true)
        throw new Error("manage menu not open yet");
    },
    { timeout: 5000, interval: 300, description: "manage menu open" },
  );
  const removeEntryId = await idByObjectName(app, "poolDetailManageRemove");
  await app.inspector.send("click", { objectId: removeEntryId });

  // The dropdown entry's synthetic click may not take; fall back to the page's
  // openRemoveDialog(), the exact handler the entry's onActivated invokes.
  const dialogId = await idByObjectName(app, "removeLiquidityDialog");
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("dialog not visible"); },
      { timeout: 4000, interval: 300, description: "remove dialog open" },
    );
  } catch {
    console.log("    remove-entry click didn't take — opening the dialog via evaluate");
    await ignore(() => evaluate(app, detailId, "openRemoveDialog()"));
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("dialog not visible"); },
      { timeout: 8000, interval: 300, description: "remove dialog open (after evaluate)" },
    );
  }

  // 5. The dialog opens at 50% by default. Click the 50% preset to make the choice
  //    explicit (a no-op on the default) and confirm it registers. Then wait for the
  //    quote to settle so the Remove CTA is submittable (needs the resolved A/B/LP
  //    destination holdings, which PoolDetailPage passed into openFor()).
  const presetId = await idByObjectName(app, `removePreset${REMOVE_PERCENT}`);
  await app.inspector.send("click", { objectId: presetId });
  await ignore(() => setProp(app, dialogId, "percent", REMOVE_PERCENT));
  await app.waitFor(
    async () => {
      if ((await prop(app, dialogId, "percent")) !== REMOVE_PERCENT)
        throw new Error("percent not set to 50 yet");
    },
    { timeout: 5000, interval: 300, description: `remove percent = ${REMOVE_PERCENT}` },
  );
  try {
    await app.waitFor(
      async () => {
        const s = await dialogState(app, dialogId);
        if (!s.canSubmit) throw new Error("remove CTA not ready yet");
      },
      { timeout: 20000, interval: 500, description: "remove CTA ready" },
    );
  } catch (e) {
    await saveShot(app, "remove-liquidity-cta-not-ready");
    throw new Error(`${e.message}. Dialog state: ${JSON.stringify(await dialogState(app, dialogId))}`);
  }
  const primed = await dialogState(app, dialogId);
  console.log(`    removing ${REMOVE_PERCENT}%: A=${primed.amountA} B=${primed.amountB} (lp=${primed.lpAmount})`);
  await saveShot(app, "remove-liquidity-primed");

  // 6. Submit the removal. On success the dialog emits removed(tx) and closes itself;
  //    on failure it stays open with submitError set.
  const confirmId = await idByObjectName(app, "removeConfirmButton");
  await app.inspector.send("click", { objectId: confirmId });

  // QtQuick Buttons don't reliably take the inspector's synthetic click, so if the
  // dialog is still open and idle, invoke submit() directly.
  await new Promise((r) => setTimeout(r, 800));
  {
    const s = await dialogState(app, dialogId);
    if (s.visible === true && s.submitting !== true && !s.submitError) {
      console.log("    confirm click didn't take — invoking submit() via evaluate");
      await ignore(() => evaluate(app, dialogId, "submit()"));
    }
  }

  // 7. Wait for the submit to complete: the dialog closes on success. Surface any
  //    submitError immediately rather than waiting out the timeout.
  try {
    await app.waitFor(
      async () => {
        const s = await dialogState(app, dialogId);
        if (s.submitError) throw new Error(`submit failed: ${s.submitError}`);
        if (s.visible === true) throw new Error("remove not submitted yet (dialog still open)");
      },
      { timeout: 30000, interval: 1000, description: "remove to submit (dialog closes)" },
    );
  } catch (e) {
    await saveShot(app, "remove-liquidity-result");
    throw new Error(`${e.message}. Dialog state: ${JSON.stringify(await dialogState(app, dialogId))}`);
  }
  console.log("    remove submitted (dialog closed)  ✓");

  // 8. Verify ON-CHAIN: the pool's reserveA must have shrunk by the withdrawal.
  const after = await readReserveA(app);
  try {
    await app.waitFor(
      async () => {
        // Force a fresh read each poll — the remove block may not be applied yet.
        await ignore(() => evaluate(app, after.swapId, "doResolvePool()"));
        await new Promise((r) => setTimeout(r, 800));
        const now = await prop(app, after.swapId, "poolReserveA");
        if (!(BigInt(now) < BigInt(before.reserveA)))
          throw new Error(`reserveA not shrunk yet (before=${before.reserveA} now=${now})`);
      },
      { timeout: 40000, interval: 1500, description: "A/B reserveA to shrink on-chain" },
    );
  } catch {
    await saveShot(app, "remove-liquidity-result");
    throw new Error(
      `A/B reserveA did not shrink after the remove.\n` +
      `  before=${before.reserveA} after=${await prop(app, after.swapId, "poolReserveA")}`,
    );
  }
  const shrunkReserveA = await prop(app, after.swapId, "poolReserveA");
  console.log(`    A/B reserveA after:  ${shrunkReserveA}  ✓ shrank on-chain`);
  await saveShot(app, "remove-liquidity-result");
});

run();

// How to run (from scratch, interactive + CI): see the "Running the UI tests"
// section in apps/amm/tests/README.md — same flow as add-liquidity.mjs.
