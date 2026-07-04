//! Natural-language phrasing for stackable items (look, examine, labels).

use crate::object::{format_weight_amount, Object};

use super::grammar::indefinite_article;

/// Whether a display name is already plural or a mass noun (`coins`, `boots`).
pub fn name_looks_plural(name: &str) -> bool {
    let lower = name.trim().to_lowercase();
    lower.ends_with('s') && !lower.ends_with("ss")
}

/// Simple English plural for item names (`gold bar` → `gold bars`, `coin` → `coins`).
pub fn pluralize_item_name(name: &str) -> String {
    if name_looks_plural(name) {
        return name.to_string();
    }
    if let Some((head, tail)) = name.rsplit_once(' ') {
        format!("{head} {}", pluralize_word(tail))
    } else {
        pluralize_word(name)
    }
}

fn pluralize_word(word: &str) -> String {
    if word.is_empty() {
        return word.to_string();
    }
    let lower = word.to_lowercase();
    if lower.ends_with('y') {
        let bytes = lower.as_bytes();
        if bytes.len() >= 2 && !matches!(bytes[bytes.len() - 2], b'a' | b'e' | b'i' | b'o' | b'u')
        {
            return format!("{}ies", &word[..word.len() - 1]);
        }
    }
    if lower.ends_with("ch") || lower.ends_with("sh") || lower.ends_with('x') || lower.ends_with('z')
    {
        return format!("{word}es");
    }
    format!("{word}s")
}

/// Count + pluralized name: `10 gold bars`, `20 coins`, `gold bar` when count is 1.
pub fn stack_quantity_phrase(obj: &Object) -> String {
    if !obj.is_stackable() {
        return obj.name.clone();
    }
    let count = obj.stack_count();
    if count <= 1 {
        return obj.name.clone();
    }
    let plural = pluralize_item_name(&obj.name);
    if name_looks_plural(&obj.name) {
        format!("{count} {}", obj.name)
    } else {
        format!("{count} {plural}")
    }
}

/// Short label for lists (containers, room contents) — same as quantity phrase for stacks.
pub fn format_stackable_label(item: &Object) -> String {
    stack_quantity_phrase(item)
}

/// Brief `look` sentence when the item has no description.
pub fn format_look_stackable_sentence(obj: &Object) -> String {
    if obj.is_stackable() && obj.stack_count() > 1 {
        format!("There are {}.", stack_quantity_phrase(obj))
    } else {
        format!("It is {} {}.", indefinite_article(&obj.name), obj.name)
    }
}

/// Examine weight line for stackables and weighted singles.
pub fn format_examine_stack_weight(obj: &Object) -> Option<String> {
    if obj.is_stackable() && obj.stack_count() > 1 {
        let total = obj.weight();
        if total > 0.0 || obj.get_numeric_property("weight").is_some() {
            return Some(format!(
                "The stack of {} weighs {} in total.",
                stack_quantity_phrase(obj),
                format_weight_amount(total)
            ));
        }
        return None;
    }

    let w = obj.weight();
    if w > 1.0 || (w > 0.0 && obj.get_numeric_property("weight").is_some()) {
        return Some(format!("It weighs {}.", format_weight_amount(w)));
    }
    None
}

/// Examine fallback when there is no description or weight line.
pub fn format_examine_stackable_fallback(obj: &Object) -> String {
    if obj.is_stackable() && obj.stack_count() > 1 {
        format!("There are {}.", stack_quantity_phrase(obj))
    } else {
        format!("It is {} {}.", indefinite_article(&obj.name), obj.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::{PermissionFlags, StackableSpec};
    use std::collections::HashMap;

    fn bare(id: &str, name: &str) -> Object {
        Object {
            id: crate::object::ObjectId::new(id),
            name: name.to_string(),
            aliases: Vec::new(),
            location: None,
            prototype: None,
            owner: crate::object::ObjectId::new("player:hero-001"),
            permissions: PermissionFlags::OWNER,
            properties: HashMap::new(),
            verbs: HashMap::new(),
            event_handlers: HashMap::new(),
            is_deleted: false,
            deleted_at: None,
        }
    }

    #[test]
    fn pluralize_gold_bar_and_coins() {
        assert_eq!(pluralize_item_name("gold bar"), "gold bars");
        assert_eq!(pluralize_item_name("coins"), "coins");
        assert_eq!(
            stack_quantity_phrase(&{
                let mut bar = bare("item:bar-001", "gold bar");
                bar.apply_stackable_role(&StackableSpec {
                    count: 10,
                    max_stack: 99,
                });
                bar
            }),
            "10 gold bars"
        );
    }

    #[test]
    fn examine_stack_shows_total_weight() {
        let mut bar = bare("item:bar-001", "gold bar");
        bar.set_property_int("weight", 10);
        bar.apply_stackable_role(&StackableSpec {
            count: 10,
            max_stack: 99,
        });

        assert_eq!(
            format_examine_stack_weight(&bar).unwrap(),
            "The stack of 10 gold bars weighs 100 in total."
        );
    }

    #[test]
    fn examine_coins_with_description_style_weight() {
        let mut coins = bare("item:coins-001", "coins");
        coins.set_property_int("weight", 1);
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });

        assert_eq!(
            format_examine_stack_weight(&coins).unwrap(),
            "The stack of 20 coins weighs 20 in total."
        );
    }

    #[test]
    fn look_stackable_plural_sentence() {
        let mut coins = bare("item:coins-001", "coins");
        coins.apply_stackable_role(&StackableSpec {
            count: 20,
            max_stack: 99,
        });
        assert_eq!(format_look_stackable_sentence(&coins), "There are 20 coins.");
    }
}