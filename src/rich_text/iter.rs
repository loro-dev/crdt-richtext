use std::mem::take;

use fxhash::FxHashMap;
use generic_btree::{rle::Mergeable, QueryResult};

use crate::Behavior;

use super::{
    ann::{Span, StyleCalculator},
    RichText,
};

pub struct Iter<'a> {
    text: &'a RichText,
    style_calc: StyleCalculator,
    cursor: QueryResult,
    end: Option<QueryResult>,
    pending_return: Option<Span>,
    done: bool,
}

impl<'a> Iter<'a> {
    pub(crate) fn new(text: &'a RichText) -> Self {
        let leaf = text.content.first_leaf();
        Self {
            style_calc: text.init_styles.clone(),
            text,
            cursor: QueryResult {
                leaf,
                elem_index: 0,
                offset: 0,
                found: true,
            },
            pending_return: None,
            done: false,
            end: None,
        }
    }

    pub(crate) fn new_range(
        text: &'a RichText,
        start: QueryResult,
        end: Option<QueryResult>,
        style: StyleCalculator,
    ) -> Self {
        Self {
            style_calc: style,
            text,
            cursor: start,
            pending_return: None,
            done: false,
            end,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = Span;

    fn next(&mut self) -> Option<Self::Item> {
        let mut pending_return = take(&mut self.pending_return);
        loop {
            if self.done {
                return pending_return;
            }

            let mut leaf = self.text.content.get_node(self.cursor.leaf);
            let mut is_end_leaf = self.end.map_or(false, |end| end.leaf == self.cursor.leaf);
            loop {
                while self.cursor.elem_index >= leaf.elements().len() {
                    if is_end_leaf {
                        self.done = true;
                        return pending_return;
                    }

                    // index out of range, find next valid leaf node
                    let next = if let Some(next) =
                        self.text.content.next_same_level_node(self.cursor.leaf)
                    {
                        next
                    } else {
                        self.done = true;
                        return pending_return;
                    };
                    self.cursor.elem_index = 0;
                    self.cursor.leaf = next;
                    is_end_leaf = self.end.map_or(false, |end| end.leaf == self.cursor.leaf);
                    leaf = self.text.content.get_node(self.cursor.leaf);
                }

                let elements = leaf.elements();

                // skip zero len (deleted) elements
                while self.cursor.elem_index < elements.len()
                    && elements[self.cursor.elem_index].content_len() == 0
                {
                    self.style_calc
                        .apply_start(&elements[self.cursor.elem_index].anchor_set);
                    self.style_calc
                        .apply_end(&elements[self.cursor.elem_index].anchor_set);
                    self.cursor.elem_index += 1;
                }

                if self.cursor.elem_index < elements.len() {
                    break;
                }
            }

            let leaf = leaf;
            let is_end_leaf = is_end_leaf;
            let elem = &leaf.elements()[self.cursor.elem_index];
            let is_end_elem = is_end_leaf
                && self
                    .end
                    .map_or(false, |end| end.elem_index == self.cursor.elem_index);
            self.style_calc.apply_start(&elem.anchor_set);
            let annotations: FxHashMap<_, _> = self
                .style_calc
                .calc_styles(&self.text.ann)
                .filter_map(|x| {
                    if x.behavior == Behavior::Delete {
                        None
                    } else {
                        Some((x.type_.clone(), x.value.clone()))
                    }
                })
                .collect();
            self.style_calc.apply_end(&elem.anchor_set);
            self.cursor.elem_index += 1;
            let ans = Span {
                insert: if is_end_elem {
                    std::str::from_utf8(&elem.string[self.cursor.offset..self.end.unwrap().offset])
                        .unwrap()
                        .to_string()
                } else {
                    std::str::from_utf8(&elem.string[self.cursor.offset..])
                        .unwrap()
                        .to_string()
                },
                attributes: annotations,
            };

            self.cursor.offset = 0;
            if is_end_elem {
                self.done = true;
            }

            if let Some(mut pending) = pending_return {
                if pending.can_merge(&ans) {
                    pending.merge_right(&ans);
                    pending_return = Some(pending);
                    continue;
                }

                self.pending_return = Some(ans);
                return Some(pending);
            } else {
                pending_return = Some(ans);
                continue;
            }
        }
    }
}
