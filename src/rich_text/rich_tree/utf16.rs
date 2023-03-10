use super::*;

#[inline(always)]
pub fn get_utf16_len(bytes: &BytesSlice) -> usize {
    let str = bytes_to_str(bytes);
    let utf16 = encode_utf16(str).count();
    utf16
}

#[inline(always)]
fn bytes_to_str(bytes: &BytesSlice) -> &str {
    // SAFETY: we are sure the range is valid utf8
    let str = std::str::from_utf8(&bytes[..]).unwrap();
    str
}

fn encode_utf16(s: &str) -> EncodeUtf16 {
    EncodeUtf16 {
        chars: s.chars(),
        extra: 0,
    }
}

// from std
#[derive(Clone)]
pub struct EncodeUtf16<'a> {
    pub(super) chars: Chars<'a>,
    pub(super) extra: u16,
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
