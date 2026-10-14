import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { codeBlock, tableCell } from "./index.ts";

const collection = {
  name: "Users API",
  requests: [
    {
      name: "List users",
      description: "Returns one page of users.",
      method: "GET",
      url: "{{baseUrl}}/users",
      query: [{ key: "page", value: "2" }, { key: "debug", value: "1", enabled: false }],
      headers: [{ key: "Accept", value: "application/json" }],
      auth: { type: "bearer", token: "{{secret:token}}" },
      tests: ["status == 200"],
    },
    {
      name: "Create user",
      method: "post",
      url: "{{baseUrl}}/users",
      body: { type: "json", content: '{"name": "Ada"}' },
      auth: { type: "plugin", name: "hmac-signature/hmac", config: { secret: "{{secret:signing}}" } },
    },
    { url: "https://example.com/health" },
  ],
};

test("exports a title, a count and one section per request", async () => {
  const md = await testPlugin(plugin).exporter("markdown", collection);
  expect(md).toMatch(/^# Users API\n\n3 request\(s\)\.\n/);
  expect(md).toContain("### List users\n\n```http\nGET {{baseUrl}}/users\n```\n\nReturns one page of users.");
  expect(md).toContain("### Create user\n\n```http\nPOST {{baseUrl}}/users\n```");
  expect(md).toContain("### GET https://example.com/health");
  expect(md.endsWith("\n")).toBe(true);
});

test("includes query, headers, body and tests", async () => {
  const md = await testPlugin(plugin).exporter("markdown", collection);
  expect(md).toContain("**Query parameters**\n\n| Name | Value |\n|---|---|\n| page | 2 |\n| debug | 1 (disabled) |");
  expect(md).toContain("| Accept | application/json |");
  expect(md).toContain('**Body** (JSON)\n\n```json\n{"name": "Ada"}\n```');
  expect(md).toContain("**Tests**\n\n- `status == 200`");
});

test("describes auth without secrets", async () => {
  const md = await testPlugin(plugin).exporter("markdown", collection);
  expect(md).toContain("**Auth:** bearer");
  expect(md).toContain("**Auth:** plugin `hmac-signature/hmac`");
  expect(md).not.toContain("secret:");
});

test("escapes table cells and code fences", () => {
  expect(tableCell("a|b\nc")).toBe("a\\|b c");
  expect(codeBlock("x ``` y", "md")).toBe("````md\nx ``` y\n````");
});
