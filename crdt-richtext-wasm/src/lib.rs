use std::panic;

use crdt_richtext::{
    rich_text::{IndexType, RichText as RichTextInner},
    Behavior, Style,
};
use serde::{Deserialize, Serialize};
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

#[derive(Serialize, Deserialize)]
struct AnnRange {
    start: usize,
    end: usize,
    expand: Option<String>,
    inclusive: Option<bool>,
}

#[wasm_bindgen]
impl RichText {
    #[wasm_bindgen(constructor)]
    pub fn new(id: u64) -> Self {
        let mut text = RichTextInner::new(id);
        text.set_event_index_type(IndexType::Utf16);
        Self { inner: text }
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn observe(&mut self, f: js_sys::Function) {
        self.inner.observe(Box::new(move |event| {
            f.call1(
                &JsValue::NULL,
                &serde_wasm_bindgen::to_value(event).unwrap(),
            )
            .unwrap();
        }));
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

    #[wasm_bindgen(skip_typescript)]
    pub fn annotate(
        &mut self,
        range: JsValue,
        ann_name: &str,
        value: JsValue,
    ) -> Result<(), JsError> {
        let range: AnnRange = serde_wasm_bindgen::from_value(range)?;

        if range.end > self.length() {
            return Err(JsError::new("index out of range"));
        }

        let (start_type, end_type) = match range.expand.as_deref() {
            None => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::Before,
            ),
            Some("none") => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::After,
            ),
            Some("start") => (
                crdt_richtext::AnchorType::After,
                crdt_richtext::AnchorType::After,
            ),
            Some("after") => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::Before,
            ),
            Some("both") => (
                crdt_richtext::AnchorType::After,
                crdt_richtext::AnchorType::Before,
            ),
            _ => return Err(JsError::new("invalid expand value")),
        };

        let inclusive = range.inclusive.unwrap_or(false);
        let value = serde_wasm_bindgen::from_value(value)?;

        let style = Style {
            start_type,
            end_type,
            behavior: if inclusive {
                Behavior::Inclusive
            } else {
                Behavior::Merge
            },
            type_: ann_name.into(),
            value,
        };

        self.inner.annotate_utf16(range.start..range.end, style);
        Ok(())
    }

    #[wasm_bindgen(js_name = "eraseAnn", skip_typescript)]
    pub fn erase_ann(&mut self, range: JsValue, ann_name: &str) -> Result<(), JsError> {
        let range: AnnRange = serde_wasm_bindgen::from_value(range)?;

        if range.end > self.length() {
            return Err(JsError::new("index out of range"));
        }

        let (start_type, end_type) = match range.expand.as_deref() {
            None => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::Before,
            ),
            Some("none") => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::After,
            ),
            Some("start") => (
                crdt_richtext::AnchorType::After,
                crdt_richtext::AnchorType::After,
            ),
            Some("after") => (
                crdt_richtext::AnchorType::Before,
                crdt_richtext::AnchorType::Before,
            ),
            Some("both") => (
                crdt_richtext::AnchorType::After,
                crdt_richtext::AnchorType::Before,
            ),
            _ => return Err(JsError::new("invalid expand value")),
        };

        let style = Style {
            start_type,
            end_type,
            behavior: Behavior::Delete,
            type_: ann_name.into(),
            value: serde_json::Value::Null,
        };

        self.inner.annotate_utf16(range.start..range.end, style);
        Ok(())
    }

    #[wasm_bindgen(js_name = "getAnnSpans", skip_typescript)]
    pub fn get_ann_spans(&self) -> Vec<JsValue> {
        let mut ans = Vec::new();
        for span in self.inner.iter() {
            ans.push(serde_wasm_bindgen::to_value(&span).unwrap());
        }

        ans
    }

    #[wasm_bindgen(js_name = "getLine", skip_typescript)]
    pub fn get_line(&self, line: usize) -> Vec<JsValue> {
        let mut ans = Vec::new();
        for span in self.inner.get_line(line) {
            ans.push(serde_wasm_bindgen::to_value(&span).unwrap());
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
export type AnnRange = {
  expand?: 'before' | 'after' | 'both' | 'none'
  inclusive?: boolean,
  start: number,
  end: number,
}

export interface Span {
    insert: string, 
    attributions: Map<string, any>,
}

export type DeltaItem = {
    retain: number,
    attributes?: Map<string, any>,
} | {
    insert: string,
    attributes?: Map<string, any>,
} | {
    delete: number,
};

export interface Event {
    ops: DeltaItem[], 
    is_local: boolean,
    index_type: "Utf8" | "Utf16",
}

export interface RichText {
  getAnnSpans(): Span[];
  getLine(line: number): Span[];
  annotate(
    range: AnnRange,
    ann_name: string,
    value: null|boolean|number|string|object,
  );
  eraseAnn(
    range: AnnRange,
    ann_name: string,
  );
  observe(cb: (event: Event) => void): void;
}
"#;
