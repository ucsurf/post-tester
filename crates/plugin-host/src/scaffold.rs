//! `pt plugin new`: creates a working plugin from a template.

use crate::manifest::PLUGIN_TYPES;
use std::path::Path;

/// Creates a plugin of `kind` named `name` in `dir`. Fails if `dir` is not empty.
pub fn create(dir: &Path, kind: &str, name: &str) -> Result<Vec<String>, String> {
    if !PLUGIN_TYPES.iter().any(|(t, _)| *t == kind) {
        let valid: Vec<&str> = PLUGIN_TYPES.iter().map(|(t, _)| *t).collect();
        return Err(format!("unknown plugin type `{kind}`. Choose one of: {}", valid.join(", ")));
    }
    let valid_name = !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_name {
        return Err(format!("`{name}` is not a valid plugin name; use lowercase letters, numbers and `-`"));
    }
    if dir.exists() && dir.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
        return Err(format!("{} is not empty", dir.display()));
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;

    let (code, test, permissions) = template(kind);
    let manifest = serde_json::json!({
        "$schema": "./node_modules/@post-tester/plugin-sdk/plugin.schema.json",
        "name": name,
        "description": format!("TODO: describe what {name} does in one sentence."),
        "version": "0.1.0",
        "engine": format!("^{}", post_tester_core::VERSION),
        "main": "index.ts",
        "provides": [kind],
        "permissions": permissions,
    });
    let readme = format!(
        "# {name}\n\nTODO: what this plugin does.\n\n## Usage\n\nTODO: one example.\n\n## Development\n\n```bash\npt plugin validate\npt plugin test\npt plugin install .\n```\n"
    );
    let files = [
        ("plugin.json", serde_json::to_string_pretty(&manifest).expect("valid JSON") + "\n"),
        ("index.ts", code.to_string()),
        ("index.test.ts", test.to_string()),
        ("README.md", readme),
    ];
    for (file, content) in &files {
        std::fs::write(dir.join(file), content).map_err(|e| e.to_string())?;
    }
    Ok(files.iter().map(|(f, _)| f.to_string()).collect())
}

/// (index.ts, index.test.ts, permissions) for each plugin type.
fn template(kind: &str) -> (&'static str, &'static str, Vec<&'static str>) {
    match kind {
        "template-function" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: {
    shout: {
      description: "Returns the text in upper case.",
      args: [{ name: "text", type: "string", description: "Text to convert" }],
      run: ({ text }) => String(text).toUpperCase(),
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("shout upper-cases text", async () => {
  const t = testPlugin(plugin);
  expect(await t.templateFunction("shout", { text: "hi" })).toBe("HI");
});
"#,
            vec![],
        ),
        "auth" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  auth: {
    "custom-token": {
      displayName: "Custom token",
      fields: [{ name: "token", type: "secret", label: "Token" }],
      apply: (request, { token }) => {
        request.headers.set("X-Custom-Token", token);
        return request;
      },
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("adds the token header", async () => {
  const t = testPlugin(plugin);
  const request = await t.auth("custom-token", {}, { token: "abc" });
  expect(request.headers).toContain(["X-Custom-Token", "abc"]);
});
"#,
            vec![],
        ),
        "hook" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  hooks: {
    beforeRequest: (request, ctx) => {
      request.headers.set("X-Request-Id", ctx.random.uuid());
      return request;
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("adds a request id", async () => {
  const request = await testPlugin(plugin).beforeRequest({ url: "https://example.com" });
  const id = request.headers.find(([name]) => name === "X-Request-Id");
  expect(id).toBeDefined();
});
"#,
            vec![],
        ),
        "importer" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

/** Imports a plain list of URLs, one per line. */
export default definePlugin({
  importers: {
    "url-list": {
      displayName: "URL list",
      fileExtensions: [".txt"],
      detect: (text) => text.split("\n").every((line) => !line.trim() || line.startsWith("http")),
      import: (text) => ({
        name: "URL list",
        requests: text
          .split("\n")
          .filter((line) => line.trim())
          .map((url) => ({ name: url.trim(), method: "GET", url: url.trim() })),
      }),
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("imports one request per line", async () => {
  const result = await testPlugin(plugin).importer("url-list", "https://a.dev\nhttps://b.dev\n");
  expect(result.requests).toHaveLength(2);
});
"#,
            vec![],
        ),
        "exporter" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

/** Exports a collection as a Markdown list of requests. */
export default definePlugin({
  exporters: {
    markdown: {
      displayName: "Markdown",
      fileExtension: ".md",
      export: (collection) =>
        `# ${collection.name}\n\n` + collection.requests.map((r) => `- **${r.method}** ${r.url}`).join("\n") + "\n",
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("exports markdown", async () => {
  const md = await testPlugin(plugin).exporter("markdown", { name: "API", requests: [{ method: "GET", url: "https://a.dev" }] });
  expect(md).toContain("**GET** https://a.dev");
});
"#,
            vec![],
        ),
        "codegen" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  codegen: {
    httpie: {
      displayName: "HTTPie",
      language: "shell",
      generate: (request) => {
        const headers = request.headers.entries().map(([k, v]) => ` '${k}:${v}'`).join("");
        return `http ${request.method} '${request.url}'${headers}`;
      },
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("generates an httpie command", async () => {
  const code = await testPlugin(plugin).codegen("httpie", { method: "GET", url: "https://a.dev" });
  expect(code).toBe("http GET 'https://a.dev'");
});
"#,
            vec![],
        ),
        "viewer" => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

/** Shows a JSON array of objects as an HTML table. */
export default definePlugin({
  viewers: {
    table: {
      displayName: "Table",
      contentTypes: ["application/json"],
      render: (response) => {
        const rows = response.json();
        if (!Array.isArray(rows) || rows.length === 0) return "<p>Not a list of objects.</p>";
        const columns = Object.keys(rows[0]);
        const escape = (v) => String(v).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
        const head = columns.map((c) => `<th>${escape(c)}</th>`).join("");
        const body = rows.map((r) => `<tr>${columns.map((c) => `<td>${escape(r[c] ?? "")}</td>`).join("")}</tr>`).join("");
        return `<table><thead><tr>${head}</tr></thead><tbody>${body}</tbody></table>`;
      },
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("renders a table", async () => {
  const html = await testPlugin(plugin).viewer("table", { body: '[{"id":1}]' });
  expect(html).toContain("<td>1</td>");
});
"#,
            vec![],
        ),
        _ => (
            r#"import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  mcpTools: {
    word_count: {
      description: "Counts the words in a text.",
      inputSchema: {
        type: "object",
        properties: { text: { type: "string", description: "The text to count" } },
        required: ["text"],
      },
      run: ({ text }) => ({ words: String(text).split(/\s+/).filter(Boolean).length }),
    },
  },
});
"#,
            r#"import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

test("counts words", async () => {
  expect(await testPlugin(plugin).mcpTool("word_count", { text: "a b c" })).toEqual({ words: 3 });
});
"#,
            vec![],
        ),
    }
}
