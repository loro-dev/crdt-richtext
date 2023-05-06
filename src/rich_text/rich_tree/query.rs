use generic_btree::{BTreeTrait, FindResult, Query};

use crate::rich_text::{ann::StyleCalculator, rich_tree::utf16::utf16_to_utf8};

use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IndexType {
    Utf8,
    Utf16,
}

struct IndexFinderWithStyles {
    left: usize,
    style_caculator: StyleCalculator,
    index_type: IndexType,
}

pub(crate) struct IndexFinder {
    left: usize,
    index_type: IndexType,
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
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in child_caches.iter().enumerate() {
            if cache.cache.len == 0 {
                continue;
            }

            let cache_len = match self.index_type {
                IndexType::Utf8 => cache.cache.len,
                IndexType::Utf16 => cache.cache.utf16_len,
            };
            // prefer the end of an element
            if self.left >= cache_len {
                last_left = self.left;
                self.left -= cache_len;
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
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            if cache.content_len() == 0 {
                continue;
            }

            let len = match self.index_type {
                IndexType::Utf8 => cache.content_len(),
                IndexType::Utf16 => {
                    if cache.status.is_dead() {
                        0
                    } else {
                        cache.utf16_len
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

        self.left = last_left;
        FindResult::new_missing(
            elements.len() - 1,
            reset_left_to_utf8(last_left, self.index_type, elements.last().unwrap()),
        )
    }
}

type TreeTrait = RichTreeTrait;

impl Query<TreeTrait> for IndexFinderWithStyles {
    type QueryArg = (usize, IndexType);

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinderWithStyles {
            left: target.0,
            style_caculator: StyleCalculator::default(),
            index_type: target.1,
        }
    }

    /// should prefer zero len element
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
            if cache.cache.len == 0 {
                continue;
            }

            let cache_len = match self.index_type {
                IndexType::Utf8 => cache.cache.len,
                IndexType::Utf16 => cache.cache.utf16_len,
            };
            self.style_caculator
                .apply_node_start(&cache.cache.anchor_set);
            if self.left >= cache_len {
                last_left = self.left;
                self.left -= cache_len;
            } else {
                return FindResult::new_found(i, self.left);
            }

            self.style_caculator.apply_node_end(&cache.cache.anchor_set);
        }

        self.left = last_left;
        FindResult::new_missing(child_caches.len() - 1, last_left)
    }

    /// should prefer zero len element
    fn find_element(&mut self, _: &Self::QueryArg, elements: &[Elem]) -> generic_btree::FindResult {
        if elements.is_empty() {
            return FindResult::new_missing(0, self.left);
        }

        let mut last_left = self.left;
        for (i, cache) in elements.iter().enumerate() {
            if cache.content_len() == 0 {
                continue;
            }

            let len = match self.index_type {
                IndexType::Utf8 => cache.content_len(),
                IndexType::Utf16 => {
                    if cache.status.is_dead() {
                        0
                    } else {
                        cache.utf16_len
                    }
                }
            };
            self.style_caculator.apply_start(&cache.anchor_set);
            if self.left >= len {
                last_left = self.left;
                self.left -= len;
            } else {
                return FindResult::new_found(
                    i,
                    reset_left_to_utf8(self.left, self.index_type, cache),
                );
            }

            self.style_caculator.apply_end(&cache.anchor_set);
        }

        self.left = last_left;
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
            assert!(element.utf16_len >= left);
            if element.utf16_len == left {
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
            self.visited_len += cache.cache.len;
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
                return FindResult::new_found(i, cache.cache.len);
            }
            self.visited_len += cache.cache.len;
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
