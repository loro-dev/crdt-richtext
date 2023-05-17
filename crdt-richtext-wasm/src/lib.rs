use std::{borrow::Borrow, cell::RefCell, panic};

use crdt_richtext::{
    rich_text::{DeltaItem, IndexType, RichText as RichTextInner},
    Behavior, Style,
};
use serde::{Deserialize, Serialize};
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub struct RichText {
    inner: RefCell<RichTextInner>,
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
        Self {
            inner: RefCell::new(text),
        }
    }

    pub fn id(&self) -> u64 {
        self.inner.borrow().id()
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn observe(&self, f: js_sys::Function) {
        self.inner.borrow_mut().observe(Box::new(move |event| {
            let serializer = serde_wasm_bindgen::Serializer::json_compatible();
            let _ = f.call1(&JsValue::NULL, &event.serialize(&serializer).unwrap());
        }));
    }

    pub fn insert(&self, index: usize, text: &str) -> Result<(), JsError> {
        if index > self.length() {
            return Err(JsError::new("index out of range"));
        }

        self.inner.borrow_mut().insert_utf16(index, text);
        Ok(())
    }

    pub fn delete(&self, index: usize, length: usize) -> Result<(), JsError> {
        if index + length > self.length() {
            return Err(JsError::new("index out of range"));
        }

        self.inner.borrow_mut().delete_utf16(index..index + length);
        Ok(())
    }

    #[allow(clippy::inherent_to_string)]
    #[wasm_bindgen(js_name = "toString")]
    pub fn to_string(&self) -> String {
        self.inner.borrow().to_string()
    }

    #[wasm_bindgen(skip_typescript)]
    pub fn annotate(&self, range: JsValue, ann_name: &str, value: JsValue) -> Result<(), JsError> {
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

        self.inner
            .borrow_mut()
            .annotate_utf16(range.start..range.end, style);
        Ok(())
    }

    #[wasm_bindgen(js_name = "eraseAnn", skip_typescript)]
    pub fn erase_ann(&self, range: JsValue, ann_name: &str) -> Result<(), JsError> {
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

        self.inner
            .borrow_mut()
            .annotate_utf16(range.start..range.end, style);
        Ok(())
    }

    #[wasm_bindgen(js_name = "getAnnSpans", skip_typescript)]
    pub fn get_ann_spans(&self) -> Vec<JsValue> {
        let mut ans = Vec::new();
        for span in self.inner.borrow().iter() {
            let serializer = serde_wasm_bindgen::Serializer::json_compatible();
            ans.push(span.serialize(&serializer).unwrap());
        }

        ans
    }

    #[wasm_bindgen(js_name = "getLine", skip_typescript)]
    pub fn get_line(&self, line: usize) -> Vec<JsValue> {
        let mut ans = Vec::new();
        for span in self.inner.borrow().get_line(line) {
            let serializer = serde_wasm_bindgen::Serializer::json_compatible();
            ans.push(span.serialize(&serializer).unwrap());
        }

        ans
    }

    #[wasm_bindgen(js_name = "sliceString")]
    pub fn slice_str(&self, start: usize, end: usize) -> String {
        self.inner.borrow().slice_str(start..end, IndexType::Utf16)
    }

    #[wasm_bindgen(js_name = "chatAt")]
    pub fn char_at(&self, index: usize) -> String {
        self.inner
            .borrow()
            .slice_str(index..index + 1, IndexType::Utf16)
    }

    pub fn lines(&self) -> usize {
        self.inner.borrow().lines()
    }

    #[wasm_bindgen(js_name = "applyDelta", skip_typescript)]
    pub fn apply_delta(&self, delta: JsValue) -> Result<(), JsError> {
        let delta: Vec<DeltaItem> = serde_wasm_bindgen::from_value(delta)?;

        if delta.is_empty() {
            return Ok(());
        }

        self.inner
            .borrow_mut()
            .apply_delta(delta.into_iter(), IndexType::Utf16);
        Ok(())
    }

    pub fn version(&self) -> Vec<u8> {
        self.inner.borrow().version().encode()
    }

    pub fn export(&self, version: &[u8]) -> Vec<u8> {
        if version.is_empty() {
            self.inner.borrow().export(&Default::default())
        } else {
            let vv = crdt_richtext::VersionVector::decode(version);
            self.inner.borrow().export(&vv)
        }
    }

    pub fn import(&self, data: &[u8]) {
        self.inner.borrow_mut().import(data);
    }

    pub fn length(&self) -> usize {
        self.inner.borrow().len_utf16()
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
    attributes: Record<string, any>,
}

export type DeltaItem = {
    retain?: number,
    insert?: string,
    delete?: number,
    attributes?: Record<string, any>,
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
  applyDelta(delta: DeltaItem[]): void;
}
"#;
