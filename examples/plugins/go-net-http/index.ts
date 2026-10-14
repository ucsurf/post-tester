/**
 * "Copy as code" for Go, using only the standard library.
 *
 * The generated program is complete (package, imports, main) so it can be
 * pasted into a file and run with `go run main.go`.
 */
import { definePlugin } from "@post-tester/plugin-sdk";
import type { PluginRequest } from "@post-tester/plugin-sdk";

/**
 * Quotes text as a Go string literal.
 *
 * JSON string syntax is a subset of Go's interpreted string literals
 * (`\"`, `\\`, `\n`, `\t`, `\uXXXX`...), so JSON.stringify is exact here.
 */
export const goString = (text: string): string => JSON.stringify(text);

/** Go has constants for the common methods; anything else is passed as a string. */
const METHOD_CONSTANTS: Record<string, string> = {
  GET: "http.MethodGet",
  HEAD: "http.MethodHead",
  POST: "http.MethodPost",
  PUT: "http.MethodPut",
  PATCH: "http.MethodPatch",
  DELETE: "http.MethodDelete",
  OPTIONS: "http.MethodOptions",
};

export function generateGo(request: PluginRequest): string {
  const hasBody = request.body.length > 0;
  const method = METHOD_CONSTANTS[request.method] ?? goString(request.method);
  const body = hasBody ? `strings.NewReader(${goString(request.body)})` : "nil";
  const imports = ['"fmt"', '"io"', '"log"', '"net/http"', ...(hasBody ? ['"strings"'] : [])].sort();

  const lines = [
    "package main",
    "",
    "import (",
    ...imports.map((name) => `\t${name}`),
    ")",
    "",
    "func main() {",
    `\treq, err := http.NewRequest(${method}, ${goString(request.url)}, ${body})`,
    "\tif err != nil {",
    "\t\tlog.Fatal(err)",
    "\t}",
    // Header.Add (not Set) keeps repeated headers, matching what post-tester sends.
    ...request.headers.entries().map(([name, value]) => `\treq.Header.Add(${goString(name)}, ${goString(value)})`),
    "",
    "\tres, err := http.DefaultClient.Do(req)",
    "\tif err != nil {",
    "\t\tlog.Fatal(err)",
    "\t}",
    "\tdefer res.Body.Close()",
    "",
    "\tdata, err := io.ReadAll(res.Body)",
    "\tif err != nil {",
    "\t\tlog.Fatal(err)",
    "\t}",
    "\tfmt.Println(res.Status)",
    "\tfmt.Println(string(data))",
    "}",
    "",
  ];
  return lines.join("\n");
}

export default definePlugin({
  codegen: {
    "go-net-http": {
      displayName: "Go (net/http)",
      language: "go",
      generate: (request) => generateGo(request),
    },
  },
});
