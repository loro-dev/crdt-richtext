use std::{
    collections::{BTreeSet, HashMap},
    ops::RangeBounds,
};

use crate::{Anchor, AnchorType};

use super::*;

#[derive(Debug, PartialEq, Eq)]
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
        Some(x) if x.annotations.keys().eq(span.annotations.keys()) => {
            merge_span(x, &span);
        }
        Some(x) if (x.len == 0 && span.len == 0) => {
            for (ann, pos) in span.annotations {
                let origin_pos = x.annotations.entry(ann).or_default();
                origin_pos.merge(&pos);
            }
        }
        _ => arr.push(span),
    }
}

fn insert_span(arr: &mut Vec<Span>, index: usize, span: Span) {
    if index == arr.len() {
        push_span(arr, span);
    } else if arr[index].len == 0 && span.len == 0 {
        for (ann, pos) in span.annotations {
            let origin_pos = arr[index].annotations.entry(ann).or_default();
            origin_pos.merge(&pos);
        }
    } else if arr[index].annotations.keys().eq(span.annotations.keys()) {
        merge_span(&mut arr[index], &span);
    } else {
        arr.insert(index, span);
    }
}

/// a and b have the same annotations
fn merge_span(a: &mut Span, b: &Span) {
    for (a, b) in a.annotations.iter_mut().zip(b.annotations.iter()) {
        a.1.merge(b.1)
    }
    a.len += b.len;
}

fn split_span(span: Span, offset: usize) -> (Span, Span) {
    let mut left = span.clone();
    left.update_pos(None, Some(false));
    left.len = offset;
    let mut right = span;
    right.update_pos(Some(false), None);
    right.len -= offset;
    (left, right)
}

impl DumbRangeMap {
    /// NOTE: it skip Span with zero length:
    /// If you find pos 2 in spans with size of `2, 0, 3`, you will get span with size of 3
    ///
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
            panic!(
                "Index out of bound. Target {char_index}, but the len is {}",
                self.len
            );
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
        let last =
            self.arr
                .iter()
                .rev()
                .position(|x| match x.annotations.iter().find(|x| x.0.id == id) {
                    Some(a) => {
                        annotation = Some(a);
                        true
                    }
                    None => false,
                });

        last.map(|last| (self.arr.len() - last - 1, annotation.unwrap().0.clone()))
    }

    fn find_annotation_first_pos(&self, id: OpID) -> Option<(usize, Arc<Annotation>)> {
        let mut annotation = None;
        let first =
            self.arr
                .iter()
                .position(|x| match x.annotations.iter().find(|x| x.0.id == id) {
                    Some(a) => {
                        annotation = Some(a);
                        true
                    }
                    None => false,
                });

        first.map(|first| (first, annotation.unwrap().0.clone()))
    }

    fn check(&self) {
        assert_eq!(self.len, self.arr.iter().map(|x| x.len).sum());

        let mut last_annotations = BTreeMap::default();
        for i in 0..self.arr.len() {
            let next_annotations = self
                .arr
                .get(i + 1)
                .map(|x| x.annotations.clone())
                .unwrap_or_default();
            let span = &self.arr[i];
            for (ann, pos) in &span.annotations {
                if pos.begin_here {
                    assert!(!last_annotations.contains_key(ann));
                } else {
                    assert!(last_annotations.contains_key(ann));
                }
                if pos.end_here {
                    assert!(!next_annotations.contains_key(ann));
                } else {
                    assert!(next_annotations.contains_key(ann));
                }
            }
            last_annotations = span.annotations.clone();
        }
    }

    fn update_ann_pos(&mut self, range: Range<usize>) {
        for i in range {
            if i >= self.arr.len() {
                continue;
            }
            if i > 0 {
                let (last, this) = arref::array_mut_ref!(&mut self.arr, [i - 1, i]);
                for (ann, pos) in this.annotations.iter_mut() {
                    pos.begin_here = !last.annotations.contains_key(ann);
                }
            } else {
                for (_, pos) in self.arr[i].annotations.iter_mut() {
                    pos.begin_here = true;
                }
            }

            if i != self.arr.len() - 1 {
                let (this, next) = arref::array_mut_ref!(&mut self.arr, [i, i + 1]);
                for (ann, pos) in this.annotations.iter_mut() {
                    pos.end_here = !next.annotations.contains_key(ann);
                }
            } else {
                for (_, pos) in self.arr[i].annotations.iter_mut() {
                    pos.end_here = true;
                }
            }
        }
    }
}

