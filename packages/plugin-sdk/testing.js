// @post-tester/plugin-sdk/testing
//
// post-tester has this module built in and serves it to test files run by
// `pt plugin test`. This copy exists for editors and type checkers. It calls a
// plugin exactly like post-tester does, so tests exercise the real code path.

/**
 * Wraps a plugin definition for testing.
 *
 *   const t = testPlugin(plugin);
 *   expect(await t.templateFunction("totp", { secret: "..." })).toMatch(/^\d{6}$/);
 */
export function testPlugin(plugin) {
  const call = (kind, name, payload) => {
    // `__pt_dispatch` is installed by post-tester's sandbox.
    if (typeof globalThis.__pt_dispatch !== "function") {
      throw new Error("testPlugin() only works inside post-tester: run your tests with `pt plugin test`");
    }
    return globalThis.__pt_dispatch(plugin, kind, name, payload);
  };
  const request = (r = {}) => ({ method: "GET", url: "https://example.com/", headers: [], body: "", ...r });
  const response = (r = {}) => ({ status: 200, statusText: "OK", headers: [], body: "", timeMs: 1, ...r });
  return {
    templateFunction: (name, args = {}) => call("templateFunction", name, { args }),
    auth: (name, req, config = {}) => call("auth", name, { request: request(req), config }),
    beforeRequest: (req) => call("beforeRequest", "", { request: request(req) }),
    afterResponse: (res) => call("afterResponse", "", { response: response(res) }),
    importer: (name, text) => call("importer", name, { text }),
    detect: (name, text) => call("importerDetect", name, { text }),
    exporter: (name, collection) => call("exporter", name, { collection }),
    codegen: (name, req) => call("codegen", name, { request: request(req) }),
    viewer: (name, res) => call("viewer", name, { response: response(res) }),
    mcpTool: (name, input = {}) => call("mcpTool", name, { input }),
    describe: () => call("describe", "", {}),
  };
}
