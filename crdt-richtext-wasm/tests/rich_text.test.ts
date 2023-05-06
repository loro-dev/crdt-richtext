import { describe, expect, it } from "vitest";
import { AnnotateType, RichText } from "../nodejs/crdt_richtext_wasm";

describe("rich-text", () => {
  it("run", () => {
    const text = new RichText(BigInt(1));
    text.insert(0, "123");
    const b = new RichText(BigInt(2));
    b.import(text.export(new Uint8Array()));
    expect(b.toString()).toBe("123");
  });

  it("bold", () => {
    const text = new RichText(BigInt(2));
    text.insert(0, "123");
    text.annotate(0, 1, "bold", AnnotateType.BoldLike);
    text.insert(1, "k");
    {
      const spans = text.getAnnSpans();
      expect(spans[0].text).toBe("1k");
      expect(spans[0].annotations).toStrictEqual(new Set(["bold"]));
    }

    text.eraseAnn(0, 2, "bold", AnnotateType.BoldLike);
    {
      const spans = text.getAnnSpans();
      expect(spans[0].text).toBe("1k23");
      expect(spans[0].annotations.size).toBe(0);
    }
  });
});
