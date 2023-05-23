use super::*;
use generic_btree::{rle, BTreeTrait};

#[derive(Debug, Clone)]
pub(crate) struct RichTreeTrait;

impl BTreeTrait for RichTreeTrait {
    type Elem = Elem;

    type Cache = Cache;

    type CacheDiff = CacheDiff;

    const MAX_LEN: usize = 16;

    fn calc_cache_internal(
        cache: &mut Self::Cache,
        caches: &[generic_btree::Child<Self>],
        diff: Option<Self::CacheDiff>,
    ) -> Option<Self::CacheDiff> {
        match diff {
            Some(diff) => {
                cache.apply_diff(&diff);
                Some(diff)
            }
            None => {
                let mut len = 0;
                let mut utf16_len = 0;
                let mut line_breaks = 0;
                let mut anchor_set = CacheAnchorSet::default();
                for child in caches.iter() {
                    len += child.cache.len;
                    utf16_len += child.cache.utf16_len;
                    line_breaks += child.cache.line_breaks;
                    anchor_set.union_(&child.cache.anchor_set);
                }

                let anchor_diff = anchor_set.calc_diff(&cache.anchor_set);
                let diff = CacheDiff {
                    anchor_diff,
                    len_diff: len as isize - cache.len as isize,
                    utf16_len_diff: utf16_len as isize - cache.utf16_len as isize,
                    line_break_diff: line_breaks as isize - cache.line_breaks as isize,
                };

                cache.len = len;
                cache.utf16_len = utf16_len;
                cache.line_breaks = line_breaks;
                Some(diff)
            }
        }
    }

    fn calc_cache_leaf(
        cache: &mut Self::Cache,
        caches: &[Self::Elem],
        diff: Option<Self::CacheDiff>,
    ) -> Self::CacheDiff {
        match diff {
            Some(diff) => {
                cache.apply_diff(&diff);
                diff
            }
            None => {
                let mut len = 0;
                let mut utf16_len = 0;
                let mut line_breaks = 0;
                let mut anchor_set = CacheAnchorSet::default();
                for child in caches.iter() {
                    if !child.is_dead() {
                        len += child.string.len();
                        utf16_len += child.utf16_len;
                        line_breaks += child.line_breaks;
                    }
                    anchor_set.union_elem_set(&child.anchor_set);
                }

                let anchor_diff = cache.anchor_set.calc_diff(&anchor_set);
                let diff = CacheDiff {
                    anchor_diff,
                    len_diff: len as isize - cache.len as isize,
                    utf16_len_diff: utf16_len as isize - cache.utf16_len as isize,
                    line_break_diff: line_breaks as isize - cache.line_breaks as isize,
                };
                cache.len = len as u32;
                cache.utf16_len = utf16_len;
                cache.line_breaks = line_breaks;
                diff
            }
        }
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        diff1.anchor_diff.merge(&diff2.anchor_diff);
        diff1.len_diff += diff2.len_diff;
        diff1.utf16_len_diff += diff2.utf16_len_diff;
        diff1.line_break_diff += diff2.line_break_diff;
    }

    fn insert(
        elements: &mut generic_btree::HeapVec<Self::Elem>,
        index: usize,
        offset: usize,
        elem: Self::Elem,
    ) {
        rle::insert_with_split(elements, index, offset, elem)
    }
}
