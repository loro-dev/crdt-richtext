import { describe, expect, it } from "vitest";
import {
  AnnotateType,
  RichText,
  setPanicHook,
} from "../nodejs/crdt_richtext_wasm";

setPanicHook();
describe("basic ops", () => {
  it("insert & merge", () => {
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

describe("utf16", () => {
  it("insert", () => {
    const text = new RichText(BigInt(1));
    text.insert(0, "你好，世界！");
    text.insert(0, "");
    text.insert(2, "呀");
    expect(text.toString()).toBe("你好呀，世界！");
    text.annotate(0, 3, "bold", AnnotateType.BoldLike);
    const spans = text.getAnnSpans();
    expect(spans.length).toBe(2);
    expect(spans[0].text).toBe("你好呀");
    expect(spans[0].annotations.size).toBe(1);
    expect(spans[1].text.length).toBe(4);
  });

  it("delete", () => {
    const text = new RichText(BigInt(1));
    text.insert(0, "你好，世界！");
    text.delete(0, 0);
    expect(text.toString()).toBe("你好，世界！");
    text.delete(0, 1);
    expect(text.toString()).toBe("好，世界！");
    text.insert(5, "x");
    expect(text.toString()).toBe("好，世界！x");
  });
});
