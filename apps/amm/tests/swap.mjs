// ---------------------------------------------------------------------------
// AMM UI test — swap the first token in the list for the second (SELL_AMOUNT).
//
// Drives the running AMM UI through the QML inspector (logos-qt-mcp). Run it
// against a LIVE app window so you can watch it happen — see "Running the UI
// tests" in apps/amm/README.md for how to run it from scratch.
//
// Requires, in the running app:
//   * the wallet backend ready with a token list (TOKENS_CONFIG) of >= 2 tokens
//   * an existing pool WITH liquidity for token[0]/token[1] — otherwise the CTA
//     stays "No pool / no liquidity" and the swap can't be submitted
//   * (to actually land on-chain) an open wallet + reachable sequencer
// ---------------------------------------------------------------------------

import { resolve } from "node:path";
import { writeFile } from "node:fs/promises";

// Locate the test framework. This defaults to ./result-mcp next to this test
// (apps/amm/result-mcp), which is where the README's build step puts it:
// from the repo root run `nix build .#test-framework -o apps/amm/result-mcp`.
// Override with LOGOS_QT_MCP=/abs/path/to/result-mcp if it lives elsewhere.
const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

const SELL_AMOUNT = "100";

// --- small helpers over the raw inspector commands -------------------------

const ignore = async (fn) => { try { return await fn(); } catch { /* best effort */ } };

// Resolve a single object's inspector id by its QML objectName.
async function idByObjectName(app, name) {
  const res = await app.findByProperty("objectName", name);
  if (res.error || !res.matches || res.matches.length === 0)
    throw new Error(`no object with objectName="${name}" (is the app on the Trade tab?)`);
  return res.matches[0].id;
}

// Read a single property off an object id.
async function prop(app, id, name) {
  const props = (await app.getProperties(id)).properties || [];
  const p = props.find((x) => x.name === name);
  return p ? p.value : undefined;
}

// Is the token picker actually visible right now?
async function pickerOpen(app) {
  const id = await idByObjectName(app, "tokenSelectorModal");
  return (await prop(app, id, "visible")) === true;
}

// Open the sell/buy picker by clicking its token button (by objectId — center
// click, no fuzzy text), then wait until the modal is genuinely visible.
async function openPicker(app, buttonObjectName) {
  const btnId = await idByObjectName(app, buttonObjectName);
  await app.inspector.send("click", { objectId: btnId });
  await app.waitFor(
    async () => { if (!(await pickerOpen(app))) throw new Error("picker not open"); },
    { timeout: 5000, interval: 200, description: `open ${buttonObjectName}` },
  );
}

// The token list delegates, in list order (objectName "tokenListItem").
async function tokenItems(app) {
  const res = await app.findByProperty("objectName", "tokenListItem");
  return (res && res.matches) || [];
}

// Click the Nth token in the (open) picker by objectId; returns its symbol.
async function pickToken(app, index) {
  const items = await tokenItems(app);
  if (items.length <= index)
    throw new Error(`token #${index + 1} not found — only ${items.length} token(s) in the list`);
  const id = items[index].id;
  const symbol = await prop(app, id, "tokenSymbol");
  await app.inspector.send("click", { objectId: id });
  // Selecting a token closes the picker.
  await app.waitFor(
    async () => { if (await pickerOpen(app)) throw new Error("picker still open"); },
    { timeout: 5000, interval: 200, description: `select token #${index + 1}` },
  );
  return symbol;
}

// Enter the sell amount by setting the SwapCard's state directly. Synthesizing
// keystrokes needs the TextInput to hold active focus, which the inspector
// can't reliably grant headlessly; setting sellInput updates the property (and
// the bound TextInput display) so the reactive flow can run.
//
// Setting a property programmatically re-evaluates dependent BINDINGS but does
// not fire the onSellInputChanged HANDLER the way real typing does — and the
// Sell preview is now driven by that handler (onSellInputChanged ->
// requestQuoteIn -> async backend.swapExactInQuote), not a synchronous binding.
// So kick the quote explicitly, mirroring the doResolvePool() nudge used in the
// reserve-change check below. Without this the CTA never leaves "Amount too
// small" because the server quote never fires.
async function setSellAmount(app, amount) {
  const cardId = await idByObjectName(app, "swapCard");
  await app.inspector.send("setProperty", { objectId: cardId, property: "editingSide", value: "sell" });
  await app.inspector.send("setProperty", { objectId: cardId, property: "sellInput", value: String(amount) });
  await app.inspector.send("evaluate", { expression: "requestQuoteIn()", objectId: cardId });
}

// Read the SwapCard's swap/pool state — explains WHY the CTA isn't "Swap" yet.
async function cardState(app) {
  const id = await idByObjectName(app, "swapCard");
  const props = (await app.getProperties(id)).properties || [];
  const get = (n) => { const p = props.find((x) => x.name === n); return p ? p.value : undefined; };
  return {
    editingSide: get("editingSide"),
    sellInput: get("sellInput"),
    poolLoading: get("poolLoading"),
    poolResolved: get("poolResolved"),
    poolExists: get("poolExists"),
    poolError: get("poolError"),
    swapError: get("swapError"),
    canSubmit: get("canSubmit"),
    submitButtonText: get("submitButtonText"),
  };
}

// Read the pool's on-chain reserves as the app sees them. These come from
// AmmUiBackend.resolvePool() — a live query against the sequencer — so
// comparing them before/after a swap is a genuine chain-state assertion.
async function poolReserves(app) {
  const id = await idByObjectName(app, "swapCard");
  return {
    a: await prop(app, id, "poolReserveA"),
    b: await prop(app, id, "poolReserveB"),
  };
}

