use generic_btree::{BTreeTrait, FindResult, Query};
use serde::{Deserialize, Serialize};

use crate::rich_text::{
    ann::StyleCalculator,
    rich_tree::utf16::{line_start_to_utf8, utf16_to_utf8},
};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexType {
    Utf8,
    Utf16,
}

pub(crate) struct IndexFinderWithStyles {
    left: usize,
    pub(crate) style_calculator: StyleCalculator,
    index_type: IndexType,
}

pub(crate) struct IndexFinder {
    left: usize,
    index_type: IndexType,
}

pub(crate) struct LineStartFinder {
    left: usize,
    pub(crate) style_calculator: StyleCalculator,
}

struct AnnotationFinderStart {
    target: AnnIdx,
    visited_len: usize,
}

struct AnnotationFinderEnd {
    target: AnnIdx,
    visited_len: usize,
}

impl Query<RichTreeTrait> for IndexFinder {
    type QueryArg = (usize, IndexType);

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinder {
            left: target.0,
            index_type: target.1,
        }
    }

    /// should prefer zero len element
    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<RichTreeTrait>],
    ) -> generic_btree::FindResult {
        if child_caches.is_empty() {
            return FindResult::new_missing(0, 0);
        }

        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            let cache_len = match self.index_type {
                IndexType::Utf8 => cache.cache.len,
                IndexType::Utf16 => cache.cache.utf16_len,
            };
            // prefer the end of an element
            if self.left >= cache_len as usize {
                last_left = self.left;
                self.left -= cache_len as usize;
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    /// should prefer zero len element
    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if elements.is_empty() {
            return FindResult::new_missing(0, 0);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            let len = match self.index_type {
                IndexType::Utf8 => cache.content_len(),
                IndexType::Utf16 => {
                    if cache.status.is_dead() {
                        0
                    } else {
                        cache.utf16_len as usize
                    }
                }
            };
            // prefer the end of an element
            if self.left >= len {
                // use content len here, because we need to skip deleted/future spans
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(
                    i,
                    reset_left_to_utf8(self.left, self.index_type, cache),
                );
            }
        }

        FindResult::new_missing(
            elements.len() - 1,
            reset_left_to_utf8(last_left, self.index_type, elements.last().unwrap()),
        )
    }
}

impl Query<RichTreeTrait> for LineStartFinder {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        LineStartFinder {
            left: *target,
            style_calculator: StyleCalculator::default(),
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<RichTreeTrait>],
    ) -> generic_btree::FindResult {
        if self.left == 0 {
            return FindResult::new_found(0, 0);
        }

        if child_caches.is_empty() {
            return FindResult::new_missing(0, 0);
        }

        for (i, cache) in child_caches.iter().enumerate() {
            self.style_calculator
                .apply_node_start(&cache.cache.anchor_set);
            if self.left > cache.cache.line_breaks as usize {
                self.left -= cache.cache.line_breaks as usize;
            } else {
                return FindResult::new_found(i, self.left);
            }
            self.style_calculator
                .apply_node_end(&cache.cache.anchor_set);
        }

        FindResult::new_missing(child_caches.len() - 1, self.left)
    }

    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if self.left == 0 {
            return FindResult::new_found(0, 0);
        }

        if elements.is_empty() {
            return FindResult::new_missing(0, 0);
        }

        for (i, cache) in elements.iter().enumerate() {
            self.style_calculator.apply_start(&cache.anchor_set);
            if cache.is_dead() {
                self.style_calculator.apply_end(&cache.anchor_set);
                continue;
            }

            if self.left > cache.line_breaks as usize {
                self.left -= cache.line_breaks as usize;
            } else {
                return FindResult::new_found(
                    i,
                    line_start_to_utf8(&cache.string, self.left).unwrap(),
                );
            }
            self.style_calculator.apply_end(&cache.anchor_set);
        }

        FindResult::new_missing(elements.len() - 1, elements.last().unwrap().atom_len())
    }
}

type TreeTrait = RichTreeTrait;

