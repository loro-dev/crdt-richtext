<script setup lang="ts">
  import { onMounted, onUnmounted, ref } from "vue";
  import Quill from "quill";
  import "quill/dist/quill.core.css";
  import "quill/dist/quill.bubble.css";
  import "quill/dist/quill.snow.css";
  import { QuillBinding } from "./binding";
  import { RichText, setPanicHook } from "crdt-richtext-wasm";

  setPanicHook();

  const editor1 = ref<null | HTMLDivElement>(null);
  const editor2 = ref<null | HTMLDivElement>(null);
  let bind1: QuillBinding | null = null;
  let bind2: QuillBinding | null = null;
  onMounted(() => {
    const quill1 = new Quill(editor1.value!, {
      modules: {
        toolbar: [
          [
            {
              header: [1, 2, 3, 4, false],
            },
          ],
          ["bold", "italic", "underline", "link"],
        ],
      },
      //theme: 'bubble',
      theme: "snow",
      formats: ["bold", "underline", "header", "italic", "link"],
      placeholder: "Type something in here!",
    });
    const text1 = new RichText(BigInt(1));
    const text2 = new RichText(BigInt(2));
    bind1 = new QuillBinding(text1, quill1);
    text2.observe((e) => {
      if (e.is_local) {
        setTimeout(() => {
          const v = text1.version();
          text1.import(text2.export(v));
        });
      }
    });
    text1.observe((e) => {
      if (e.is_local) {
        setTimeout(() => {
          const v = text2.version();
          text2.import(text1.export(v));
        });
      }
    });
    const quill2 = new Quill(editor2.value!, {
      modules: {
        toolbar: [
          [
            {
              header: [1, 2, 3, 4, false],
            },
          ],
          ["bold", "italic", "underline", "link"],
        ],
      },
      //theme: 'bubble',
      theme: "snow",
      formats: ["bold", "underline", "header", "italic", "link"],
      placeholder: "Type something in here!",
    });
    bind2 = new QuillBinding(text2, quill2);
  });

  onUnmounted(() => {
    bind1?.destroy();
    bind2?.destroy();
  });
</script>

<template>
  <div class="editor">
    <div ref="editor1" />
  </div>
  <div class="editor">
    <div ref="editor2" />
  </div>
</template>

<style scoped>
  .editor {
    margin: 2em;
  }
</style>
