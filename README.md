# Rust implementation of [Peritext](https://www.inkandswitch.com/peritext/)

This Rust crate provides an implementation of Peritext that is optimized for
performance. This crate uses a separate data structure to store the range
annotation, decoupled from the underlying list CRDT. This implementation depends
on `RangeMap` trait, which can be implemented efficiently to make the overall
algorithm fast. But currently, this crate only provides a dumb implementation to
provide a proof of concept.