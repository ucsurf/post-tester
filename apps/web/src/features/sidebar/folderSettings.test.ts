import type { FolderSettings } from "@post-tester/bindings";
import { describe, expect, it } from "vitest";
import { applyForm, toForm } from "./folderSettings";

describe("folder settings form", () => {
  const loaded = {
    name: "Users",
    description: "",
    headers: [{ key: "x-team", value: "core" }],
    auth: { type: "bearer", token: "{{token}}" },
    variables: { userId: "2" },
    // Written by a newer post-tester or by hand; the editor must not drop it.
    retries: 3,
  } as FolderSettings & { retries: number };

  it("round-trips without changes and keeps unknown fields", () => {
    expect(applyForm(loaded, toForm(loaded))).toEqual(loaded);
  });

  it("applies edits, drops blank rows, and removes auth when set to inherit", () => {
    const form = toForm(loaded);
    const saved = applyForm(loaded, {
      ...form,
      name: "  People ",
      headers: [...form.headers, { key: "", value: "" }],
      variables: [...form.variables, { key: "region", value: "eu" }, { key: " ", value: "orphan" }],
      auth: undefined,
    });
    expect(saved).toEqual({ name: "People", description: "", headers: [{ key: "x-team", value: "core" }], variables: { userId: "2", region: "eu" }, retries: 3 });
    expect("auth" in saved).toBe(false);
  });
});
