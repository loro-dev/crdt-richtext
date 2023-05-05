use super::*;

#[derive(Debug, PartialEq, Eq)]
pub struct DumbRangeMap {
    arr: Vec<Span>,
    ann: BTreeSet<Arc<Annotation>>,
    deleted: BTreeSet<OpID>,
    len: usize,
}

pub struct Position {
    pub index: usize,
    pub offset: usize,
}

fn push_span(arr: &mut Vec<Span>, span: Span) {
    match arr.last_mut() {
        Some(x) if x.annotations.iter().eq(span.annotations.iter()) => {
            merge_span(x, &span);
        }
        Some(x) if (x.len == 0 && span.len == 0) => {
            for ann in span.annotations {
                x.annotations.insert(ann);
            }
        }
        _ => arr.push(span),
    }
}

/// a and b have the same annotations
fn merge_span(a: &mut Span, b: &Span) {
    a.len += b.len;
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

    fn try_merge_empty_spans(&mut self, start_index: usize, max_len: Option<usize>) {
        let end = (max_len.unwrap_or(self.arr.len()) + start_index).min(self.arr.len());
        let mut empty_start = 0;
        let mut empty_len = 0;
        for i in start_index.saturating_sub(1)..end {
            if self.arr[i].len == 0 {
                if empty_len == 0 {
                    empty_len = 1;
                    empty_start = i;
                } else {
                    empty_len += 1;
                }
            } else if empty_len > 0 {
                if empty_len > 1 {
                    break;
                } else {
                    empty_len = 0;
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

    fn check(&self) {
        assert_eq!(self.len, self.arr.iter().map(|x| x.len).sum::<usize>());

        for i in 0..self.arr.len() {
            if self.arr[i].len == 0 {
                if i > 0 {
                    assert!(self.arr[i - 1].len > 0);
                }
                if i < self.arr.len() - 1 {
                    assert!(self.arr[i + 1].len > 0);
                }
            }
        }

        for i in 1..self.arr.len() - 1 {
            let last = &self.arr[i - 1].annotations;
            let next = &self.arr[i + 1].annotations;
            let cur = &self.arr[i].annotations;
            for ann in last.iter() {
                if !cur.contains(ann) {
                    assert!(!next.contains(ann));
                }
            }
            for ann in next.iter() {
                if !cur.contains(ann) {
                    assert!(!last.contains(ann));
                }
            }
        }

        let mut all_annotations = BTreeSet::new();
        for span in self.arr.iter() {
            for ann in span.annotations.iter() {
                all_annotations.insert(ann.id);
                assert!(!self.deleted.contains(&ann.id));
            }
        }

        let mut end_ann = BTreeSet::new();
        for i in 1..self.arr.len() {
            let last = &self.arr[i - 1].annotations;
            let cur = &self.arr[i].annotations;
            for ann in last.iter() {
                assert!(!end_ann.contains(&ann.id));
                if !cur.contains(ann) {
                    end_ann.insert(ann.id);
                }
            }
        }

        for ann in self.ann.iter() {
            assert!(all_annotations.contains(&ann.id) || self.deleted.contains(&ann.id));
        }
    }

    fn _replace(&mut self, ann: Arc<Annotation>, new_ann: Arc<Annotation>) {
        for span in self.arr.iter_mut() {
            if span.annotations.remove(&ann) {
                span.annotations.insert(new_ann.clone());
            }
        }
    }

    fn _delete(&mut self, ann: &Arc<Annotation>) {
        for span in self.arr.iter_mut() {
            span.annotations.remove(ann);
        }
    }
}

impl RangeMap for DumbRangeMap {
    fn init() -> Self {
        DumbRangeMap {
            arr: Default::default(),
            ann: BTreeSet::new(),
            deleted: Default::default(),
            len: 0,
        }
    }

    fn insert<F>(&mut self, pos: usize, len: usize, mut f: F)
    where
        F: FnMut(&Annotation) -> AnnPosRelativeToInsert,
    {
        debug_log::debug_dbg!("BEFORE INSERT", &self);
        let Position { index, offset } = self.find_pos(pos);
        self.len += len;
        let mut done = false;
        let mut last = None;
        let mut next = None;
        let mut middle = None;

        if self.arr.is_empty() {
            self.arr.push(Span::new(len));
            done = true;
        } else if offset != 0 || index == 0 {
            self.arr[index].len += len;
            done = true;
        } else if self.arr[index - 1].len == 0 {
            // need to decide how to distribute the annotations on span with len of 0
            // need to decide take which annotation from the neighbor spans
            if index == 1 {
                assert!(self.arr[index - 1].len == 0);
                assert!(self.arr[index].len > 0);
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
            assert!(self.arr[index - 1].len > 0);
            assert!(self.arr[index].len > 0);
            last = Some(index - 1);
            next = Some(index);
        }

        if !done {
            let mut shared: Option<BTreeSet<_>> = None;
            for a in last.iter().chain(middle.iter()).chain(next.iter()) {
                match &mut shared {
                    Some(shared) => shared.retain(|x| self.arr[*a].annotations.contains(x)),
                    None => {
                        shared = Some(self.arr[*a].annotations.clone());
                    }
                }
            }

            let shared = shared.unwrap();
            let mut new_insert_span = Span::new(len);
            let mut next_empty_span = Span::new(0);
            new_insert_span.annotations = shared.clone();
            next_empty_span.annotations = shared.clone();
            let mut middle_annotations = BTreeSet::new();

            let mut use_next = false;
            // middle
            if let Some(middle) = middle {
                for ann in std::mem::take(&mut self.arr[middle].annotations) {
                    if shared.contains(&ann) {
                        middle_annotations.insert(ann);
                        continue;
                    }

                    match f(&ann) {
                        AnnPosRelativeToInsert::Before => {
                            middle_annotations.insert(ann);
                        }
                        AnnPosRelativeToInsert::After => {
                            use_next = true;
                            next_empty_span.annotations.insert(ann);
                        }
                        AnnPosRelativeToInsert::IncludeInsert => {
                            middle_annotations.insert(ann.clone());
                            new_insert_span.annotations.insert(ann.clone());
                            next_empty_span.annotations.insert(ann);
                        }
                    }
                }
            }

            // left
            let use_next = use_next; // make it immutable
            if let Some(last) = last {
                for ann in self.arr[last].annotations.iter() {
                    if shared.contains(ann) {
                        continue;
                    }

                    match f(ann) {
                        AnnPosRelativeToInsert::Before => {}
                        AnnPosRelativeToInsert::After => unreachable!(),
                        AnnPosRelativeToInsert::IncludeInsert => {
                            middle_annotations.insert(ann.clone());
                            new_insert_span.annotations.insert(ann.clone());
                            if use_next {
                                debug_log::debug_log!("next from left {:?}", &ann);
                                next_empty_span.annotations.insert(ann.clone());
                            }
                        }
                    }
                }
            }

            // right
            if let Some(next) = next {
                for ann in self.arr[next].annotations.iter() {
                    if shared.contains(ann) {
                        continue;
                    }

                    match f(ann) {
                        AnnPosRelativeToInsert::Before => unreachable!(),
                        AnnPosRelativeToInsert::After => {}
                        AnnPosRelativeToInsert::IncludeInsert => {
                            middle_annotations.insert(ann.clone());
                            new_insert_span.annotations.insert(ann.clone());
                            if use_next {
                                debug_log::debug_log!("next from right {:?}", &ann);
                                next_empty_span.annotations.insert(ann.clone());
                            }
                        }
                    }
                }
            }

            if let Some(middle) = middle {
                self.arr[middle].annotations = middle_annotations;
            }

            debug_log::debug_log!("new_insert_span {index} {:?}", &new_insert_span);
            self.arr.insert(index, new_insert_span);
            if use_next {
                debug_log::debug_log!("use_next {} {:?}", index + 1, &next_empty_span);
                self.arr.insert(index + 1, next_empty_span);
            }

            if index > 0 {
                self.try_merge_empty_spans(index - 1, None);
            } else {
                self.try_merge_empty_spans(index, None);
            }
        }

        debug_log::debug_dbg!("AFTER INSERT", &self);
        self.check();
    }

    fn delete(&mut self, pos: usize, len: usize) {
        self.check();
        debug_log::debug_dbg!("BEFORE DELETE", &self.arr);
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
                if self.arr[index].len == 0 {
                    to_empty = true;
                }

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

        self.len -= len;
        if to_empty {
            self.try_merge_empty_spans(start_index, Some(len + 3));
        }

        debug_log::debug_dbg!("AFTER DELETE", &self.arr);
        self.check();
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
        self.ann.insert(annotation.clone());
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
                push_span(&mut splitted, span);
                if !clean_end {
                    let mut span = self.arr[start_index].clone();
                    span.len = end_len;
                    push_span(&mut splitted, span);
                }

                self.arr.splice(start_index..start_index + 1, splitted);
                self.try_merge_empty_spans(start_index, Some(5));
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
        self.check();
    }

    fn delete_annotation(&mut self, id: OpID) {
        for i in 0..self.arr.len() {
            self.arr[i].annotations.retain(|f| f.id != id);
        }
    }

    fn get_annotations(&mut self, pos: usize, len: usize) -> Vec<Span> {
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

    fn get_annotation_pos(&self, id: OpID) -> Option<(Arc<Annotation>, Range<usize>)> {
        let mut index = 0;
        let mut start_index = 0;
        let mut end_index = 0;
        let mut found = false;
        let mut ann = None;
        for span in self.arr.iter() {
            if let Some(annotation) = span.annotations.iter().find(|x| x.id == id) {
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

    fn adjust_annotation(
        &mut self,
        id: OpID,
        lamport: Lamport,
        patch_id: OpID,
        start: Option<(isize, Option<OpID>)>,
        end: Option<(isize, Option<OpID>)>,
    ) {
        self.check();
        let (ann, pos) = self.get_annotation_pos(id).unwrap();
        let mut end_pos = pos.end as isize;
        let mut start_pos = pos.start as isize;
        let mut new_ann = (*ann).clone();
        let deleted = start.map(|x| x.1.is_none()).unwrap_or(false)
            && end.map(|x| x.1.is_none()).unwrap_or(false);
        if let Some((end, new_end_id)) = end {
            if ann.range_lamport < (lamport, patch_id) {
                new_ann.range.end.id = new_end_id;
                new_ann.range_lamport = (lamport, patch_id);
                end_pos += end;
            }
        }

        if let Some((start, new_start_id)) = start {
            if ann.range_lamport < (lamport, patch_id) {
                new_ann.range.start.id = new_start_id;
                new_ann.range_lamport = (lamport, patch_id);
                start_pos += start;
            }
        }

        self._delete(&ann);
        if deleted {
            self.deleted.insert(ann.id);
        } else {
            self.annotate(start_pos as usize, (end_pos - start_pos) as usize, new_ann);
        }

        self.check();
    }
}

#[cfg(test)]
mod test {
    use crate::{Anchor, AnchorType, InternalString};

    fn id(k: u64) -> OpID {
        OpID::new(k, 0)
    }

    fn a(n: u64) -> Annotation {
        Annotation {
            id: id(n),
            range_lamport: (0, id(n)),
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
            behavior: crate::Behavior::Merge,
            type_: InternalString::from(""),
            meta: None,
        }
    }

    fn from_spans(spans: &[Span]) -> Vec<(Vec<u64>, usize)> {
        spans
            .iter()
            .map(|Span { annotations, len }| {
                (
                    annotations.iter().map(|x| x.id.client).collect::<Vec<_>>(),
                    *len,
                )
            })
            .collect()
    }

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
            &**range_map.arr[0].annotations.iter().next().unwrap(),
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
            &**range_map.arr[0].annotations.iter().next().unwrap(),
            &a(0)
        );
        range_map.annotate(6, 4, a(1));
        assert_eq!(range_map.arr.len(), 3);
        assert_eq!(
            &**range_map.arr[0].annotations.iter().next().unwrap(),
            &a(0)
        );
        assert_eq!(range_map.arr[1].annotations.len(), 0);
        assert_eq!(
            &**range_map.arr[2].annotations.iter().next().unwrap(),
            &a(1)
        );
    }

    #[test]
    fn test_expand() {
        let mut range_map = DumbRangeMap::init();
        range_map.insert_directly(0, 10);
        range_map.annotate(2, 2, a(1));
        range_map.adjust_annotation(id(1), 2, id(1), None, Some((2, None)));
        let spans = range_map.get_annotations(0, 10);
        assert_eq!(
            from_spans(&spans),
            (vec![(vec![], 2), (vec![1], 4), (vec![], 4)])
        );
        // 0..2..6..7..10
        //  0  2  1  0
        range_map.annotate(2, 5, a(2));
        let spans = range_map.get_annotations(0, 10);
        assert_eq!(
            from_spans(&spans),
            (vec![(vec![], 2), (vec![1, 2], 4), (vec![2], 1), (vec![], 3)])
        );

        range_map.adjust_annotation(id(1), 3, id(1), None, Some((2, None)));
        let spans = range_map.get_annotations(0, 10);
        assert_eq!(
            from_spans(&spans),
            (vec![(vec![], 2), (vec![1, 2], 5), (vec![1], 1), (vec![], 2)])
        );

        range_map.check();
    }
}
