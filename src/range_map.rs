use std::{collections::BTreeSet, sync::Arc};

use crate::{Annotation, OpID};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub annotations: BTreeSet<Arc<Annotation>>,
    pub len: usize,
}

impl Span {
    pub fn new(len: usize) -> Self {
        Span {
            annotations: BTreeSet::new(),
            len,
        }
    }
}

pub trait RangeMap {
    fn init() -> Self;
    fn insert(&mut self, pos: usize, len: usize);
    fn delete(&mut self, pos: usize, len: usize);
    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation);
    fn expand_annotation(&mut self, id: OpID, len: usize);
    fn shrink_annotation(&mut self, id: OpID, len: usize);
    fn delete_annotation(&mut self, id: OpID);
    fn get_annotations(&self, pos: usize, len: usize) -> Vec<Span>;
    fn len(&self) -> usize;
}

#[cfg(feature = "test")]
pub mod dumb {
    use super::*;

    #[derive(Debug)]
    pub struct DumbRangeMap {
        arr: Vec<Span>,
        len: usize,
    }

    pub struct Position {
        pub index: usize,
        pub offset: usize,
    }

    fn push_span(arr: &mut Vec<Span>, span: Span) {
        match arr.last_mut() {
            Some(x) if x.annotations == span.annotations => {
                x.len += span.len;
            }
            _ => arr.push(span),
        }
    }

    fn insert_span(arr: &mut Vec<Span>, index: usize, span: Span) {
        if index == arr.len() {
            push_span(arr, span);
        } else if arr[index].annotations == span.annotations {
            arr[index].len += span.len;
        } else {
            arr.insert(index, span);
        }
    }

    fn split_span(span: Span, offset: usize) -> (Span, Span) {
        let mut left = span.clone();
        left.len = offset;
        let mut right = span;
        right.len -= offset;
        (left, right)
    }

    impl DumbRangeMap {
        /// return: (index, offset)
        pub fn find_pos(&self, char_index: usize) -> Position {
            if self.arr.is_empty() && char_index == 0 {
                return Position {
                    index: 0,
                    offset: 0,
                };
            }
            let mut index = 0;
            for i in 0..self.arr.len() {
                let len = self.arr[i].len;
                if index + len > char_index {
                    return Position {
                        index: i,
                        offset: char_index - index,
                    };
                }

                index += len;
            }

            if index == char_index {
                let last_index = self.arr.len() - 1;
                Position {
                    index: last_index,
                    offset: self.arr[last_index].len,
                }
            } else {
                panic!("Index out of bound");
            }
        }

        fn try_merge_empty_spans(&mut self, start_index: usize) {
            let mut empty_start = 0;
            let mut empty_len = 0;
            for i in start_index.saturating_sub(1)..self.arr.len() {
                if self.arr[i].len == 0 {
                    if empty_len == 0 {
                        empty_len = 1;
                        empty_start = i;
                    } else {
                        empty_len += 1;
                    }
                }
            }

            if empty_len > 1 {
                let mut annotations = std::mem::take(&mut self.arr[empty_start].annotations);
                for mut item in self.arr.drain(empty_start + 1..empty_start + empty_len) {
                    annotations.append(&mut item.annotations);
                }

                self.arr[empty_start].annotations = annotations;
            }
        }

        fn find_annotation_last_pos(&self, id: OpID) -> Option<(usize, Arc<Annotation>)> {
            let mut annotation = None;
            let last = self.arr.iter().rev().position(|x| {
                match x.annotations.iter().find(|x| x.id == id) {
                    Some(a) => {
                        annotation = Some(a.clone());
                        true
                    }
                    None => false,
                }
            });

            last.map(|last| (self.arr.len() - last - 1, annotation.unwrap()))
        }
    }

    impl RangeMap for DumbRangeMap {
        fn init() -> Self {
            DumbRangeMap {
                arr: Default::default(),
                len: 0,
            }
        }

        fn insert(&mut self, pos: usize, len: usize) {
            let Position { index, offset } = self.find_pos(pos);
            if offset == 0 {
                insert_span(&mut self.arr, index, Span::new(len));
            } else {
                self.arr[index].len += len;
            }
            self.len += len;
        }

        fn delete(&mut self, pos: usize, len: usize) {
            let Position {
                mut index,
                mut offset,
            } = self.find_pos(pos);

            let start_index = index;
            let mut left_len = len;
            let mut to_empty = false;
            while left_len > 0 {
                if self.arr[index].len >= left_len + offset {
                    self.arr[index].len -= left_len;
                    break;
                } else {
                    left_len -= self.arr[index].len - offset;
                    self.arr[index].len = offset;
                }

                if self.arr[index].len == 0 {
                    to_empty = true;
                }

                offset = 0;
                index += 1;
            }

            if to_empty {
                self.try_merge_empty_spans(start_index);
            }

            self.len -= len;
        }

        fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
            let Position {
                index: mut start_index,
                offset: start_offset,
            } = self.find_pos(pos);
            let Position {
                index: mut end_index,
                offset: mut end_offset,
            } = self.find_pos(pos + len);
            let clean_start = start_offset == 0;
            if end_offset == 0 && len > 0 {
                end_index -= 1;
                end_offset = self.arr[end_index].len;
            }

            let annotation = Arc::new(annotation);
            let clean_end = end_offset == self.arr[end_index].len;
            if start_index == end_index {
                if clean_start && clean_end {
                    self.arr[start_index].annotations.insert(annotation);
                } else {
                    let mut splitted: Vec<Span> = vec![];
                    let start_len = start_offset;
                    let end_len = self.arr[start_index].len - end_offset;
                    let left_len = self.arr[start_index].len - end_len - start_len;
                    if !clean_start {
                        let mut span = self.arr[start_index].clone();
                        span.len = start_len;
                        splitted.push(span);
                    }
                    let mut span = self.arr[start_index].clone();
                    span.len = left_len;
                    span.annotations.insert(annotation);
                    splitted.push(span);
                    if !clean_end {
                        let mut span = self.arr[start_index].clone();
                        span.len = end_len;
                        splitted.push(span);
                    }

                    self.arr.splice(start_index..start_index + 1, splitted);
                }
            } else {
                if !clean_end {
                    let mut span = self.arr[end_index].clone();
                    span.len -= end_offset;
                    self.arr[end_index].len = end_offset;
                    self.arr.insert(end_index + 1, span);
                }

                if !clean_start {
                    let mut span = self.arr[start_index].clone();
                    span.len -= start_offset;
                    self.arr[start_index].len = start_offset;
                    self.arr.insert(start_index + 1, span);
                    start_index += 1;
                    end_index += 1;
                }

                for i in start_index..=end_index {
                    self.arr[i].annotations.insert(annotation.clone());
                }
            }
        }

        fn expand_annotation(&mut self, id: OpID, len: usize) {
            let (mut index, annotation) = self.find_annotation_last_pos(id).unwrap();
            let mut left_len = len;
            index += 1;
            while left_len > 0 {
                if self.arr[index].len > left_len {
                    let (mut a, b) = split_span(std::mem::take(&mut self.arr[index]), left_len);
                    a.annotations.insert(annotation);
                    self.arr[index] = a;
                    insert_span(&mut self.arr, index + 1, b);
                    break;
                } else {
                    self.arr[index].annotations.insert(annotation.clone());
                }

                left_len -= self.arr[index].len;
                index += 1;
            }
        }

        fn shrink_annotation(&mut self, id: OpID, len: usize) {
            let (mut index, _) = self.find_annotation_last_pos(id).unwrap();
            let mut left_len = len;
            while left_len > 0 {
                if self.arr[index].len > left_len {
                    let len = self.arr[index].len;
                    let (a, mut b) =
                        split_span(std::mem::take(&mut self.arr[index]), len - left_len);
                    b.annotations.retain(|f| f.id != id);
                    self.arr[index] = b;
                    insert_span(&mut self.arr, index, a);
                    break;
                } else {
                    self.arr[index].annotations.retain(|f| f.id != id);
                }

                left_len -= self.arr[index].len;
                index -= 1;
            }
        }

        fn delete_annotation(&mut self, id: OpID) {
            for i in 0..self.arr.len() {
                self.arr[i].annotations.retain(|f| f.id != id);
            }
        }

        fn get_annotations(&self, pos: usize, len: usize) -> Vec<Span> {
            if len == 0 {
                return vec![];
            }

            let Position {
                index: start_index,
                offset: start_offset,
            } = self.find_pos(pos);
            let Position {
                index: mut end_index,
                offset: mut end_offset,
            } = self.find_pos(pos + len);
            let mut ans = Vec::with_capacity(end_index - start_index + 1);
            let mut start = self.arr[start_index].clone();
            if end_offset == 0 && len > 0 {
                end_index -= 1;
                end_offset = self.arr[end_index].len;
            }

            if start_index == end_index {
                start.len = end_offset - start_offset;
            } else {
                start.len -= start_offset;
            }

            push_span(&mut ans, start);
            for i in start_index + 1..end_index {
                push_span(&mut ans, self.arr[i].clone());
            }

            if end_index != start_index {
                let mut end = self.arr[end_index].clone();
                end.len = end_offset;
                push_span(&mut ans, end);
            }

            ans
        }

