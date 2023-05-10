use std::{ops::Deref, panic};

use crdt_richtext::{rich_text::RichText as RichTextInner, Style};
use js_sys::Object;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct RichText {
    inner: RichTextInner,
}

#[wasm_bindgen]
pub enum AnnotateType {
    BoldLike,
    LinkLike,
}

#[wasm_bindgen]
impl RichText {
    #[wasm_bindgen(constructor)]
    pub fn new(id: u64) -> Self {
        Self {
            inner: RichTextInner::new(id),
        }
    }

    pub fn insert(&mut self, index: usize, text: &str) -> Result<(), JsError> {
        if index > self.length() {
            return Err(JsError::new("index out of range"));
        }

        self.inner.insert_utf16(index, text);
        Ok(())
    }

    pub fn delete(&mut self, index: usize, length: usize) -> Result<(), JsError> {
        if index + length > self.length() {
            return Err(JsError::new("index out of range"));
        }

        self.inner.delete_utf16(index..index + length);
        Ok(())
    }

    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.inner.to_string()
    }

    pub fn annotate(
        &mut self,
        index: usize,
        length: usize,
        ann_name: &str,
        ann_type: AnnotateType,
    ) -> Result<(), JsError> {
        if index + length > self.length() {
            return Err(JsError::new("index out of range"));
        }

        let style = match ann_type {
            AnnotateType::BoldLike => Style {
                start_type: crdt_richtext::AnchorType::Before,
                end_type: crdt_richtext::AnchorType::Before,
                behavior: crdt_richtext::Behavior::Merge,
                type_: ann_name.into(),
            },
            AnnotateType::LinkLike => Style {
                start_type: crdt_richtext::AnchorType::Before,
                end_type: crdt_richtext::AnchorType::After,
                behavior: crdt_richtext::Behavior::Merge,
                type_: ann_name.into(),
            },
        };

        self.inner.annotate_utf16(index..index + length, style);
        Ok(())
    }

    #[wasm_bindgen(js_name = "eraseAnn")]
    pub fn erase_ann(
        &mut self,
        index: usize,
        length: usize,
        ann_name: &str,
        ann_type: AnnotateType,
    ) -> Result<(), JsError> {
        if index + length > self.length() {
            return Err(JsError::new("index out of range"));
        }

        let style = match ann_type {
            AnnotateType::BoldLike => Style {
                start_type: crdt_richtext::AnchorType::Before,
                end_type: crdt_richtext::AnchorType::Before,
                behavior: crdt_richtext::Behavior::Delete,
                type_: ann_name.into(),
            },
            AnnotateType::LinkLike => Style {
                start_type: crdt_richtext::AnchorType::Before,
                end_type: crdt_richtext::AnchorType::After,
                behavior: crdt_richtext::Behavior::Delete,
                type_: ann_name.into(),
            },
        };

        self.inner.annotate_utf16(index..index + length, style);
        Ok(())
    }

    /// @returns {{text: string, annotations: Set<string>}[]}
    #[wasm_bindgen(js_name = "getAnnSpans", skip_typescript)]
    pub fn get_ann_spans(&self) -> Vec<Object> {
        let mut ans = Vec::new();
        for span in self.inner.iter() {
            let obj = js_sys::Object::new();
            let set = js_sys::Set::new(&JsValue::undefined());
            for ann in span.annotations {
                set.add(&ann.deref().into());
            }
            js_sys::Reflect::set(&obj, &"text".into(), &span.text.into()).unwrap();
            js_sys::Reflect::set(&obj, &"annotations".into(), &set).unwrap();
            ans.push(obj);
        }

        ans
    }

    pub fn version(&self) -> Vec<u8> {
        self.inner.version().encode()
    }

    pub fn export(&self, version: &[u8]) -> Vec<u8> {
        if version.is_empty() {
            self.inner.export(&Default::default())
        } else {
            let vv = crdt_richtext::VersionVector::decode(version);
            self.inner.export(&vv)
        }
    }

    pub fn import(&mut self, data: &[u8]) {
        self.inner.import(data);
    }

    pub fn length(&self) -> usize {
        self.inner.len_utf16()
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
    panic::set_hook(Box::new(console_error_panic_hook::hook));
}

#[wasm_bindgen(typescript_custom_section)]
const TS_APPEND_CONTENT: &'static str = r#"
export interface RichText {
  getAnnSpans(): {text: string, annotations: Set<string>}[];
}
"#;
