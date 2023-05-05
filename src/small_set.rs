use fxhash::FxHashSet;
const STACK_LEN: usize = 4;

/// When the set is small it only live in the stack memory.
///
/// It's used to store the difference of the annotation, which is
/// usually small or empty. It uses positive(or negative) values to represent new
/// insertions(or deletion). 0 is illegal value.
///
/// A absolute value can only exist once in the set.
/// i.e. if 3 is in the set, -3 must not be in the set.
///
/// If 3 is in the set and user insert -3, the insertion will not happen,
/// instead the 3 will be removed from the set.
#[derive(Debug, Clone)]
pub enum SmallSetI32 {
    Stack(([i32; STACK_LEN], u8)),
    Heap(FxHashSet<i32>),
}

impl Default for SmallSetI32 {
    fn default() -> Self {
        Self::new()
    }
}

impl SmallSetI32 {
    const EMPTY_VALUE: i32 = 0;
    pub(crate) fn new() -> Self {
        SmallSetI32::Stack(([Self::EMPTY_VALUE; STACK_LEN], 0))
    }

    pub(crate) fn insert(&mut self, value: i32) {
        assert!(value != 0);
        if self.contains(-value) {
            self.remove(-value);
            return;
        }

        match self {
            SmallSetI32::Stack((stack, size)) => {
                for entry in stack.iter() {
                    if *entry == value {
                        // already exists
                        return;
                    }
                }

                for entry in stack.iter_mut() {
                    if *entry == Self::EMPTY_VALUE {
                        *entry = value;
                        *size += 1;
                        return;
                    }
                }

                let mut set =
                    FxHashSet::with_capacity_and_hasher(STACK_LEN * 2, Default::default());

                for &v in stack.iter() {
                    // we already know it's non empty
                    set.insert(v);
                }
                set.insert(value);
                *self = SmallSetI32::Heap(set);
            }
            SmallSetI32::Heap(set) => {
                set.insert(value);
            }
        }
    }

    pub(crate) fn contains(&self, value: i32) -> bool {
        if value == 0 {
            return false;
        }

        match self {
            SmallSetI32::Stack((stack, size)) => {
                if *size == 0 {
                    return false;
                }

                for entry in stack.iter() {
                    if *entry == value {
                        return true;
                    }
                }

                false
            }
            SmallSetI32::Heap(set) => set.contains(&value),
        }
    }

    pub(crate) fn remove(&mut self, value: i32) {
        match self {
            SmallSetI32::Stack((stack, size)) => {
                if *size == 0 {
                    return;
                }

                for entry in stack.iter_mut() {
                    if *entry == value {
                        *entry = Self::EMPTY_VALUE;
                        *size -= 1;
                    }
                }
            }
            SmallSetI32::Heap(set) => {
                set.remove(&value);
            }
        }
    }

    pub(crate) fn iter(&self) -> SmallSetIter {
        match self {
            SmallSetI32::Stack((stack, _)) => SmallSetIter::Stack(stack.iter()),
            SmallSetI32::Heap(set) => SmallSetIter::Heap(set.iter()),
        }
    }

    pub(crate) fn len(&self) -> usize {
        match self {
            SmallSetI32::Stack((_, size)) => *size as usize,
            SmallSetI32::Heap(set) => set.len(),
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

pub(crate) enum SmallSetIter<'a> {
    Stack(std::slice::Iter<'a, i32>),
    Heap(std::collections::hash_set::Iter<'a, i32>),
}

impl<'a> Iterator for SmallSetIter<'a> {
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            SmallSetIter::Stack(iter) => {
                let mut ans = iter.next();
                while ans == Some(&SmallSetI32::EMPTY_VALUE) {
                    ans = iter.next();
                }
                ans.copied()
            }
            SmallSetIter::Heap(iter) => iter.next().copied(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::SmallSetI32;

    #[test]
    fn test() {
        let mut set = SmallSetI32::new();
        set.insert(1);
        set.insert(2);
        set.insert(2);
        set.insert(2);
        set.insert(1);
        assert_eq!(set.len(), 2);
        assert!(set.contains(2));
        assert!(set.contains(1));
        assert!(!set.contains(0));
        assert!(!set.contains(-2));
        set.remove(2);
        assert!(!set.contains(2));
        assert!(set.len() == 1);
    }

    #[test]
    fn smallset_size() {
        assert_eq!(32, std::mem::size_of::<SmallSetI32>());
    }
}
