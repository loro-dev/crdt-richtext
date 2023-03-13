use fxhash::FxHashSet;
use generic_btree::{rle::HasLength, ArenaIndex};

use super::{
    ann::{Span, StyleCalculator},
    RichText,
};

pub struct Iter<'a> {
    text: &'a RichText,
    style_calc: StyleCalculator,
    leaf: ArenaIndex,
    index: usize,
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
            done: false,
        }
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = Span;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let mut leaf = self.text.content.get_node(self.leaf);
        loop {
            while self.index >= leaf.elements().len() {
                // index out of range, find next valid leaf node
                let next = if let Some(next) = self.text.content.next_same_level_node(self.leaf) {
                    next
                } else {
                    self.done = true;
                    return None;
                };
                self.index = 0;
                self.leaf = next;
                leaf = self.text.content.get_node(self.leaf);
            }

            let elements = leaf.elements();
            while self.index < elements.len() && elements[self.index].rle_len() == 0 {
                // skip zero len elements
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
        dbg!(&elem);
        self.style_calc.apply_start(&elem.anchor_set);
        let annotations: FxHashSet<_> = self
            .style_calc
            .iter()
            .map(|&x| self.text.ann.get_ann_by_idx(x).unwrap().type_.clone())
            .collect();
        self.style_calc.apply_end(&elem.anchor_set);
        self.index += 1;
        Some(Span {
            text: elem.string.clone(),
            annotations,
        })
    }
}
