import { describe, expect, it } from "vitest";
import { canShowOverview } from "./api";

describe("canShowOverview", () => {
  it("requires at least two sessions", () => {
    expect(canShowOverview([])).toBe(false);
    expect(canShowOverview([{ id: "a" }])).toBe(false);
    expect(canShowOverview([{ id: "a" }, { id: "b" }])).toBe(true);
  });
});
