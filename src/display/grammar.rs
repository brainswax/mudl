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