impl RangeMap for DumbRangeMap {
    fn init() -> Self {
        DumbRangeMap {
            arr: Default::default(),
            len: 0,
        }
    }

    fn insert<F>(&mut self, pos: usize, len: usize, mut f: F)
    where
        F: FnMut(&Annotation, AnnPos, RelativeSpanPos) -> AnnPosRelativeToInsert,
    {
        let Position { index, offset } = self.find_pos(pos);
        self.len += len;
        let mut done = false;
        let mut last = None;
        let mut next = None;
        let mut middle = None;

        if offset != 0 {
            self.arr[index].len += len;
            done = true;
        } else if self.arr.is_empty() {
            self.arr.push(Span::new(len));
            done = true;
        } else if index == 0 {
            self.arr[index].len += len;
            done = true;
        } else if self.arr[index - 1].len == 0 {
            // need to decide how to distribute the annotations on span with len of 0
            // need to decide take which annotation from the neighbor spans
            if index == 1 {
                middle = Some(index - 1);
                next = Some(index);
            } else {
                assert!(self.arr[index - 2].len > 0);
                assert!(self.arr[index].len > 0);
                last = Some(index - 2);
                middle = Some(index - 1);
                next = Some(index);
            }
        } else {
            last = Some(index - 1);
            next = Some(index);
        }

        if !done {
            let mut shared: Option<BTreeMap<_, _>> = None;
            for a in last.iter().chain(middle.iter()).chain(last.iter()) {
                match &mut shared {
                    Some(shared) => shared.retain(|x, _| self.arr[*a].annotations.contains_key(x)),
                    None => {
                        shared = Some(self.arr[*a].annotations.clone());
                    }
                }
            }

            let shared = shared.unwrap();
            let mut new_insert_span = Span::new(len);
            for (ann, _) in shared.iter() {
                new_insert_span.annotations.insert(
                    ann.clone(),
                    AnnPos {
                        begin_here: false,
                        end_here: false,
                    },
                );
            }

            let mut next_empty_span = Span::new(0);
            let mut use_next = false;
            // middle
            if let Some(middle) = middle {
                let annotations = std::mem::take(&mut self.arr[middle].annotations);
                for (ann, pos) in annotations {
                    if shared.contains_key(&ann) {
                        continue;
                    }

                    match f(&ann, pos, RelativeSpanPos::Middle) {
                        AnnPosRelativeToInsert::EndBeforeInsert => {
                            self.arr[middle].annotations.insert(ann, pos);
                        }
                        AnnPosRelativeToInsert::StartAfterInsert => {
                            use_next = true;
                            next_empty_span.annotations.insert(ann, pos);
                        }
                        AnnPosRelativeToInsert::IncludeInsert => {
                            self.arr[middle].annotations.insert(
                                ann.clone(),
                                AnnPos {
                                    begin_here: pos.begin_here,
                                    end_here: false,
                                },
                            );
                            new_insert_span.annotations.insert(
                                ann.clone(),
                                AnnPos {
                                    begin_here: false,
                                    end_here: true,
                                },
                            );
                            next_empty_span.annotations.insert(
                                ann,
                                AnnPos {
                                    begin_here: false,
                                    end_here: pos.end_here,
                                },
                            );
                        }
                    }
                }

                if use_next {
                    for (_, pos) in new_insert_span.annotations.iter_mut() {
                        pos.end_here = false;
                    }
                }
            }

            // left
            if let Some(last) = last {
                for (ann, pos) in self.arr[last].annotations.iter_mut() {
                    if shared.contains_key(ann) {
                        continue;
                    }

                    match f(ann, *pos, RelativeSpanPos::Before) {
                        AnnPosRelativeToInsert::EndBeforeInsert => {}
                        AnnPosRelativeToInsert::StartAfterInsert => unreachable!(),
                        AnnPosRelativeToInsert::IncludeInsert => {
                            new_insert_span
                                .annotations
                                .entry(ann.clone())
                                .or_insert(AnnPos {
                                    begin_here: false,
                                    end_here: if use_next { false } else { pos.end_here },
                                });
                            if use_next {
                                next_empty_span
                                    .annotations
                                    .entry(ann.clone())
                                    .or_insert(AnnPos {
                                        begin_here: false,
                                        end_here: pos.end_here,
                                    });
                            }
                            pos.end_here = false;
                        }
                    }
                }
            }

            // right
            if let Some(next) = next {
                for (ann, pos) in self.arr[next].annotations.iter_mut() {
                    if shared.contains_key(ann) {
                        continue;
                    }

                    match f(ann, *pos, RelativeSpanPos::After) {
                        AnnPosRelativeToInsert::EndBeforeInsert => unreachable!(),
                        AnnPosRelativeToInsert::StartAfterInsert => {}
                        AnnPosRelativeToInsert::IncludeInsert => {
                            new_insert_span
                                .annotations
                                .entry(ann.clone())
                                .or_insert(AnnPos {
                                    begin_here: pos.begin_here,
                                    end_here: false,
                                });
                            pos.begin_here = false;
                        }
                    }
                }
            }

            insert_span(&mut self.arr, index, new_insert_span);
            if use_next {
                insert_span(&mut self.arr, index + 1, next_empty_span);
            }

            // TODO: Perf
            let last = last.unwrap_or_else(|| middle.unwrap());
            let next = next.unwrap_or_else(|| middle.unwrap()) + len;
            self.update_ann_pos(last..next + 1);
        }

        self.check();
    }

