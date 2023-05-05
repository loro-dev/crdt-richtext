use crdt_richtext::rich_text::RichText as RichTextInner;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct RichText {
    inner: RichTextInner,
}

#[wasm_bindgen]
impl RichText {
    #[wasm_bindgen(constructor)]
    pub fn new(id: u64) -> Self {
        Self {
            inner: RichTextInner::new(id),
        }
    }

    pub fn insert(&mut self, index: usize, text: &str) {
        self.inner.insert(index, text);
    }

    pub fn delete(&mut self, index: usize, length: usize) {
        self.inner.delete(index..index + length);
    }

    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.inner.to_string()
    }

    pub fn annotate(&mut self, index: usize, length: usize, annotation: &str) {
        // self.inner.annotate(index..index + length, annotation);
    }
}

#[wasm_bindgen(js_name = setPanicHook)]
pub fn set_panic_hook() {
    // When the `console_error_panic_hook` feature is enabled, we can call the
    // `set_panic_hook` function at least once during initialization, and then
    // we will get better error messages if our code ever panics.
    //
    // For more details see
    // https://github.com/rustwasm/console_error_panic_hook#readme
    console_error_panic_hook::set_once();
}

#[wasm_bindgen]
pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
