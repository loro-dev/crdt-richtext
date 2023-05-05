import { describe, expect, it } from "vitest";
import { add } from "../";

it("add", () => {
  expect(add(0, 2)).toBe(2);
});
