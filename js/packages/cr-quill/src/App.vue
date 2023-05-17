<script setup lang="ts">
  import { onMounted, onUnmounted, reactive, ref, watch } from "vue";
  import Quill from "quill";
  import "quill/dist/quill.core.css";
  import "quill/dist/quill.bubble.css";
  import "quill/dist/quill.snow.css";
  import { QuillBinding } from "./binding";
  import { RichText, setPanicHook } from "crdt-richtext-wasm";

  setPanicHook();

  const editor1 = ref<null | HTMLDivElement>(null);
  const editor2 = ref<null | HTMLDivElement>(null);
  const editor3 = ref<null | HTMLDivElement>(null);
  const editor4 = ref<null | HTMLDivElement>(null);
  const binds: QuillBinding[] = [];
  const texts: RichText[] = [];
  const editors = [editor1, editor2, editor3, editor4];
  const editorVersions = reactive(["", "", "", ""]);
  const online = reactive([true, true, true, true]);
  onMounted(() => {
    let index = 0;
    for (const editor of editors) {
      const text = new RichText(BigInt(index));
      texts.push(text);
      const quill = new Quill(editor.value!, {
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
      binds.push(new QuillBinding(text, quill));
      const this_index = index;

      const sync = () => {
        if (!online[this_index]) {
          return;
        }

        for (let i = 0; i < texts.length; i++) {
          if (i === this_index || !online[i]) {
            continue;
          }

          texts[i].import(text.export(texts[i].version()));
          text.import(texts[i].export(text.version()));
        }
      };

      text.observe((e) => {
        if (e.is_local) {
          Promise.resolve().then(sync);
        }
        Promise.resolve().then(() => {
          const versionStr = JSON.stringify(text.versionDebugMap(), null, 2);
          editorVersions[this_index] = versionStr;
        });
      });

      watch(
        () => online[this_index],
        (isOnline) => {
          if (isOnline) {
            sync();
          }
        }
      );

      index += 1;
    }
  });

  onUnmounted(() => {
    binds.forEach((x) => x.destroy());
  });
</script>

<template>
  <div class="parent">
    <div class="editor">
      <button
        @click="
          () => {
            online[0] = !online[0];
          }
        "
      >
        online: {{ online[0] }}
      </button>
      <div class="version">version: {{ editorVersions[0] }}</div>
      <div ref="editor1" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[1] = !online[1];
          }
        "
      >
        online: {{ online[1] }}
      </button>
      <div class="version">version: {{ editorVersions[1] }}</div>
      <div ref="editor2" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[2] = !online[2];
          }
        "
      >
        online: {{ online[2] }}
      </button>
      <div class="version">version: {{ editorVersions[2] }}</div>
      <div ref="editor3" />
    </div>
    <div class="editor">
      <button
        @click="
          () => {
            online[2] = !online[2];
          }
        "
      >
        online: {{ online[2] }}
      </button>
      <div class="version">version: {{ editorVersions[3] }}</div>
      <div ref="editor4" />
    </div>
  </div>
</template>

<style scoped>
  .editor {
    margin: 3em 0em;
    width: 400px;
    max-height: 800px;
  }

  button {
    color: #565656;
    padding: 0.3em 0.6em;
    margin-bottom: 0.4em;
    background-color: #eee;
  }

  /**matrix 2x2 */
  .parent {
    display: grid;
    grid-template-columns: 1fr 1fr;
    grid-template-rows: 1fr 1fr;
    gap: 3em;
  }

  .version {
    color: grey;
    font-size: 0.8em;
  }
</style>
