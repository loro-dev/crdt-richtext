# crdt-richtext

> Rust implementation of [Peritext](https://www.inkandswitch.com/peritext/) and
> [Fugue](https://arxiv.org/abs/2305.00583)

This crate contains a subset of [Loro CRDT](https://loro.dev/)(which is not yet
open-source)

[**📚 See the blog post**](https://loro-dev.notion.site/crdt-richtext-Rust-implementation-of-Peritext-and-Fugue-c49ef2a411c0404196170ac8daf066c0)

[**🎨 Try online Demo**](https://crdt-richtext-quill-demo.vercel.app/)

_The interface is not yet stable and is subject to changes. Do not use it in
production._

This CRDT lib combines [Peritext](https://inkandswitch.com/peritext) and
[Fugue](https://arxiv.org/abs/2305.00583)'s power, delivering impressive
performance specifically tailored for rich text. It leverages the
[generic-btree](https://github.com/loro-dev/generic-btree) library to boost
speed, and the [serde-columnar](https://github.com/loro-dev/columnar) simplifies
the implementation of efficient columnar encoding.

## Benchmark

The benchmark was conducted on a 2020 M1 MacBook Pro 13-inch on 2023-05-11.

The complete benchmark result and code is available
[here](https://github.com/zxch3n/fugue-bench/blob/main/results_table.md).

| N=6000                                                           | crdt-richtext-wasm     | loro-wasm               | automerge-wasm      | tree-fugue                  | yjs                          | ywasm               |
| ---------------------------------------------------------------- | ---------------------- | ----------------------- | ------------------- | --------------------------- | ---------------------------- | ------------------- |
| [B4] Apply real-world editing dataset (time)                     | 176 +/- 10 ms          | 141 +/- 15 ms           | 821 +/- 7 ms        | 721 +/- 15 ms               | 1,114 +/- 33 ms              | 23,419 +/- 102 ms   |
| [B4] Apply real-world editing dataset (memUsed)                  | skipped                | skipped                 | skipped             | 2,373,909 +/- 13725 bytes   | 3,480,708 +/- 168887 bytes   | skipped             |
| [B4] Apply real-world editing dataset (encodeTime)               | 8 +/- 1 ms             | 8 +/- 1 ms              | 115 +/- 2 ms        | 12 +/- 0 ms                 | 12 +/- 1 ms                  | 6 +/- 1 ms          |
| [B4] Apply real-world editing dataset (docSize)                  | 127,639 +/- 0 bytes    | 255,603 +/- 8 bytes     | 129,093 +/- 0 bytes | 167,873 +/- 0 bytes         | 159,929 +/- 0 bytes          | 159,929 +/- 0 bytes |
| [B4] Apply real-world editing dataset (parseTime)                | 11 +/- 0 ms            | 2 +/- 0 ms              | 620 +/- 5 ms        | 8 +/- 0 ms                  | 43 +/- 3 ms                  | 40 +/- 3 ms         |
| [B4x100] Apply real-world editing dataset 100 times (time)       | 15,324 +/- 3188 ms     | 12,436 +/- 444 ms       | skipped             | 91,902 +/- 863 ms           | 112,563 +/- 3861 ms          | skipped             |
| [B4x100] Apply real-world editing dataset 100 times (memUsed)    | skipped                | skipped                 | skipped             | 224076566 +/- 2812359 bytes | 318807378 +/- 15737245 bytes | skipped             |
| [B4x100] Apply real-world editing dataset 100 times (encodeTime) | 769 +/- 37 ms          | 780 +/- 32 ms           | skipped             | 943 +/- 52 ms               | 297 +/- 16 ms                | skipped             |
| [B4x100] Apply real-world editing dataset 100 times (docSize)    | 12,667,753 +/- 0 bytes | 26,634,606 +/- 80 bytes | skipped             | 17,844,936 +/- 0 bytes      | 15,989,245 +/- 0 bytes       | skipped             |
| [B4x100] Apply real-world editing dataset 100 times (parseTime)  | 1,252 +/- 14 ms        | 170 +/- 15 ms           | skipped             | 368 +/- 13 ms               | 1,335 +/- 238 ms             | skipped             |

- The benchmark for Automerge is based on `automerge-wasm`, which is not the
  latest version of Automerge 2.0.
- `crdt-richtext` and `fugue` are special-purpose CRDTs that tend to be faster
  and have a smaller encoding size.
- The encoding of `yjs`, `ywasm`, and `loro-wasm` still contains redundancy that
  can be compressed significantly. For more details, see
  [the full report](https://loro.dev/docs/performance/docsize).
- loro-wasm and fugue only support plain text for now
