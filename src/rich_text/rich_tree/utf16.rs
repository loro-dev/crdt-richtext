use super::*;

pub struct Utf16LenAndLineBreaks {
    pub utf16: u32,
    pub line_breaks: u32,
}

pub fn get_utf16_len(str: &str) -> usize {
    if str.is_empty() {
        return 0;
    }

    let iter = encode_utf16(str);
    iter.count()
}

#[inline(always)]
pub fn get_utf16_len_and_line_breaks(bytes: &[u8]) -> Utf16LenAndLineBreaks {
    if bytes.is_empty() {
        return Utf16LenAndLineBreaks {
            line_breaks: 0,
            utf16: 0,
        };
    }

    let str = bytes_to_str(bytes);
    let mut iter = encode_utf16(str);
    let mut utf16 = 0;
    for _ in iter.by_ref() {
        utf16 += 1;
    }

    Utf16LenAndLineBreaks {
        utf16,
        line_breaks: iter.line_breaks,
    }
}

pub fn utf16_to_utf8(bytes: &[u8], utf16_index: usize) -> usize {
    if utf16_index == 0 {
        return 0;
    }

    let str = bytes_to_str(bytes);
    let mut iter = encode_utf16(str);
    for _ in 0..utf16_index {
        iter.next();
    }

    iter.visited
}

/// get the index of nth line start in bytes (in utf8)
///
/// if n exceed the number of lines in bytes, return None
pub fn line_start_to_utf8(bytes: &BytesSlice, n: usize) -> Option<usize> {
    if n == 0 {
        return Some(0);
    }

    let str = bytes_to_str(bytes);
    let mut visited_bytes = 0;
    let mut iter_line_breaks = 0;
    for c in str.chars() {
        if c.eq(&'\n') {
            iter_line_breaks += 1;
            if iter_line_breaks == n {
                return Some(visited_bytes + 1);
            }
        }

        visited_bytes += c.len_utf8();
    }

    None
}

#[inline(always)]
pub fn bytes_to_str(bytes: &[u8]) -> &str {
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
        line_breaks: 0,
    }
}

// from std
#[derive(Clone)]
pub struct EncodeUtf16<'a> {
    chars: Chars<'a>,
    extra: u16,
    visited: usize,
    line_breaks: u32,
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
            self.line_breaks += if ch.eq(&'\n') { 1 } else { 0 };
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

#[cfg(test)]
mod test {
    use super::line_start_to_utf8;

    #[test]
    fn line_breaks() {
        use append_only_bytes::AppendOnlyBytes;
        let mut bytes = AppendOnlyBytes::new();
        bytes.push_str("abc\ndragon\nzz");
        assert_eq!(bytes.len(), 13);
        assert_eq!(line_start_to_utf8(&bytes.slice(..), 0).unwrap(), 0);
        assert_eq!(line_start_to_utf8(&bytes.slice(..), 1).unwrap(), 4);
        assert_eq!(line_start_to_utf8(&bytes.slice(..), 2).unwrap(), 11);
        assert!(line_start_to_utf8(&bytes.slice(..), 3).is_none());
        assert_eq!(line_start_to_utf8(&bytes.slice(0..0), 0).unwrap(), 0);
        assert_eq!(line_start_to_utf8(&bytes.slice(0..0), 1), None);
    }
}
