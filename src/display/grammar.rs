//! Shared English phrasing helpers for player-facing output.

/// Join a list for natural prose: `a, b and c`.
pub fn join_natural_list(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => items[0].clone(),
        2 => format!("{} and {}", items[0], items[1]),
        _ => {
            let mut rest = items.to_vec();
            let last = rest.pop().unwrap();
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

/// Join names for prose: first gets a/an, rest chained with "and".
///
/// `["Rusty Sword", "Wooden Sword"]` → `a Rusty Sword and Wooden Sword`
fn phrase_needs_article(phrase: &str) -> bool {
    !phrase
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit())
}

pub fn phrase_with_leading_article(items: &[String]) -> String {
    match items.len() {
        0 => String::new(),
        1 => {
            if phrase_needs_article(&items[0]) {
                format!("{} {}", indefinite_article(&items[0]), items[0])
            } else {
                items[0].clone()
            }
        }
        _ => {
            let first = if phrase_needs_article(&items[0]) {
                format!("{} {}", indefinite_article(&items[0]), items[0])
            } else {
                items[0].clone()
            };
            format!("{first} and {}", items[1..].join(" and "))
        }
    }
}

/// Indefinite article from the first character of a word or phrase.
pub fn indefinite_article(word: &str) -> &'static str {
    match word.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrase_with_leading_article_uses_an_before_vowel() {
        assert_eq!(
            phrase_with_leading_article(&["apple".to_string()]),
            "an apple"
        );
        assert_eq!(
            phrase_with_leading_article(&["Rusty Sword".to_string(), "apple".to_string()]),
            "a Rusty Sword and apple"
        );
        assert_eq!(
            phrase_with_leading_article(&["6 gold bars".to_string()]),
            "6 gold bars"
        );
    }

    #[test]
    fn join_natural_list_formats_two_and_three() {
        assert_eq!(
            join_natural_list(&["20 coins".into(), "sword".into()]),
            "20 coins and sword"
        );
        assert_eq!(
            join_natural_list(&["a".into(), "b".into(), "c".into()]),
            "a, b, and c"
        );
    }
}