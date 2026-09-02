import { resolve } from "node:path";

const frameworkRoot = process.env.LOGOS_QT_MCP;
if (!frameworkRoot)
  throw new Error("LOGOS_QT_MCP is required");

const { test, run } = await import(resolve(frameworkRoot, "test-framework/framework.mjs"));

test("amm ui: renders primary navigation", async (app) => {
  await app.waitFor(
    async () => { await app.expectTexts(["Trade", "Liquidity", "Pools"]); },
    { timeout: 20000, interval: 500, description: "primary navigation" },
  );
});

run();
