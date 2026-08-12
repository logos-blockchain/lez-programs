// ---------------------------------------------------------------------------
// AMM UI test — add a CUSTOM (unlisted) token by id through the Liquidity view.
//
// Token D is created on-chain by the setup script but deliberately LEFT OUT of
// the token config, so it never appears in the selector on its own. This test
// pastes D's definition id into a liquidity token slot and verifies the app
// RESOLVES it (reads its on-chain definition), SELECTS it, and PERSISTS it to the
// custom-token store — the addCustomToken path. No pool / submit is involved, so
// it needs neither an open wallet nor a seeded pool.
//
// NOTE: the add path is holding-agnostic — a token resolves from its public
// definition whether or not the wallet holds it (balance shows "0" when not held).
// D happens to be held by the test wallet only because minting requires the holding
// account to sign; a genuinely un-owned token adds via exactly the same path.
//
// Prereqs in the running app (see apps/amm/tests/README.md):
//   * launched against the isolated test wallet + TOKENS_CONFIG the setup writes
//     (TKA, TKB, TKC — NOT token D) and CUSTOM_TOKEN_CONFIG pointing at a
//     writable path (defaults below, matching the README launch line)
//   * a reachable local sequencer (to read D's definition)
// ---------------------------------------------------------------------------

import { resolve } from "node:path";
import { rm } from "node:fs/promises";
import { execFileSync } from "node:child_process";

const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

// The isolated test wallet home (set by the setup script) — used to resolve token
// D's deterministic definition id via the wallet CLI, the same way the setup does.
const WALLET_HOME =
  process.env.LEE_WALLET_HOME_DIR ||
  new URL("./testnet/.wallet", import.meta.url).pathname;

// Where the app persists custom tokens. Defaults to the isolated test store; set
// CUSTOM_TOKEN_CONFIG to override. IMPORTANT: launch the app with the SAME path
// (CUSTOM_TOKEN_CONFIG) so the test's clean-slate clears the store the app actually
// uses — otherwise the app writes to its default per-user store and the test starts
// from a stale slate. Only used to pre-clear; persistence is verified through the app.
const CUSTOM_TOKEN_CONFIG =
  process.env.CUSTOM_TOKEN_CONFIG ||
  new URL("./testnet/custom-tokens.json", import.meta.url).pathname;

// --- small helpers (mirror create-pool.mjs) --------------------------------

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

async function evaluate(app, id, expression) {
  await app.inspector.send("evaluate", { expression, objectId: id });
}

async function saveShot(app, name) {
  const shot = await ignore(() => app.screenshot());
  if (shot && shot.image) {
    const path = new URL(`./${name}.png`, import.meta.url).pathname;
    await import("node:fs/promises").then(({ writeFile }) =>
      writeFile(path, Buffer.from(shot.image, "base64")));
    console.log(`    screenshot -> ${path}`);
  }
}

// Resolve token D's definition id from the test wallet (deterministic under the
// test mnemonic), the same account label the setup script mints it to.
function resolveTokenD() {
  const out = execFileSync("wallet", ["account", "id", "--account-id", "token-d-def"], {
    encoding: "utf8",
    env: { ...process.env, LEE_WALLET_HOME_DIR: WALLET_HOME, NSSA_WALLET_HOME_DIR: WALLET_HOME },
  });
  const id = (out.match(/[1-9A-HJ-NP-Za-km-z]{32,44}/) || [])[0];
  if (!id)
    throw new Error(`could not resolve token-d-def id from wallet (home=${WALLET_HOME}) — run the setup script`);
  return id;
}

// Trigger a token-list reload and wait for it to complete. resolveTokens() re-reads the
// persisted custom-token store from disk (in C++), so a token that survives a reload was
// genuinely persisted — no need to know the app's store path.
async function reloadTokens(app, pageId) {
  await evaluate(app, pageId, "refreshTokens()");
  await app.waitFor(
    async () => { if ((await prop(app, pageId, "tokensLoading")) === true) throw new Error("reloading"); },
    { timeout: 10000, interval: 200, description: "token reload to finish" },
  );
}

