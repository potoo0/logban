use std::borrow::{Borrow, Cow};
use std::collections::HashMap;

use regex::Regex;

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

/// Expand variables in the template string using the provided variables map.
/// Variables can be in the form of `$var` or `${var}`.
/// If a variable is not found in the map, it remains unchanged in the output.
pub fn expand_template<'a, K, V>(template: &'a str, vars: &HashMap<K, V>) -> Cow<'a, str>
where
    K: Borrow<str> + Eq + std::hash::Hash,
    V: AsRef<str>,
{
    let re = Regex::new(r"\$(\w+)|\$\{(\w+)}").unwrap();

    re.replace_all(template, |caps: &regex::Captures| {
        let key = caps.get(1).or(caps.get(2)).unwrap().as_str();
        match vars.get(key) {
            Some(val) => Cow::Borrowed(val.as_ref()),
            None => Cow::Owned(caps[0].to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_template() {
        let cases = vec![
            "nft add element inet logban banned_ips '{ $ip timeout $timeout }'; echo $date",
            "nft add element inet logban banned_ips '{ ${ip} timeout ${timeout} }'; echo $date",
        ];

        let mut vars = HashMap::new();
        vars.insert("ip", "1.2.3.4");
        vars.insert("timeout", "1h");

        for template in cases {
            let expanded = expand_template(template, &vars);
            assert_eq!(
                expanded,
                "nft add element inet logban banned_ips '{ 1.2.3.4 timeout 1h }'; echo $date"
            );
        }
    }
}
