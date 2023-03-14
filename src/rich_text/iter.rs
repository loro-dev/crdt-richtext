use std::mem::{take};

use fxhash::FxHashSet;
use generic_btree::{rle::Mergeable, ArenaIndex};

use crate::Behavior;

use super::{
    ann::{Span, StyleCalculator},
    RichText,
};

pub struct Iter<'a> {
    text: &'a RichText,
    style_calc: StyleCalculator,
    leaf: ArenaIndex,
    index: usize,
    pending_return: Option<Span>,
    done: bool,
}

impl<'a> Iter<'a> {
    pub(crate) fn new(text: &'a RichText) -> Self {
        let leaf = text.content.first_leaf();
        Self {
            style_calc: text.init_styles.clone(),
            text,
            leaf,
            index: 0,
            pending_return: None,
            done: false,
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

            let mut leaf = self.text.content.get_node(self.leaf);
            loop {
                while self.index >= leaf.elements().len() {
                    // index out of range, find next valid leaf node
                    let next = if let Some(next) = self.text.content.next_same_level_node(self.leaf)
                    {
                        next
                    } else {
                        self.done = true;
                        return pending_return;
                    };
                    self.index = 0;
                    self.leaf = next;
                    leaf = self.text.content.get_node(self.leaf);
                }

                let elements = leaf.elements();

                // skip zero len (deleted) elements
                while self.index < elements.len() && elements[self.index].content_len() == 0 {
                    self.style_calc
                        .apply_start(&elements[self.index].anchor_set);
                    self.style_calc.apply_end(&elements[self.index].anchor_set);
                    self.index += 1;
                }

                if self.index < elements.len() {
                    break;
                }
            }

            let elem = &leaf.elements()[self.index];
            self.style_calc.apply_start(&elem.anchor_set);
            let annotations: FxHashSet<_> = self
                .style_calc
                .calc_styles(&self.text.ann)
                .into_iter()
                .filter_map(|x| {
                    if x.behavior == Behavior::Delete {
                        None
                    } else {
                        Some(x.type_.clone())
                    }
                })
                .collect();
            self.style_calc.apply_end(&elem.anchor_set);
            self.index += 1;
            let ans = Span {
                text: std::str::from_utf8(&elem.string).unwrap().to_string(),
                annotations,
            };

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