async function selectableIds(app, formId) {
  // Read the CSV string form (var arrays don't reliably serialize over the inspector).
  const csv = await prop(app, formId, "selectableTokenIdsCsv");
  return typeof csv === "string" && csv.length > 0 ? csv.split(",") : [];
}

// --- the test ---------------------------------------------------------------

test("amm liquidity: add a custom (unlisted) token by id", async (app) => {
  const tokenD = resolveTokenD();
  console.log(`    custom token D = ${tokenD}`);

  // 1. Switch to the Liquidity tab and wait for the form + page to render.
  await app.waitFor(
    async () => { await app.expectTexts(["Trade", "Liquidity"]); },
    { timeout: 20000, interval: 500, description: "nav bar to load" },
  );
  await ignore(() => app.click("Liquidity"));
  await app.waitFor(
    async () => { await idByObjectName(app, "newPositionForm"); },
    { timeout: 10000, interval: 300, description: "liquidity form to render" },
  );
  const formId = await idByObjectName(app, "newPositionForm");
  const pageId = await idByObjectName(app, "liquidityPage");

  // 2. Clean slate: clear the isolated custom-token store the app uses and reload, so D is
  //    genuinely absent and pasting it must go through addCustomToken (not a direct select).
  //    If D is still listed after this, the app isn't using this store — launch it with
  //    CUSTOM_TOKEN_CONFIG pointing here (see README).
  await rm(CUSTOM_TOKEN_CONFIG, { force: true });
  await reloadTokens(app, pageId);
  if ((await selectableIds(app, formId)).includes(tokenD))
    throw new Error(
      `token D is still listed after clearing ${CUSTOM_TOKEN_CONFIG} — launch the app with ` +
      "CUSTOM_TOKEN_CONFIG set to this same path so the test controls the store (see README).",
    );

  // 3. Paste D's id into token slot A — the same entry point the token input's
  //    onTokenEntered uses. It's unlisted, so resolveToken routes through the app's
  //    addCustomToken: resolve the on-chain definition, persist, select.
  await evaluate(app, formId, `resolveToken("A", "${tokenD}")`);

  // 4. Wait for the resolution to complete: D selected on side A, no resolution error.
  try {
    await app.waitFor(
      async () => {
        const err = await prop(app, formId, "tokenResolutionError");
        if (err) throw new Error(`resolution failed: ${err}`);
        const selected = await prop(app, formId, "selectedTokenAId");
        if (selected !== tokenD) throw new Error(`D not selected yet (selectedTokenAId=${selected})`);
      },
      { timeout: 20000, interval: 500, description: "token D resolved + selected" },
    );
  } catch (e) {
    await saveShot(app, "custom-token-not-resolved");
    const err = await prop(app, formId, "tokenResolutionError");
    throw new Error(`${e.message}. tokenResolutionError=${err}`);
  }

  // 5. The persistence proof: reload the list (re-reads the store from disk) and confirm D
  //    SURVIVES. D isn't in the token config, so if it's still selectable after a reload it
  //    can only have come from the persisted custom-token store — i.e. addCustomToken wrote
  //    it. If persistence silently failed, D would vanish here.
  await reloadTokens(app, pageId);
  if (!(await selectableIds(app, formId)).includes(tokenD)) {
    await saveShot(app, "custom-token-not-persisted");
    throw new Error(
      "token D disappeared after a reload — it resolved + selected but was NOT persisted to the " +
      "custom-token store. Launch the app with CUSTOM_TOKEN_CONFIG set to a writable path (see README).",
    );
  }

  console.log("    token D added as a custom token  ✓  (survives a token-list reload)");
  await saveShot(app, "custom-token-added");

  // Leave no side effects: clear the persisted custom token. The running app keeps it in
  // memory until restart, but the next run's clean-slate step re-reads this cleared store.
  await rm(CUSTOM_TOKEN_CONFIG, { force: true });
});

run();

// How to run: see apps/amm/tests/README.md — same flow as create-pool.mjs, plus
// launch the UI with CUSTOM_TOKEN_CONFIG set (the setup creates token D on-chain
// but leaves it out of the token config).