        fn len(&self) -> usize {
            self.len
        }
    }

    #[cfg(test)]
    mod test {
        use std::collections::HashMap;

        use crate::{Anchor, AnchorType};

        use super::*;
        fn check(r: &DumbRangeMap) {
            assert_eq!(r.len, r.arr.iter().map(|x| x.len).sum())
        }

        fn id(k: u64) -> OpID {
            OpID {
                client: k,
                counter: 0,
            }
        }

        fn a(n: u64) -> Annotation {
            Annotation {
                id: id(n),
                lamport: 0,
                start: Anchor {
                    id: Some(id(n)),
                    type_: AnchorType::Before,
                },
                end: Anchor {
                    id: Some(id(n)),
                    type_: AnchorType::Before,
                },
                merge_method: crate::RangeMergeRule::Merge,
                type_: String::new(),
                meta: None,
            }
        }

        fn make_spans(spans: Vec<(Vec<u64>, usize)>) -> Vec<Span> {
            let mut map = HashMap::new();
            let mut ans = Vec::new();
            for (annotations, len) in spans {
                let annotations = annotations
                    .into_iter()
                    .map(|x| {
                        let a = map.entry(x).or_insert_with(|| Arc::new(a(x))).clone();
                        a
                    })
                    .collect();
                ans.push(Span { annotations, len });
            }

            ans
        }

        fn from_spans(spans: &[Span]) -> Vec<(Vec<u64>, usize)> {
            spans
                .into_iter()
                .map(|Span { annotations, len }| {
                    (
                        annotations
                            .into_iter()
                            .map(|x| x.id.client)
                            .collect::<Vec<_>>(),
                        *len,
                    )
                })
                .collect()
        }

        #[test]
        fn test_insert_delete() {
            let mut range_map = DumbRangeMap::init();
            range_map.insert(0, 10);
            range_map.delete(0, 10);
            assert_eq!(range_map.len(), 0);
            assert!(range_map.arr.len() == 1);
            assert!(range_map.arr[0].len == 0);
            check(&range_map);
        }

        #[test]
        fn test_annotating() {
            let mut range_map = DumbRangeMap::init();
            range_map.insert(0, 10);
            range_map.annotate(0, 10, a(0));
            assert_eq!(range_map.arr.len(), 1);
            assert_eq!(&**range_map.arr[0].annotations.first().unwrap(), &a(0));
            // 0..2..4..10
            //  1  2  1
            range_map.annotate(2, 2, a(1));
            assert_eq!(range_map.arr.len(), 3);
            assert_eq!(range_map.arr[0].annotations.len(), 1);
            assert_eq!(range_map.arr[1].annotations.len(), 2);
            assert_eq!(range_map.arr[2].annotations.len(), 1);
            // 0..1..2..4..8..10
            //  1  2  3  2  1
            range_map.annotate(1, 7, a(2));
            assert_eq!(range_map.arr.len(), 5);
            assert_eq!(range_map.arr[0].annotations.len(), 1);
            assert_eq!(range_map.arr[1].annotations.len(), 2);
            assert_eq!(range_map.arr[2].annotations.len(), 3);
            assert_eq!(range_map.arr[3].annotations.len(), 2);
            assert_eq!(range_map.arr[4].annotations.len(), 1);
            check(&range_map);
        }

        #[test]
        fn test_annotate_inner() {
            let mut range_map = DumbRangeMap::init();
            range_map.insert(0, 10);
            range_map.annotate(0, 2, a(0));
            assert_eq!(range_map.arr.len(), 2);
            assert_eq!(&**range_map.arr[0].annotations.first().unwrap(), &a(0));
            range_map.annotate(6, 4, a(1));
            assert_eq!(range_map.arr.len(), 3);
            assert_eq!(&**range_map.arr[0].annotations.first().unwrap(), &a(0));
            assert_eq!(range_map.arr[1].annotations.len(), 0);
            assert_eq!(&**range_map.arr[2].annotations.first().unwrap(), &a(1));
        }

        #[test]
        fn test_expand() {
            let mut range_map = DumbRangeMap::init();
            range_map.insert(0, 10);
            range_map.annotate(2, 2, a(0));
            range_map.expand_annotation(id(0), 2);
            let spans = range_map.get_annotations(0, 10);
            assert_eq!(
                from_spans(&spans),
                (vec![(vec![], 2), (vec![0], 4), (vec![], 4)])
            );
            // 0..2..6..7..10
            //  0  2  1  0
            range_map.annotate(2, 5, a(1));
            let spans = range_map.get_annotations(0, 10);
            assert_eq!(
                from_spans(&spans),
                (vec![(vec![], 2), (vec![0, 1], 4), (vec![1], 1), (vec![], 3)])
            );

            range_map.expand_annotation(id(0), 2);
            let spans = range_map.get_annotations(0, 10);
            assert_eq!(
                from_spans(&spans),
                (vec![(vec![], 2), (vec![0, 1], 5), (vec![0], 1), (vec![], 2)])
            );

            check(&range_map);
        }
    }
}
