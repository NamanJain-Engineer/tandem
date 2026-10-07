//! Lane slugs (F-CD-15, ARCHITECTURE §7 step 1).

use std::collections::HashSet;

pub const MAX_SLUG: usize = 40;

/// Kebab-case, ASCII only, at most 40 characters. Accents fold to their base
/// letter; anything else that isn't a letter or digit becomes a separator.
/// Empty input gives "lane".
pub fn slugify(text: &str) -> String {
    let mut out = String::with_capacity(text.len().min(MAX_SLUG));
    let mut dash = false;
    for ch in text.chars().flat_map(fold) {
        if ch.is_ascii_alphanumeric() {
            if dash && !out.is_empty() {
                out.push('-');
            }
            dash = false;
            out.push(ch.to_ascii_lowercase());
        } else {
            dash = true;
        }
        if out.len() >= MAX_SLUG {
            break;
        }
    }
    out.truncate(MAX_SLUG);
    let out = out.trim_end_matches('-').to_string();
    if out.is_empty() {
        "lane".into()
    } else {
        out
    }
}

/// The first of `slug`, `slug-2`, `slug-3`… not in `taken`. A suffix never
/// pushes the result past 40 characters: the base is shortened instead.
pub fn dedupe(slug: &str, taken: &HashSet<String>) -> String {
    if !taken.contains(slug) {
        return slug.to_string();
    }
    (2u32..)
        .map(|n| {
            let suffix = format!("-{n}");
            let mut keep = MAX_SLUG.saturating_sub(suffix.len()).min(slug.len());
            while !slug.is_char_boundary(keep) {
                keep -= 1;
            }
            format!("{}{suffix}", slug[..keep].trim_end_matches('-'))
        })
        .find(|candidate| !taken.contains(candidate))
        .unwrap_or_else(|| slug.to_string())
}

/// Latin letters with diacritics, and a few ligatures, to plain ASCII.
fn fold(ch: char) -> impl Iterator<Item = char> {
    let s: &'static str = match ch {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => "a",
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' | 'Ā' | 'Ă' | 'Ą' => "a",
        'ç' | 'ć' | 'č' | 'Ç' | 'Ć' | 'Č' => "c",
        'ď' | 'đ' | 'Ď' | 'Đ' => "d",
        'è' | 'é' | 'ê' | 'ë' | 'ē' | 'ę' | 'ě' => "e",
        'È' | 'É' | 'Ê' | 'Ë' | 'Ē' | 'Ę' | 'Ě' => "e",
        'ğ' | 'Ğ' => "g",
        'ì' | 'í' | 'î' | 'ï' | 'ī' | 'ı' | 'Ì' | 'Í' | 'Î' | 'Ï' | 'Ī' | 'İ' => "i",
        'ł' | 'Ł' => "l",
        'ñ' | 'ń' | 'ň' | 'Ñ' | 'Ń' | 'Ň' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => "o",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' | 'Ō' | 'Ő' => "o",
        'ř' | 'Ř' => "r",
        'ś' | 'š' | 'ş' | 'Ś' | 'Š' | 'Ş' => "s",
        'ß' => "ss",
        'ť' | 'ţ' | 'Ť' | 'Ţ' => "t",
        'ù' | 'ú' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => "u",
        'Ù' | 'Ú' | 'Û' | 'Ü' | 'Ū' | 'Ů' | 'Ű' => "u",
        'ý' | 'ÿ' | 'Ý' | 'Ÿ' => "y",
        'ź' | 'ż' | 'ž' | 'Ź' | 'Ż' | 'Ž' => "z",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        _ => "",
    };
    let single = s.is_empty().then_some(ch);
    s.chars().chain(single)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f_cd_15_slug_is_kebab_case() {
        assert_eq!(slugify("fix auth"), "fix-auth");
        assert_eq!(slugify("  Fix the AUTH flow!! "), "fix-the-auth-flow");
        assert_eq!(
            slugify("Réparer l'écran d'accueil"),
            "reparer-l-ecran-d-accueil"
        );
        assert_eq!(slugify("snake_case/and.dots"), "snake-case-and-dots");
        assert_eq!(slugify("🚀"), "lane");
        assert_eq!(slugify(""), "lane");
        assert_eq!(slugify("测试 tests"), "tests");
    }

    #[test]
    fn f_cd_15_slug_is_at_most_40_characters() {
        let s = slugify("make the flaky integration tests deterministic again please");
        assert!(s.len() <= MAX_SLUG, "{s}");
        assert_eq!(s, "make-the-flaky-integration-tests-determi");
        // A cut on a separator doesn't leave a trailing dash.
        assert_eq!(slugify(&format!("{} b", "a".repeat(39))), "a".repeat(39));
    }

    #[test]
    fn f_cd_15_dedupe_counts_from_2() {
        let mut taken = HashSet::new();
        assert_eq!(dedupe("fix-auth", &taken), "fix-auth");
        taken.insert("fix-auth".to_string());
        assert_eq!(dedupe("fix-auth", &taken), "fix-auth-2");
        taken.insert("fix-auth-2".to_string());
        assert_eq!(dedupe("fix-auth", &taken), "fix-auth-3");
    }

    #[test]
    fn f_cd_15_dedupe_keeps_40_characters() {
        let long = "a".repeat(40);
        let taken = HashSet::from([long.clone()]);
        let next = dedupe(&long, &taken);
        assert_eq!(next.len(), 40);
        assert!(next.ends_with("-2"));
    }
}