    fn delete(&mut self, pos: usize, len: usize) {
        self.check();
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

        self.update_ann_pos(start_index.saturating_sub(1)..(start_index + 2).min(self.arr.len()));
        self.len -= len;
        self.check();
    }

    fn annotate(&mut self, pos: usize, len: usize, annotation: Annotation) {
        self.check();
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
                self.arr[start_index].annotations.insert(
                    annotation,
                    AnnPos {
                        end_here: true,
                        begin_here: true,
                    },
                );
            } else {
                let mut splitted: Vec<Span> = vec![];
                let start_len = start_offset;
                let end_len = self.arr[start_index].len - end_offset;
                let left_len = self.arr[start_index].len - end_len - start_len;
                if !clean_start {
                    let mut span = self.arr[start_index].clone();
                    span.len = start_len;
                    span.update_pos(None, Some(false));
                    splitted.push(span);
                }
                let mut span = self.arr[start_index].clone();
                span.update_pos(
                    if clean_start { None } else { Some(false) },
                    if clean_end { None } else { Some(false) },
                );
                span.len = left_len;
                span.annotations.insert(
                    annotation,
                    AnnPos {
                        begin_here: true,
                        end_here: true,
                    },
                );
                splitted.push(span);
                if !clean_end {
                    let mut span = self.arr[start_index].clone();
                    span.len = end_len;
                    span.update_pos(Some(false), None);
                    splitted.push(span);
                }

                self.arr.splice(start_index..start_index + 1, splitted);
            }
        } else {
            if !clean_end {
                let mut span = self.arr[end_index].clone();
                span.update_pos(Some(false), None);
                self.arr[end_index].update_pos(None, Some(false));
                span.len -= end_offset;
                self.arr[end_index].len = end_offset;
                self.arr.insert(end_index + 1, span);
            }

            if !clean_start {
                let mut span = self.arr[start_index].clone();
                span.update_pos(Some(false), None);
                self.arr[start_index].update_pos(None, Some(false));
                span.len -= start_offset;
                self.arr[start_index].len = start_offset;
                self.arr.insert(start_index + 1, span);
                start_index += 1;
                end_index += 1;
            }

            for i in start_index..=end_index {
                self.arr[i].annotations.insert(
                    annotation.clone(),
                    AnnPos {
                        begin_here: i == start_index,
                        end_here: i == end_index,
                    },
                );
            }
        }
        self.check();
    }

    fn delete_annotation(&mut self, id: OpID) {
        for i in 0..self.arr.len() {
            self.arr[i].annotations.retain(|f, _| f.id != id);
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
            start.update_pos(
                if start_offset > 0 { Some(false) } else { None },
                if end_offset != self.arr[end_index].len {
                    Some(false)
                } else {
                    None
                },
            );
        } else {
            start.len -= start_offset;
            start.update_pos(if start_offset > 0 { Some(false) } else { None }, None);
        }

        push_span(&mut ans, start);
        for i in start_index + 1..end_index {
            push_span(&mut ans, self.arr[i].clone());
        }

        if end_index != start_index {
            let mut end = self.arr[end_index].clone();
            end.len = end_offset;
            end.update_pos(
                None,
                if end_offset != self.arr[end_index].len {
                    Some(false)
                } else {
                    None
                },
            );
            push_span(&mut ans, end);
        }

        ans
    }

    fn len(&self) -> usize {
        self.len
    }

    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, Range<usize>)> {
        let mut index = 0;
        let mut start_index = 0;
        let mut end_index = 0;
        let mut found = false;
        let mut ann = None;
        for span in self.arr.iter() {
            if let Some(annotation) = span.annotations.keys().find(|x| x.id == id) {
                if !found {
                    start_index = index;
                    found = true;
                    ann = Some(annotation.clone());
                }
            } else if found {
                end_index = index;
                break;
            }

            index += span.len;
        }

        ann.map(|x| (x, start_index..end_index))
    }

    // TODO: Refactor
    fn adjust_annotation(&mut self, id: OpID, start: Option<isize>, end: Option<isize>) {
        self.check();
        if let Some(end) = end {
            match end.cmp(&0) {
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    // move end forward, expand
                    let (mut index, annotation) = self.find_annotation_last_pos(id).unwrap();
                    let mut left_len = end as usize;
                    self.arr[index]
                        .annotations
                        .get_mut(&annotation)
                        .unwrap()
                        .end_here = false;
                    index += 1;
                    while left_len > 0 {
                        if self.arr[index].len > left_len {
                            let (mut a, b) =
                                split_span(std::mem::take(&mut self.arr[index]), left_len);
                            a.annotations.insert(
                                annotation,
                                AnnPos {
                                    begin_here: false,
                                    end_here: true,
                                },
                            );
                            self.arr[index] = b;
                            insert_span(&mut self.arr, index, a);
                            break;
                        } else {
                            let end_here = left_len == self.arr[index].len;
                            self.arr[index].annotations.insert(
                                annotation.clone(),
                                AnnPos {
                                    begin_here: false,
                                    end_here,
                                },
                            );
                        }

                        left_len -= self.arr[index].len;
                        index += 1;
                    }
                }
                std::cmp::Ordering::Less => {
                    // move end backward, shrink
                    let len = (-end) as usize;
                    if len == 0 {
                        return;
                    }

                    let (mut index, _) = self.find_annotation_last_pos(id).unwrap();
                    let mut left_len = len;
                    while left_len > 0 {
                        if self.arr[index].len > left_len {
                            let len = self.arr[index].len;
                            let (mut a, mut b) =
                                split_span(std::mem::take(&mut self.arr[index]), len - left_len);
                            b.annotations.retain(|f, _| f.id != id);
                            for (ann, pos) in a.annotations.iter_mut() {
                                if ann.id == id {
                                    pos.end_here = true;
                                }
                            }
                            self.arr[index] = b;
                            insert_span(&mut self.arr, index, a);
                            break;
                        } else {
                            self.arr[index].annotations.retain(|f, _| f.id != id);
                            if left_len == self.arr[index].len {
                                if let Some((index, annotation)) =
                                    self.find_annotation_first_pos(id)
                                {
                                    if let Some(span) = self.arr.get_mut(index - 1) {
                                        if let Some(pos) = span.annotations.get_mut(&annotation) {
                                            pos.end_here = true;
                                        }
                                    }
                                }
                            }
                        }

                        left_len -= self.arr[index].len;
                        index -= 1;
                    }
                }
            }
        }
        if let Some(start) = start {
            match start.cmp(&0) {
                std::cmp::Ordering::Equal => {}
                std::cmp::Ordering::Greater => {
                    // move start forward, shrink
                    let (mut index, annotation) = self.find_annotation_first_pos(id).unwrap();
                    let mut left_len = start as usize;
                    while left_len > 0 {
                        if self.arr[index].len > left_len {
                            let (mut a, mut b) =
                                split_span(std::mem::take(&mut self.arr[index]), left_len);
                            a.annotations.retain(|f, _| f.id != id);
                            for (ann, pos) in b.annotations.iter_mut() {
                                if ann.id == id {
                                    pos.begin_here = true;
                                }
                            }
                            self.arr[index] = b;
                            insert_span(&mut self.arr, index, a);
                            break;
                        } else {
                            self.arr[index].annotations.retain(|f, _| f.id != id);
                            if left_len == self.arr[index].len {
                                if let Some(span) = self.arr.get_mut(index + 1) {
                                    if let Some(pos) = span.annotations.get_mut(&annotation) {
                                        pos.begin_here = true;
                                    }
                                }
                            }
                        }

                        left_len -= self.arr[index].len;
                        index += 1;
                    }
                }
                std::cmp::Ordering::Less => {
                    // move start backward, expand
                    let (mut index, annotation) = self.find_annotation_first_pos(id).unwrap();
                    let mut left_len = (-start) as usize;
                    self.arr[index]
                        .annotations
                        .get_mut(&annotation)
                        .unwrap()
                        .begin_here = false;

                    index -= 1;
                    while left_len > 0 {
                        if self.arr[index].len > left_len {
                            let (mut a, b) =
                                split_span(std::mem::take(&mut self.arr[index]), left_len);
                            a.annotations.insert(
                                annotation,
                                AnnPos {
                                    begin_here: true,
                                    end_here: false,
                                },
                            );
                            self.arr[index] = a;
                            insert_span(&mut self.arr, index, b);
                            break;
                        } else {
                            let begin_here = left_len == self.arr[index].len;
                            self.arr[index].annotations.insert(
                                annotation.clone(),
                                AnnPos {
                                    begin_here,
                                    end_here: false,
                                },
                            );
                        }

                        left_len -= self.arr[index].len;
                        index -= 1;
                    }
                }
            }
        }

        self.check();
    }
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
        range: crate::AnchorRange {
            start: Anchor {
                id: Some(id(n)),
                type_: AnchorType::Before,
            },
            end: Anchor {
                id: Some(id(n)),
                type_: AnchorType::Before,
            },
        },
        merge_method: crate::RangeMergeRule::Merge,
        type_: String::new(),
        meta: None,
    }
}

