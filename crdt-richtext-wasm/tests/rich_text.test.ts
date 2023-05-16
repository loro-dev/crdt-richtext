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
    text.annotate({ start: 0, end: 1 }, "bold", null);
    text.insert(1, "k");
    {
      const spans = text.getAnnSpans();
      expect(spans[0].insert).toBe("1k");
      expect(spans[0].attributions).toStrictEqual(
        new Map([["bold", undefined]]),
      );
    }

    text.eraseAnn({ start: 0, end: 2 }, "bold");
    {
      const spans = text.getAnnSpans();
      expect(spans[0].insert).toBe("1k23");
      expect(spans[0].attributions.size).toBe(0);
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
    text.annotate({ start: 0, end: 3 }, "bold", null);
    const spans = text.getAnnSpans();
    expect(spans.length).toBe(2);
    expect(spans[0].insert).toBe("你好呀");
    expect(spans[0].attributions.size).toBe(1);
    expect(spans[0].attributions.has("bold")).toBeTruthy();
    expect(spans[1].insert.length).toBe(4);

    expect(() => text.annotate({ start: 0, end: 100 }, "bold", null)).toThrow();
    expect(() => text.annotate({} as any, "bold", null)).toThrow();
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

describe("get line", () => {
  it("basic", () => {
    const text = new RichText(BigInt(1));
    text.insert(0, "你好，\n世界！");
    expect(text.getLine(0)[0].insert).toBe("你好，\n");
    expect(text.getLine(1)[0].insert).toBe("世界！");
    expect(text.getLine(2).length).toBe(0);
    expect(text.getLine(3).length).toBe(0);
    text.insert(0, "\n");
    expect(text.getLine(0)[0].insert).toBe("\n");
    expect(text.getLine(1)[0].insert).toBe("你好，\n");
    expect(text.getLine(2)[0].insert).toBe("世界！");
    expect(text.getLine(3).length).toBe(0);
    expect(text.getLine(4).length).toBe(0);
  });
});
