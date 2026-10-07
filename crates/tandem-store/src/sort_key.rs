//! Fractional-index sort keys: reordering updates one row (ARCHITECTURE §4.1).
//! Keys are non-empty strings over `0-9a-z` with no trailing `0`, compared
//! lexicographically. Port of the well-known midpoint algorithm.

const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

fn digit(c: u8) -> usize {
    DIGITS.iter().position(|d| *d == c).unwrap_or(0)
}

fn midpoint(a: &[u8], b: Option<&[u8]>) -> Vec<u8> {
    if let Some(b) = b {
        let mut n = 0;
        while n < b.len() && a.get(n).copied().unwrap_or(b'0') == b[n] {
            n += 1;
        }
        if n > 0 {
            let mut out = b[..n].to_vec();
            out.extend(midpoint(a.get(n..).unwrap_or(&[]), Some(&b[n..])));
            return out;
        }
    }
    let da = a.first().map(|c| digit(*c)).unwrap_or(0);
    let db = b
        .and_then(|b| b.first())
        .map(|c| digit(*c))
        .unwrap_or(DIGITS.len());
    if db - da > 1 {
        vec![DIGITS[(da + db).div_ceil(2)]]
    } else if let Some(b) = b.filter(|b| b.len() > 1) {
        b[..1].to_vec()
    } else {
        let mut out = vec![DIGITS[da]];
        out.extend(midpoint(a.get(1..).unwrap_or(&[]), None));
        out
    }
}

/// A key strictly between `a` and `b` (`None` means open-ended).
pub fn between(a: Option<&str>, b: Option<&str>) -> String {
    let out = midpoint(a.unwrap_or("").as_bytes(), b.map(str::as_bytes));
    String::from_utf8(out).unwrap_or_default()
}

/// Keys for `n` items in order.
pub fn sequence(n: usize) -> Vec<String> {
    let mut keys: Vec<String> = Vec::with_capacity(n);
    for _ in 0..n {
        let next = between(keys.last().map(String::as_str), None);
        keys.push(next);
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_sort_between_neighbours() {
        let first = between(None, None);
        let after = between(Some(&first), None);
        assert!(after > first);
        let mid = between(Some(&first), Some(&after));
        assert!(first < mid && mid < after, "{first} < {mid} < {after}");
        let before = between(None, Some(&first));
        assert!(before < first);
        let mut lo = first.clone();
        for _ in 0..200 {
            let m = between(Some(&lo), Some(&after));
            assert!(lo < m && m < after, "{lo} < {m} < {after}");
            lo = m;
        }
        let mut hi = first.clone();
        for _ in 0..200 {
            let m = between(None, Some(&hi));
            assert!(m < hi && !m.ends_with('0'), "{m} < {hi}");
            hi = m;
        }
    }

    #[test]
    fn sequence_is_increasing() {
        let keys = sequence(500);
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
    }
}
