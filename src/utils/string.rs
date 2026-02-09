use std::borrow::Cow;

/// Return a UTF-8 safe tail of `s`, limited to `limit` bytes.
/// If truncated, the result is prefixed with an ellipsis (`…`).
pub fn truncate_tail(s: &str, limit: usize) -> Cow<'_, str> {
    if s.len() <= limit {
        return Cow::Borrowed(s);
    }

    if limit == 0 {
        return Cow::Borrowed("");
    }

    let mut start = s.len();
    for (i, _) in s.char_indices().rev() {
        if s.len() - i > limit.saturating_sub(1) {
            break;
        }
        start = i;
    }

    Cow::Owned(format!("…{}", &s[start..]))
}
