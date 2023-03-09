use crate::{test_utils::AnnotationType, RangeMergeRule};

use super::*;
use arbitrary::Arbitrary;

pub struct Actor {
    text: RichText,
}

#[derive(Arbitrary, Clone, Debug, Copy)]
pub enum Action {
    Insert {
        actor: u8,
        pos: u8,
        content: u16,
    },
    Delete {
        actor: u8,
        pos: u8,
        len: u8,
    },
    Annotate {
        actor: u8,
        pos: u8,
        len: u8,
        annotation: AnnotationType,
    },
    Sync(u8, u8),
}

pub fn preprocess_action(actors: &[Actor], action: &mut Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
        }
        Action::Delete { actor, pos, len } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation: _,
        } => {
            *actor %= actors.len() as u8;
            *pos = (*pos as usize % (actors[*actor as usize].len() + 1)) as u8;
            *len = (*len).min(10);
            *len %= (actors[*actor as usize].len().max(*pos as usize + 1) - *pos as usize)
                .min(255)
                .max(1) as u8;
        }
        Action::Sync(a, b) => {
            *a %= actors.len() as u8;
            *b %= actors.len() as u8;
            if b == a {
                *b = (*a + 1) % actors.len() as u8;
            }
        }
    }
}

pub fn apply_action(actors: &mut [Actor], action: Action) {
    match action {
        Action::Insert {
            actor,
            pos,
            content,
        } => {
            actors[actor as usize].insert(pos as usize, content.to_string().as_str());
            // actors[actor as usize].check();
        }
        Action::Delete { actor, pos, len } => {
            if len == 0 {
                return;
            }

            actors[actor as usize].delete(pos as usize, len as usize);
            // actors[actor as usize].check();
        }
        Action::Annotate {
            actor,
            pos,
            len,
            annotation,
        } => {
            if len == 0 {
                return;
            }

            match annotation {
                AnnotationType::Link => {
                    actors[actor as usize]
                        .annotate(pos as usize..=pos as usize + len as usize - 1, "link");
                }
                AnnotationType::Bold => {
                    actors[actor as usize]
                        .annotate(pos as usize..pos as usize + len as usize, "bold");
                }
                AnnotationType::Comment => {
                    // TODO:
                }
                AnnotationType::UnBold => {
                    actors[actor as usize]
                        .un_annotate(pos as usize..pos as usize + len as usize, "bold");
                }
                AnnotationType::UnLink => {
                    actors[actor as usize]
                        .un_annotate(pos as usize..=pos as usize + len as usize - 1, "link");
                }
            }
            // actors[actor as usize].check();
        }
        Action::Sync(a, b) => {
            let (a, b) = arref::array_mut_ref!(actors, [a as usize, b as usize]);
            a.merge(b);
            // a.check();
        }
    }
}

pub fn fuzzing(actor_num: usize, actions: Vec<Action>) {
    let mut actors = vec![];
    for i in 0..actor_num {
        actors.push(Actor::new(i + 1));
    }

    for mut action in actions {
        preprocess_action(&actors, &mut action);
        // println!("{:?},", &action);
        debug_log::group!("{:?},", &action);
        apply_action(&mut actors, action);
        debug_log::group_end!();
    }

    for i in 0..actors.len() {
        for j in (i + 1)..actors.len() {
            let (a, b) = arref::array_mut_ref!(&mut actors, [i, j]);
            debug_log::group!("merge {i}<-{j}");
            a.merge(b);
            debug_log::group_end!();
            debug_log::group!("merge {i}->{j}");
            b.merge(a);
            debug_log::group_end!();
            assert_eq!(a.text.to_string(), b.text.to_string());
        }
    }
}

impl Actor {
    pub fn new(id: usize) -> Self {
        Self {
            text: RichText::new(id as u64),
        }
    }

    pub fn insert(&mut self, pos: usize, content: &str) {
        self.text.insert(pos, content);
    }

    /// this should happen after the op is integrated to the list crdt
    pub fn delete(&mut self, pos: usize, len: usize) {
        self.text.delete(pos..pos + len)
    }

    #[inline(always)]
    pub fn annotate(&mut self, range: impl RangeBounds<usize>, type_: &str) {}

    #[inline(always)]
    fn un_annotate(&mut self, range: impl RangeBounds<usize>, type_: &str) {}

    fn merge(&mut self, other: &Self) {
        self.text.merge(&other.text)
    }

    #[allow(unused)]
    fn check_eq(&mut self, other: &mut Self) {
        assert_eq!(self.len(), other.len());
        assert_eq!(self.text.to_string(), other.text.to_string());
    }

    #[allow(unused)]
    fn len(&self) -> usize {
        self.text.len()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use Action::*;

    #[test]
    fn fuzz_0() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Insert {
                    actor: 0,
                    pos: 129,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18432,
                },
                Insert {
                    actor: 72,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
            ],
        )
    }

    #[test]
    fn fuzz_1() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 72,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
            ],
        );
    }

    #[test]
    fn fuzz_2() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 18504,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 2,
                },
                Delete {
                    actor: 0,
                    pos: 2,
                    len: 2,
                },
            ],
        );
    }

    #[test]
    fn fuzz_3() {
        fuzzing(
            2,
            vec![
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 5395,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 47394,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
            ],
        );
    }

    #[test]
    fn fuzz_4() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 32,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 8224,
                },
                Insert {
                    actor: 32,
                    pos: 32,
                    content: 18464,
                },
                Insert {
                    actor: 0,
                    pos: 72,
                    content: 18504,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }

    #[test]
    fn fuzz_5() {
        fuzzing(2, vec![]);
    }
}
