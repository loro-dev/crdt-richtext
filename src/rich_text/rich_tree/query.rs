use generic_btree::{BTreeTrait, FindResult, Query};

use crate::rich_text::ann::StyleCalculator;

use super::*;

struct IndexFinderWithStyles {
    left: usize,
    style_caculator: StyleCalculator,
}

pub(crate) struct IndexFinder {
    left: usize,
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
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinder { left: *target }
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

            // prefer the end of an element
            if self.left >= cache.cache.len {
                last_left = self.left;
                self.left -= cache.cache.len;
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

            // prefer the end of an element
            if self.left >= cache.content_len() {
                // use content len here, because we need to skip deleted/future spans
                last_left = self.left;
                self.left -= cache.content_len();
            } else {
                return FindResult::new_found(i, self.left);
            }
        }

        self.left = last_left;
        FindResult::new_missing(elements.len() - 1, last_left)
    }
}

type TreeTrait = RichTreeTrait;

impl Query<TreeTrait> for IndexFinderWithStyles {
    type QueryArg = usize;

    fn init(target: &Self::QueryArg) -> Self {
        IndexFinderWithStyles {
            left: *target,
            style_caculator: StyleCalculator::default(),
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

            self.style_caculator
                .apply_node_start(&cache.cache.anchor_set);
            if self.left >= cache.cache.len {
                last_left = self.left;
                self.left -= cache.cache.len;
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

            self.style_caculator.apply_start(&cache.anchor_set);
            if self.left >= cache.content_len() {
                last_left = self.left;
                self.left -= cache.content_len();
            } else {
                return FindResult::new_found(i, self.left);
            }

            self.style_caculator.apply_end(&cache.anchor_set);
        }

        self.left = last_left;
        FindResult::new_missing(elements.len() - 1, last_left)
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
