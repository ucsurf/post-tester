import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { splitUrl } from "./index.ts";

/** A small HAR 1.2 file with a GET, a JSON POST, a form POST and a text PUT. */
const sample = JSON.stringify({
  log: {
    version: "1.2",
    creator: { name: "Browser DevTools" },
    entries: [
      {
        request: {
          method: "GET",
          url: "https://api.example.com/users?page=2&q=ada%20lovelace&flag#top",
          headers: [
            { name: ":authority", value: "api.example.com" },
            { name: "accept", value: "application/json" },
            { name: "cookie", value: "session=abc" },
            { name: "x-trace", value: "1" },
          ],
        },
      },
      {
        request: {
          method: "post",
          url: "https://api.example.com/users",
          headers: [{ name: "Content-Type", value: "application/json" }, { name: "Content-Length", value: "15" }],
          postData: { mimeType: "application/json; charset=utf-8", text: '{"name":"Ada"}' },
        },
      },
      {
        request: {
          method: "POST",
          url: "http://localhost:8080/login",
          headers: [{ name: "Content-Type", value: "application/x-www-form-urlencoded" }],
          postData: {
            mimeType: "application/x-www-form-urlencoded",
            text: "user=ada&pass=x",
            params: [{ name: "user", value: "ada" }, { name: "pass", value: "x" }],
          },
        },
      },
      {
        request: { method: "PUT", url: "https://files.example.com/notes/1", headers: [], postData: { mimeType: "text/markdown", text: "# Notes" } },
      },
    ],
  },
});

test("detects HAR files and nothing else", async () => {
  const t = testPlugin(plugin);
  expect(await t.detect("har", sample)).toBe(true);
  expect(await t.detect("har", '{"entries": []}')).toBe(false);
  expect(await t.detect("har", '{"log": {"entries": "nope"}}')).toBe(false);
  expect(await t.detect("har", "not json")).toBe(false);
});

test("imports every entry, grouped by host", async () => {
  const result = await testPlugin(plugin).importer("har", sample);
  expect(result.name).toBe("HAR from Browser DevTools");
  expect(result.requests.map((r) => [r.folder, r.name])).toEqual([
    ["api.example.com", "GET /users"],
    ["api.example.com", "POST /users"],
    ["localhost-8080", "POST /login"],
    ["files.example.com", "PUT /notes/1"],
  ]);
});

test("moves the query string into query parameters", async () => {
  const [get] = (await testPlugin(plugin).importer("har", sample)).requests;
  expect(get.url).toBe("https://api.example.com/users");
  expect(get.query).toEqual([
    { key: "page", value: "2" },
    { key: "q", value: "ada lovelace" },
    { key: "flag", value: "" },
  ]);
});

test("keeps useful headers and drops pseudo, computed and cookie headers", async () => {
  const [get, post] = (await testPlugin(plugin).importer("har", sample)).requests;
  expect(get.headers).toEqual([{ key: "accept", value: "application/json" }, { key: "x-trace", value: "1" }]);
  expect(post.headers).toEqual([{ key: "Content-Type", value: "application/json" }]);
});

test("converts JSON, form and text bodies", async () => {
  const [, json, form, text] = (await testPlugin(plugin).importer("har", sample)).requests;
  expect(json.method).toBe("POST");
  expect(json.body).toEqual({ type: "json", content: '{"name":"Ada"}' });
  expect(form.body).toEqual({ type: "form", fields: [{ key: "user", value: "ada" }, { key: "pass", value: "x" }] });
  expect(text.body).toEqual({ type: "text", content: "# Notes", content_type: "text/markdown" });
});

test("invalid input explains what is expected", async () => {
  let error: Error | undefined;
  try {
    await testPlugin(plugin).importer("har", '{"log": {}}');
  } catch (e) {
    error = e as Error;
  }
  expect(error?.message).toContain('expected JSON with a "log.entries" array');
});

test("splitUrl handles URLs without a query and bad encoding", () => {
  expect(splitUrl("https://a.dev/x")).toEqual({ base: "https://a.dev/x", query: [] });
  expect(splitUrl("https://a.dev/?a=%E0%A4%A&b=c+d")).toEqual({
    base: "https://a.dev/",
    query: [{ key: "a", value: "%E0%A4%A" }, { key: "b", value: "c d" }],
  });
});
