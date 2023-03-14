use crate::{test_utils::AnnotationType};

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
    pub fn annotate(&mut self, _range: impl RangeBounds<usize>, _type_: &str) {}

    #[inline(always)]
    fn un_annotate(&mut self, _range: impl RangeBounds<usize>, _type_: &str) {}

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

    fn check(&self) {
        self.text.check()
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
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 1,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 1,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 3,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 8,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 4,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 0,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 21,
                    content: 5397,
                },
                Insert {
                    actor: 1,
                    pos: 21,
                    content: 65301,
                },
                Sync(1, 0),
                Sync(1, 0),
                Sync(0, 1),
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 4,
                    len: 10,
                },
            ],
        );
    }

    #[test]
    fn fuzz_6() {
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
                    pos: 0,
                    content: 18504,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 20,
                    content: 5140,
                },
                Insert {
                    actor: 20,
                    pos: 255,
                    content: 65535,
                },
                Sync(255, 255),
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Sync(255, 255),
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }

    #[test]
    fn fuzz_7() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 512,
                },
                Delete {
                    actor: 0,
                    pos: 0,
                    len: 1,
                },
                Insert {
                    actor: 0,
                    pos: 1,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 1,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 7,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 3,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Insert {
                    actor: 0,
                    pos: 22,
                    content: 5654,
                },
                Sync(1, 0),
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 31,
                    content: 65535,
                },
                Sync(1, 0),
                Sync(1, 0),
                Sync(1, 0),
                Sync(1, 0),
                Delete {
                    actor: 0,
                    pos: 11,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 21,
                    len: 10,
                },
                Sync(1, 0),
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Sync(0, 1),
                Sync(1, 0),
                Sync(1, 0),
                Delete {
                    actor: 0,
                    pos: 30,
                    len: 10,
                },
                Delete {
                    actor: 0,
                    pos: 8,
                    len: 10,
                },
                Delete {
                    actor: 1,
                    pos: 3,
                    len: 10,
                },
                Sync(1, 0),
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
                    content: 65535,
                },
            ],
        );
    }

    #[test]
    fn fuzz_8() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 179,
                    pos: 72,
                    len: 21,
                },
                Delete {
                    actor: 29,
                    pos: 29,
                    len: 29,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 10786,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 92,
                    pos: 34,
                    content: 8738,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8994,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 7458,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 156,
                    content: 40092,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8739,
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
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 6,
                    pos: 6,
                    content: 1542,
                },
                Insert {
                    actor: 0,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 2304,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 2313,
                },
                Insert {
                    actor: 9,
                    pos: 9,
                    content: 63228,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 42,
                    pos: 34,
                    content: 8738,
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
                    pos: 34,
                    content: 8738,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8739,
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
                Sync(231, 231),
                Sync(231, 231),
                Sync(231, 231),
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
                Insert {
                    actor: 34,
                    pos: 42,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 0,
                    content: 0,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 255,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 42,
                },
                Sync(255, 255),
                Insert {
                    actor: 5,
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
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 16674,
                },
                Insert {
                    actor: 0,
                    pos: 4,
                    content: 512,
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
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 40,
                    pos: 72,
                    len: 72,
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
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 34,
                    pos: 34,
                    len: 34,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 22873,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 22562,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 0,
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
                    pos: 34,
                    content: 2338,
                },
                Insert {
                    actor: 9,
                    pos: 34,
                    content: 8738,
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
                Insert {
                    actor: 34,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 29,
                    pos: 29,
                    content: 7453,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 2,
                    pos: 34,
                    content: 8738,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 4,
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
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 1,
                    pos: 254,
                    len: 72,
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
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 88,
                    pos: 88,
                    len: 88,
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
                    actor: 86,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 0,
                    content: 0,
                },
                Insert {
                    actor: 34,
                    pos: 65,
                    content: 34,
                },
                Insert {
                    actor: 0,
                    pos: 2,
                    content: 18432,
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
                Delete {
                    actor: 72,
                    pos: 72,
                    len: 72,
                },
                Delete {
                    actor: 40,
                    pos: 72,
                    len: 72,
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
                Delete {
                    actor: 72,
                    pos: 89,
                    len: 89,
                },
                Delete {
                    actor: 89,
                    pos: 89,
                    len: 89,
                },
            ],
        );
    }

    #[test]
    fn fuzz_9() {
        fuzzing(
            2,
            vec![
                Delete {
                    actor: 184,
                    pos: 183,
                    len: 183,
                },
                Insert {
                    actor: 2,
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
                    content: 0,
                },
                Insert {
                    actor: 0,
                    pos: 255,
                    content: 65535,
                },
                Sync(255, 255),
                Delete {
                    actor: 255,
                    pos: 255,
                    len: 255,
                },
            ],
        );
    }
}