impl Query<TreeTrait> for IndexFinderWithStyles {
    type QueryArg = (usize, IndexType);

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinderWithStyles {
            left: target.0,
            style_calculator: StyleCalculator::default(),
            index_type: target.1,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> generic_btree::FindResult {
        if child_caches.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            let cache_len = match self.index_type {
                IndexType::Utf8 => cache.cache.len,
                IndexType::Utf16 => cache.cache.utf16_len,
            };
            if self.left >= cache_len as usize {
                last_left = self.left;
                self.left -= cache_len as usize;
            } else {
                return FindResult::new_found(i, self.left);
            }

            self.style_calculator
                .apply_node_start(&cache.cache.anchor_set);
            self.style_calculator
                .apply_node_end(&cache.cache.anchor_set);
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if elements.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            let len = match self.index_type {
                IndexType::Utf8 => cache.content_len(),
                IndexType::Utf16 => {
                    if cache.status.is_dead() {
                        0
                    } else {
                        cache.utf16_len as usize
                    }
                }
            };
            self.style_calculator.apply_start(&cache.anchor_set);
            self.style_calculator.cache_end(&cache.anchor_set);
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(
                    i,
                    reset_left_to_utf8(self.left, self.index_type, cache),
                );
            }

            self.style_calculator.commit_cache();
        }

        FindResult::new_missing(
            elements.len() - 1,
            reset_left_to_utf8(last_left, self.index_type, elements.last().unwrap()),
        )
    }
}

fn reset_left_to_utf8(left: usize, index_type: IndexType, element: &Elem) -> usize {
    if left == 0 {
        return left;
    }

    match index_type {
        IndexType::Utf8 => left,
        IndexType::Utf16 => {
            assert!(element.utf16_len as usize >= left);
            if element.utf16_len as usize == left {
                return element.atom_len();
            }

            utf16_to_utf8(&element.string, left)
        }
    }
}

impl Query<TreeTrait> for AnnotationFinderStart {
    type QueryArg = AnnIdx;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.anchor_set.contains_start(self.target) {
                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.cache.len as usize;
        }

        FindResult::new_missing(0, 0)
    }

    fn find_element(
        &mut self,
        _: &Self::QueryArg,
        elements: &[<TreeTrait as BTreeTrait>::Elem],
    ) -> FindResult {
        for (i, cache) in elements.iter().enumerate() {
            let (contains_start, inclusive) = cache.anchor_set.contains_start(self.target);
            if contains_start {
                if !inclusive {
                    self.visited_len += cache.content_len();
                }

                return FindResult::new_found(i, 0);
            }
            self.visited_len += cache.content_len();
        }

        FindResult::new_missing(0, 0)
    }
}

impl Query<TreeTrait> for AnnotationFinderEnd {
    type QueryArg = AnnIdx;

    fn init(target: &Self::QueryArg) -> Self {
        Self {
            target: *target,
            visited_len: 0,
        }
    }

    fn find_node(
        &mut self,
        _: &Self::QueryArg,
        child_caches: &[generic_btree::Child<TreeTrait>],
    ) -> FindResult {
        for (i, cache) in child_caches.iter().enumerate().rev() {
            if cache.cache.anchor_set.contains_end(self.target) {
                return FindResult::new_found(i, cache.cache.len as usize);
            }
            self.visited_len += cache.cache.len as usize;
        }

        FindResult::new_missing(0, 0)
    }

    fn find_element(
        &mut self,
        _: &Self::QueryArg,
        elements: &[<TreeTrait as BTreeTrait>::Elem],
    ) -> FindResult {
        for (i, cache) in elements.iter().enumerate().rev() {
            let (contains_end, inclusive) = cache.anchor_set.contains_end(self.target);
            if contains_end {
                if !inclusive {
                    self.visited_len += cache.content_len();
                }

                return FindResult::new_found(i, cache.content_len());
            }
            self.visited_len += cache.content_len();
        }

        FindResult::new_missing(0, 0)
    }
}
