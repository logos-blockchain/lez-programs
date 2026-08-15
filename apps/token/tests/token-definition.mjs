// Token UI end-to-end test for Logos Basecamp.
//
// Normal mode drives the complete visible create -> inspect shell without
// mutating chain state. Set TOKEN_E2E_LIVE=1 to create fresh wallet accounts,
// submit a fixed-supply definition through token_module, and verify it appears
// in the live Inspect view.

import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { mkdir, writeFile } from "node:fs/promises";

const fwRoot =
  process.env.LOGOS_QT_MCP ||
  new URL("../result-mcp", import.meta.url).pathname;
const { test, run } = await import(resolve(fwRoot, "test-framework/framework.mjs"));

const live = process.env.TOKEN_E2E_LIVE === "1";
const evidenceDir = process.env.TOKEN_E2E_OUTPUT
  ? resolve(process.env.TOKEN_E2E_OUTPUT)
  : fileURLToPath(
      new URL(
        "../../../.3esmit/projects/lez-programs/docs/token-basecamp-e2e/",
        import.meta.url,
      ),
    );
const tokenName = process.env.TOKEN_E2E_NAME || `Basecamp E2E ${Date.now()}`;

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

async function clickObject(app, name) {
  const objectId = await idByObjectName(app, name);
  const result = await app.inspector.send("click", { objectId });
  if (result.error)
    throw new Error(`click ${name}: ${result.error}`);
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
      const clickResult = await app.inspector.send("click", { objectId: match.id });
      if (clickResult.error)
        throw new Error(`click ${name}: ${clickResult.error}`);
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

async function chooseFixedTemplate(app) {
  await clickObject(app, "tokenExamplesButton");
  await app.waitFor(
    async () => {
      const menuId = await idByObjectName(app, "tokenExamplesMenu");
      if ((await prop(app, menuId, "visible")) !== true)
        throw new Error("template menu is closed");
    },
    { timeout: 3000, interval: 150, description: "template menu to open" },
  );
  await clickObject(app, "tokenFixedTemplate");
  const nameFieldId = await idByObjectName(app, "tokenNameField");
  await waitForProperty(
    app,
    nameFieldId,
    "text",
    (value) => value === "Fixed supply token",
    "fixed template values",
  );
}

async function openTokenApp(app) {
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

test(live
  ? "token definition: create through token_module and inspect live state"
  : "token UI: create shell flows into Inspect", async (app) => {
  await openTokenApp(app);
  await app.waitFor(
    async () => { await app.expectTexts(["Create definition", "Fungible", "Use template"]); },
    { timeout: 20000, interval: 400, description: "token create view to load" },
  );

  const createPageId = await idByObjectName(app, "tokenCreatePage");
  await saveShot(app, "token-create-initial");

  await chooseFixedTemplate(app);
  const nameFieldId = await idByObjectName(app, "tokenNameField");
  await setProp(app, nameFieldId, "text", tokenName);
  await saveShot(app, "token-create-template");

  await clickFirstVisible(app, ["tokenContinueButton", "tokenSummaryContinueButton"]);
  await waitForProperty(
    app,
    createPageId,
    "step",
    (value) => value === 1,
    "account-target step",
  );

  if (!live) {
    const createAccountsButtonId = await idByObjectName(app, "tokenCreateAccountsButton");
    if (await prop(app, createAccountsButtonId, "enabled") === true)
      throw new Error("visual mode unexpectedly has a live wallet");
    await saveShot(app, "token-account-targets-disconnected");

    await app.click("Inspect");
    await app.waitFor(
      async () => { await app.expectTexts(["Inspect token definitions", "Definition index"]); },
      { timeout: 10000, interval: 300, description: "Inspect view to load" },
    );
    await app.waitFor(
      async () => {
        const result = await app.findByProperty("text", "Pebble");
        if (!result.matches || result.matches.length === 0)
          throw new Error("example definitions not rendered");
      },
      { timeout: 5000, interval: 250, description: "example definitions" },
    );
    await saveShot(app, "token-inspect-examples");
    console.log("    mode: visual shell; chain state unchanged");
    return;
  }

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

  const definitionTargetId = await idByObjectName(app, "tokenDefinitionTargetField");
  const holdingTargetId = await idByObjectName(app, "tokenHoldingTargetField");
  const validAccount = (value) => typeof value === "string" && value.length > 0;
  await waitForProperty(app, definitionTargetId, "text", validAccount, "definition target account", 20000);
  await waitForProperty(app, holdingTargetId, "text", validAccount, "holding target account", 20000);
  await saveShot(app, "token-account-targets-ready");

  await clickObject(app, "tokenReviewButton");
  await waitForProperty(app, createPageId, "step", (value) => value === 2, "definition review");
  await saveShot(app, "token-definition-review");

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
  await saveShot(app, "token-definition-submitted");

  await app.click("Inspect");
  await app.waitFor(
    async () => { await app.expectTexts(["Inspect token definitions", "Definition index"]); },
    { timeout: 10000, interval: 300, description: "live Inspect view" },
  );
  await app.waitFor(
    async () => {
      try { await clickObject(app, "tokenRefreshButton"); } catch { /* refresh may be busy */ }
      const result = await app.findByProperty("text", tokenName);
      if (!result.matches || result.matches.length === 0)
        throw new Error(`live definition ${tokenName} not indexed yet`);
    },
    { timeout: 60000, interval: 1200, description: "created definition in live Inspect view" },
  );
  await app.expectTexts([tokenName, "Network"]);
  await saveShot(app, "token-inspect-live-definition");
  console.log(`    mode: live; verified ${tokenName} in wallet-backed Inspect view`);
});

run();
