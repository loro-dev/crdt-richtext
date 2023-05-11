# crdt-richtext

> Rust implementation of [Peritext](https://www.inkandswitch.com/peritext/) and
> [Fugue](https://arxiv.org/abs/2305.00583)

This crate contains a subset of [Loro CRDT](https://loro.dev/)(which is not yet
open-source)

[**📚 See the blog post**](https://loro-dev.notion.site/crdt-richtext-Rust-implementation-of-Peritext-and-Fugue-c49ef2a411c0404196170ac8daf066c0)

_The interface is not yet stable and is subject to changes. Do not use it in
production._

This Rust crate provides an implementation of Peritext that is optimized for
performance. This crate uses a separate data structure to store the range
annotation, decoupled from the underlying list CRDT. This implementation depends
on `RangeMap` trait, which can be implemented efficiently to make the overall
algorithm fast. But currently, this crate only provides a dumb implementation to
provide a proof of concept.
