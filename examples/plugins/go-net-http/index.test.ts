import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { goString } from "./index.ts";

test("generates a GET without a body or the strings import", async () => {
  const code = await testPlugin(plugin).codegen("go-net-http", { url: "https://api.example.com/users?page=2" });
  expect(code).toContain('req, err := http.NewRequest(http.MethodGet, "https://api.example.com/users?page=2", nil)');
  expect(code).not.toContain('"strings"');
  expect(code).toContain("package main");
  expect(code).toContain("res, err := http.DefaultClient.Do(req)");
});

test("generates a POST with headers and an escaped body", async () => {
  const code = await testPlugin(plugin).codegen("go-net-http", {
    method: "POST",
    url: "https://api.example.com/users",
    headers: [["Content-Type", "application/json"], ["X-Note", 'say "hi"']],
    body: '{"name": "Ada",\n "tags": ["a\\b"]}',
  });
  expect(code).toContain('http.NewRequest(http.MethodPost, "https://api.example.com/users", strings.NewReader("{\\"name\\": \\"Ada\\",\\n \\"tags\\": [\\"a\\\\b\\"]}"))');
  expect(code).toContain('req.Header.Add("Content-Type", "application/json")');
  expect(code).toContain('req.Header.Add("X-Note", "say \\"hi\\"")');
  expect(code).toContain('\t"strings"\n');
});

test("keeps repeated headers", async () => {
  const code = await testPlugin(plugin).codegen("go-net-http", { headers: [["Cookie", "a=1"], ["Cookie", "b=2"]] });
  expect(code).toContain('req.Header.Add("Cookie", "a=1")\n\treq.Header.Add("Cookie", "b=2")');
});

test("uses a string for custom methods", async () => {
  const code = await testPlugin(plugin).codegen("go-net-http", { method: "PURGE" });
  expect(code).toContain('http.NewRequest("PURGE", ');
});

test("Go string quoting escapes control characters", () => {
  expect(goString("tab\there\u0001")).toBe('"tab\\there\\u0001"');
});

test("describes the generator", async () => {
  expect((await testPlugin(plugin).describe()).codegen).toEqual([{ name: "go-net-http", displayName: "Go (net/http)", language: "go" }]);
});