fn make_spans(spans: Vec<(Vec<u64>, usize)>) -> Vec<Span> {
    let mut map = HashMap::new();
    let mut ans = Vec::new();
    for i in 0..spans.len() {
        let (annotations, len) = &spans[i];
        let mut new_annotations = BTreeMap::new();
        for ann in annotations {
            let a = map.entry(*ann).or_insert_with(|| Arc::new(a(*ann))).clone();
            let start = i == 0 || spans[i - 1].0.contains(ann);
            let end = i == spans.len() - 1 || spans[i + 1].0.contains(ann);
            new_annotations.insert(
                a,
                AnnPos {
                    begin_here: start,
                    end_here: end,
                },
            );
        }
        ans.push(Span {
            annotations: new_annotations,
            len: *len,
        });
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
                    .map(|x| x.0.id.client)
                    .collect::<Vec<_>>(),
                *len,
            )
        })
        .collect()
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn test_insert_delete() {
        let mut range_map = DumbRangeMap::init();
        range_map.insert_directly(0, 10);
        range_map.delete(0, 10);
        assert_eq!(range_map.len(), 0);
        assert!(range_map.arr.len() == 1);
        assert!(range_map.arr[0].len == 0);
        range_map.check();
    }

    #[test]
    fn test_annotating() {
        let mut range_map = DumbRangeMap::init();
        range_map.insert_directly(0, 10);
        range_map.annotate(0, 10, a(0));
        assert_eq!(range_map.arr.len(), 1);
        assert_eq!(
            &**range_map.arr[0].annotations.iter().next().unwrap().0,
            &a(0)
        );
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
        range_map.check();
    }

    #[test]
    fn test_annotate_inner() {
        let mut range_map = DumbRangeMap::init();
        range_map.insert_directly(0, 10);
        range_map.annotate(0, 2, a(0));
        assert_eq!(range_map.arr.len(), 2);
        assert_eq!(
            &**range_map.arr[0].annotations.iter().next().unwrap().0,
            &a(0)
        );
        range_map.annotate(6, 4, a(1));
        assert_eq!(range_map.arr.len(), 3);
        assert_eq!(
            &**range_map.arr[0].annotations.iter().next().unwrap().0,
            &a(0)
        );
        assert_eq!(range_map.arr[1].annotations.len(), 0);
        assert_eq!(
            &**range_map.arr[2].annotations.iter().next().unwrap().0,
            &a(1)
        );
    }

    #[test]
    fn test_expand() {
        let mut range_map = DumbRangeMap::init();
        range_map.insert_directly(0, 10);
        range_map.annotate(2, 2, a(0));
        range_map.adjust_annotation(id(0), None, Some(2));
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

        range_map.adjust_annotation(id(0), None, Some(2));
        let spans = range_map.get_annotations(0, 10);
        assert_eq!(
            from_spans(&spans),
            (vec![(vec![], 2), (vec![0, 1], 5), (vec![0], 1), (vec![], 2)])
        );

        range_map.check();
    }
}
