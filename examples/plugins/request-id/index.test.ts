import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { HEADER, THRESHOLD_VARIABLE } from "./index.ts";

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

test("adds a UUID request id", async () => {
  const request = await testPlugin(plugin).beforeRequest({ url: "https://api.example.com/users" });
  const headers = Object.fromEntries(request.headers);
  expect(headers[HEADER]).toMatch(UUID);
  expect(request.url).toBe("https://api.example.com/users");
});

test("every request gets a different id", async () => {
  const t = testPlugin(plugin);
  const first = Object.fromEntries((await t.beforeRequest()).headers)[HEADER];
  const second = Object.fromEntries((await t.beforeRequest()).headers)[HEADER];
  expect(first).not.toBe(second);
});

test("keeps an existing id, whatever its case", async () => {
  const request = await testPlugin(plugin).beforeRequest({ headers: { "x-request-id": "trace-42" } });
  expect(request.headers).toEqual([["x-request-id", "trace-42"]]);
});

test("passes responses through unchanged", async () => {
  const response = await testPlugin(plugin).afterResponse({ status: 201, body: "{}", timeMs: 5000 });
  expect(response.status).toBe(201);
  expect(response.body).toBe("{}");
  expect(response.time_ms).toBe(5000);
});

test("the threshold can be set with a variable", async () => {
  // Variables set here are session variables; reset it so other tests are unaffected.
  ctx.variables.set(THRESHOLD_VARIABLE, "50");
  try {
    const response = await testPlugin(plugin).afterResponse({ timeMs: 60 });
    expect(response.status).toBe(200);
  } finally {
    ctx.variables.set(THRESHOLD_VARIABLE, "");
  }
});

test("describes its hooks", async () => {
  expect((await testPlugin(plugin).describe()).hooks).toEqual(["beforeRequest", "afterResponse"]);
});
