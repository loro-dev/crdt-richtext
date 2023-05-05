import { describe, expect, it } from "vitest";
import { RichText } from "../nodejs/crdt_richtext_wasm";

describe("rich-text", () => {
  it("run", () => {
    const text = new RichText(BigInt(1));
    text.insert(0, "123");
    const b = new RichText(BigInt(2));
    b.import(text.export(new Uint8Array()));
    expect(b.toString()).toBe("123");
  });
});
