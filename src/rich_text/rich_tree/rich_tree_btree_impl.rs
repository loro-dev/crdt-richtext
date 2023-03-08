use super::*;
use generic_btree::BTreeTrait;

#[derive(Debug, Clone)]
pub(crate) struct RichTreeTrait;

impl BTreeTrait for RichTreeTrait {
    type Elem = Elem;

    type Cache = Cache;

    type CacheDiff = CacheDiff;

    type WriteBuffer = ();

    const MAX_LEN: usize = 12;

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
                cache.len = 0;
                cache.anchor_set.clear();
                for child in caches.iter() {
                    cache.len += child.cache.len;
                    cache.anchor_set.union_(&child.cache.anchor_set);
                }
                None
            }
        }
    }

    fn calc_cache_leaf(cache: &mut Self::Cache, caches: &[Self::Elem]) -> Self::CacheDiff {
        let mut len = 0;
        let mut utf16_len = 0;
        for child in caches.iter() {
            len += child.string.len();
            utf16_len += child.utf16_len;
            cache.anchor_set.process_diff(&child.anchor_set);
        }

        let temp_diff = cache.anchor_set.finish_diff_calc();
        let diff = CacheDiff {
            start: temp_diff.start,
            end: temp_diff.end,
            len_diff: len as isize - cache.len as isize,
            utf16_len_diff: utf16_len as isize - cache.utf16_len as isize,
        };
        cache.len = len;
        cache.utf16_len = utf16_len;
        diff
    }

    fn merge_cache_diff(diff1: &mut Self::CacheDiff, diff2: &Self::CacheDiff) {
        for ann in diff2.start.iter() {
            if diff1.start.contains(-ann) {
                diff1.start.remove(-ann);
            } else {
                diff1.start.insert(ann);
            }
        }
        for ann in diff2.end.iter() {
            if diff1.end.contains(-ann) {
                diff1.end.remove(-ann);
            } else {
                diff1.end.insert(ann);
            }
        }

        diff1.len_diff += diff2.len_diff;
        diff1.utf16_len_diff += diff2.utf16_len_diff;
    }

    fn insert(_: &mut generic_btree::HeapVec<Self::Elem>, _: usize, _: usize, _: Self::Elem) {
        unreachable!()
    }
}
