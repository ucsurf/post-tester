import { describe, expect, it } from "vitest";
import { describePermission } from "./permissions";

describe("describePermission", () => {
  it("explains each permission in plain language", () => {
    expect(describePermission("network:api.github.com")).toEqual({ text: "Send HTTP requests to api.github.com", risk: "medium" });
    expect(describePermission("network:*").risk).toBe("high");
    expect(describePermission("secrets:read").text).toMatch(/secrets/);
    expect(describePermission("fs:write").text).toMatch(/change files/);
    expect(describePermission("everything").text).toMatch(/Unknown/);
  });
});
