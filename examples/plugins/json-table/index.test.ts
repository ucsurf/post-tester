import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { escapeHtml } from "./index.ts";

const render = (body: unknown) =>
  testPlugin(plugin).viewer("table", { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });

test("an array of objects becomes rows with the union of all keys", async () => {
  const html = await render([{ id: 1, name: "Ada" }, { id: 2, email: "b@example.com" }]);
  expect(html).toContain("<thead><tr><th>id</th><th>name</th><th>email</th></tr></thead>");
  expect(html).toContain("<tr><td>1</td><td>Ada</td><td></td></tr>");
  expect(html).toContain("<tr><td>2</td><td></td><td>b@example.com</td></tr>");
});

test("an object becomes key/value rows, nested values as JSON", async () => {
  const html = await render({ id: 7, tags: ["a", "b"], owner: null });
  expect(html).toContain("<tr><td>id</td><td>7</td></tr>");
  expect(html).toContain("<tr><td>tags</td><td>[&quot;a&quot;,&quot;b&quot;]</td></tr>");
  expect(html).toContain("<tr><td>owner</td><td>null</td></tr>");
});

test("escapes HTML in keys and values", async () => {
  const html = await render([{ "<b>key</b>": `<script>alert("x")</script> & 'y'` }]);
  expect(html).not.toContain("<script>");
  expect(html).not.toContain("<b>");
  expect(html).toContain("<th>&lt;b&gt;key&lt;/b&gt;</th>");
  expect(html).toContain("<td>&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt; &amp; &#39;y&#39;</td>");
});

test("arrays of plain values and empty arrays", async () => {
  expect(await render([1, "two"])).toContain("<tr><td>1</td></tr><tr><td>two</td></tr>");
  expect(await render([])).toBe("<p>The array is empty.</p>");
});

test("invalid JSON is explained, escaped", async () => {
  const html = await testPlugin(plugin).viewer("table", { body: "<html>not json" });
  expect(html).toMatch(/^<p>Cannot show this response as a table: the response body is not valid JSON/);
  expect(html).not.toContain("<html>");
});

test("escapeHtml handles every special character", () => {
  expect(escapeHtml(`&<>"'`)).toBe("&amp;&lt;&gt;&quot;&#39;");
});
