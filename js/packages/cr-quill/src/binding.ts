import { RichText } from "crdt-richtext-wasm";
import Quill, { Delta as DeltaType, DeltaStatic, Sources } from "quill";

const Delta = Quill.import("delta");

export class QuillBinding {
  constructor(public richtext: RichText, public quill: Quill) {
    this.quill = quill;
    richtext.observe((event) => {
      // Promise.resolve().then(() => {
      //   let delta: DeltaType = new Delta(
      //     richtext.getAnnSpans(),
      //   );
      //   quill.setContents(
      //     delta,
      //     "this" as any,
      //   );
      // });
      setTimeout(() => {
        if (!event.is_local) {
          const eventDelta = event.ops;
          // We always explicitly set attributes, otherwise concurrent edits may
          // result in quill assuming that a text insertion shall inherit existing
          // attributes.
          const delta = [];
          for (let i = 0; i < eventDelta.length; i++) {
            const d = eventDelta[i];
            if (d.insert !== undefined) {
              delta.push(
                Object.assign({}, d, {
                  attributes: Object.assign(
                    {},
                    d.attributes || {},
                  ),
                }),
              );
            } else {
              delta.push(d);
            }
          }
          quill.updateContents(new Delta(delta), "this" as any);
        }
      });
    });
    quill.on("editor-change", this._quillObserver);
    // This indirectly initializes _negatedUsedFormats.
    // Make sure that this call this after the _quillObserver is set.
    quill.setContents(
      new Delta(
        richtext.getAnnSpans().map((x) => ({
          insert: x.insert,
          attributions: x.attributions,
        })),
      ),
      "this" as any,
    );
  }

  _quillObserver: (
    name: "text-change",
    delta: DeltaStatic,
    oldContents: DeltaStatic,
    source: Sources,
  ) => any = (eventType, delta, state, origin) => {
    if (delta && delta.ops) {
      // update content
      const ops = delta.ops;
      if (origin !== "this" as any) {
        this.richtext.applyDelta(ops);
        console.log(
          "CHECK_MATCH",
          this.richtext.id(),
          this.richtext.getAnnSpans(),
          this.quill.getContents(),
        );
        console.log("SIZE", this.richtext.export(new Uint8Array()).length);
      }
    }
  };
  destroy() {
    // TODO: unobserve
    this.quill.off("editor-change", this._quillObserver);
  }
}