// Save a screenshot PNG next to this test file so failures are inspectable.
async function saveShot(app, name) {
  const shot = await ignore(() => app.screenshot());
  if (shot && shot.image) {
    const path = new URL(`./${name}.png`, import.meta.url).pathname;
    await writeFile(path, Buffer.from(shot.image, "base64"));
    console.log(`    screenshot -> ${path}`);
  }
}

// --- the test ---------------------------------------------------------------

test("amm swap: sell token #1 for token #2", async (app) => {
  // 1. Wait for the swap card to render (Trade tab is the default, index 0).
  await app.waitFor(
    async () => { await app.expectTexts(["Sell", "Buy"]); },
    { timeout: 20000, interval: 500, description: "swap card to load" },
  );
  await ignore(() => app.click("Trade")); // make the active tab explicit

  // 2. Pick the FIRST token for the SELL side.
  await openPicker(app, "swapSellTokenButton");
  const first = await pickToken(app, 0);

  // 3. Pick the SECOND token for the BUY side.
  await openPicker(app, "swapBuyTokenButton");
  const second = await pickToken(app, 1);
  console.log(`    sell ${first} -> buy ${second}`);

  // 5. Enter the sell amount.
  await setSellAmount(app, SELL_AMOUNT);
  await app.expectTexts([SELL_AMOUNT]); // the amount should now be visible

  // 6. Wait for pool resolution — the CTA turns into a live "Swap" button
  //    (canSubmit) once a pool with enough liquidity is found for this pair.
  try {
    await app.waitFor(
      async () => {
        const s = await cardState(app);
        if (!s.canSubmit) throw new Error("not submittable yet");
      },
      { timeout: 15000, interval: 500, description: "pool resolve / CTA ready" },
    );
  } catch {
    await saveShot(app, "swap-cta-not-ready");
    throw new Error(`CTA never became submittable. Card state: ${JSON.stringify(await cardState(app))}`);
  }
  await saveShot(app, "swap-filled"); // filled-in swap form

  // Capture the pool reserves BEFORE the swap (baseline for the on-chain check).
  const before = await poolReserves(app);
  console.log(`    pool reserves before: A=${before.a} B=${before.b}`);

  // 7. Submit -> confirmation dialog -> confirm (all by objectId).
  const submitId = await idByObjectName(app, "swapSubmitButton");
  await app.inspector.send("click", { objectId: submitId });

  // The confirm dialog is a shared TransactionConfirmationDialog (a Popup), so
  // its open state is the Popup's `visible`, and the confirm button carries the
  // shared objectName "transactionConfirmButton".
  const dialogId = await idByObjectName(app, "swapConfirmDialog");
  await app.waitFor(
    async () => { if ((await prop(app, dialogId, "visible")) !== true) throw new Error("dialog not open"); },
    { timeout: 8000, interval: 300, description: "confirm dialog open" },
  );

  // Click the confirm button. QtQuick Controls Buttons don't always react to the
  // inspector's synthetic click, so if the dialog doesn't close, fall back to
  // invoking the dialog's confirm() slot directly.
  const confirmId = await idByObjectName(app, "transactionConfirmButton");
  await app.inspector.send("click", { objectId: confirmId });
  // QtQuick Controls Buttons don't reliably react to the inspector's synthetic
  // click, so if the dialog hasn't closed, invoke confirm() in the dialog's own
  // QML context (this is what fires executeSwap via onConfirmed).
  try {
    await app.waitFor(
      async () => { if ((await prop(app, dialogId, "visible")) === true) throw new Error("still open"); },
      { timeout: 3000, interval: 300, description: "confirm click registered" },
    );
  } catch {
    console.log("    confirm button click didn't take — invoking confirm() via evaluate");
    await ignore(() => app.inspector.send("evaluate", { expression: "confirm()", objectId: dialogId }));
  }

  // 8. Verify the swap actually hit the chain: after a successful submit the
  //    card re-resolves the pool from the sequencer, so the reserves must move.
  //    (Needs an open wallet + reachable local sequencer with this pool.)
  const cardId = await idByObjectName(app, "swapCard");
  let after = before;
  try {
    await app.waitFor(
      async () => {
        // Force a fresh pool read each poll: after a swap the app re-resolves
        // only once and can race the not-yet-applied block, leaving poolReserveA/B
        // stale. Re-trigger doResolvePool() until the applied block shows up.
        await ignore(() => app.inspector.send("evaluate", { expression: "doResolvePool()", objectId: cardId }));
        await new Promise((r) => setTimeout(r, 800));
        after = await poolReserves(app);
        if (after.a === before.a && after.b === before.b)
          throw new Error("reserves unchanged");
      },
      { timeout: 40000, interval: 1200, description: "pool reserves to change on-chain" },
    );
  } catch {
    await saveShot(app, "swap-result");
    const s = await cardState(app);
    const inProgress = await prop(app, cardId, "swapInProgress");
    const sellLeft = await prop(app, cardId, "sellInput");
    throw new Error(
      `pool reserves did not change after the swap.\n` +
      `  before: A=${before.a} B=${before.b}\n` +
      `  after:  A=${after.a} B=${after.b}\n` +
      `  swapInProgress=${inProgress} sellInput="${sellLeft}" swapError=${JSON.stringify(s.swapError)}\n` +
      `  (sellInput="" => executeSwap ran & reset; sellInput="${SELL_AMOUNT}" => confirm never triggered executeSwap)`,
    );
  }
  console.log(`    pool reserves after:  A=${after.a} B=${after.b}  ✓ changed on-chain`);
  await saveShot(app, "swap-result");
});

run();

// How to run these tests (from scratch, interactive + CI): see the
// "Running the UI tests" section in apps/amm/README.md.
