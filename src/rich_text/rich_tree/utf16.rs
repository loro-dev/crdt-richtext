use super::*;

#[inline(always)]
pub fn get_utf16_len(bytes: &BytesSlice) -> usize {
    let str = bytes_to_str(bytes);
    let utf16 = encode_utf16(str).count();
    utf16
}

pub fn utf16_to_utf8(bytes: &BytesSlice, utf16_index: usize) -> usize {
    let str = bytes_to_str(bytes);
    let mut iter = encode_utf16(str);
    for _ in 0..utf16_index {
        iter.next();
    }

    iter.visited
}

#[inline(always)]
fn bytes_to_str(bytes: &BytesSlice) -> &str {
    #[allow(unsafe_code)]
    // SAFETY: we are sure the range is valid utf8
    let str = unsafe { std::str::from_utf8_unchecked(&bytes[..]) };
    str
}

fn encode_utf16(s: &str) -> EncodeUtf16 {
    EncodeUtf16 {
        chars: s.chars(),
        extra: 0,
        visited: 0,
    }
}

// from std
#[derive(Clone)]
pub struct EncodeUtf16<'a> {
    chars: Chars<'a>,
    extra: u16,
    visited: usize,
}

impl fmt::Debug for EncodeUtf16<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EncodeUtf16").finish_non_exhaustive()
    }
}

impl<'a> Iterator for EncodeUtf16<'a> {
    type Item = u16;

    #[inline]
    fn next(&mut self) -> Option<u16> {
        if self.extra != 0 {
            let tmp = self.extra;
            self.extra = 0;
            return Some(tmp);
        }

        let mut buf = [0; 2];
        self.chars.next().map(|ch| {
            self.visited += ch.len_utf8();
            let n = ch.encode_utf16(&mut buf).len();
            if n == 2 {
                self.extra = buf[1];
            }
            buf[0]
        })
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let (low, high) = self.chars.size_hint();
        // every char gets either one u16 or two u16,
        // so this iterator is between 1 or 2 times as
        // long as the underlying iterator.
        (low, high.and_then(|n| n.checked_mul(2)))
    }
}
