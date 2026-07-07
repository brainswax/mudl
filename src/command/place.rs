//! Parsers for builder place commands (`@dig`, `@link`, `@unlink`).

use crate::command::is_option_token;
use crate::world::place_builder::DigOptions;

/// Parsed `@dig` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedDigCommand {
    pub direction: String,
    pub name: String,
    pub options: DigOptions,
}

/// Parsed `@link` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLinkCommand {
    pub from: Option<String>,
    pub direction: String,
    pub target: String,
    pub reciprocal: bool,
    pub return_exit: Option<String>,
}

/// Parsed `@unlink` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedUnlinkCommand {
    pub from: Option<String>,
    pub direction: String,
}

fn strip_verb<'a>(input: &'a str, verb: &str) -> Option<&'a str> {
    let trimmed = input.trim();
    let without_at = trimmed.strip_prefix('@').unwrap_or(trimmed);
    without_at.strip_prefix(verb).map(str::trim_start)
}

fn parse_leading_quoted(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    let quote = s.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let inner = &s[1..];
    let end = inner.find(quote)?;
    let name = inner[..end].to_string();
    Some((name, inner[end + 1..].trim()))
}

fn split_name_and_options(rest: &str) -> anyhow::Result<(String, Vec<String>)> {
    if let Some((name, remainder)) = parse_leading_quoted(rest) {
        return Ok((name, tokenize_option_assignments(remainder)));
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let opt_idx = tokens.iter().position(|t| is_option_token(t));
    match opt_idx {
        None => Ok((tokens.join(" "), Vec::new())),
        Some(idx) => Ok((
            tokens[..idx].join(" "),
            tokenize_option_assignments(&tokens[idx..].join(" ")),
        )),
    }
}

fn tokenize_option_assignments(remainder: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    let bytes = remainder.as_bytes();
    while i < bytes.len() {
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let start = i;
        while i < bytes.len() && bytes[i] != b'=' {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        i += 1;
        if i < bytes.len() && (bytes[i] == b'"' || bytes[i] == b'\'') {
            let quote = bytes[i];
            i += 1;
            while i < bytes.len() && bytes[i] != quote {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
        } else {
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() {
                i += 1;
            }
        }
        let token = remainder[start..i].trim();
        if is_option_token(token) {
            tokens.push(token.to_string());
        }
    }
    tokens
}

fn parse_dig_options(tokens: &[String]) -> DigOptions {
    let mut opts = DigOptions::default();
    for token in tokens {
        if let Some((key, value)) = token.split_once('=') {
            let value = value.trim().trim_matches('"').trim_matches('\'');
            match key {
                "type" | "place_type" => opts.place_type = Some(value.to_string()),
                "description" | "desc" => opts.description = Some(value.to_string()),
                "reciprocal" => {
                    opts.reciprocal = Some(value == "true" || value == "1" || value == "yes");
                }
                "return" | "return_exit" => opts.return_exit = Some(value.to_string()),
                _ => {}
            }
        }
    }
    opts
}

/// Parse `@dig <direction> <name...> [description="..."] [type=room] [reciprocal=false]`.
pub fn parse_dig_command(input: &str) -> anyhow::Result<ParsedDigCommand> {
    let rest = strip_verb(input, "dig").ok_or_else(|| {
        anyhow::anyhow!("Usage: @dig <direction> <name...> [description=\"...\"] [type=room]")
    })?;
    if rest.is_empty() {
        anyhow::bail!("Usage: @dig <direction> <name...> [description=\"...\"] [type=room]");
    }

    let mut parts = rest.split_whitespace();
    let direction = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("Usage: @dig <direction> <name...>"))?
        .to_string();
    let remainder = parts.collect::<Vec<_>>().join(" ");
    if remainder.trim().is_empty() {
        anyhow::bail!("Usage: @dig <direction> <name...>");
    }

    let (name, option_tokens) = split_name_and_options(&remainder)?;
    if name.is_empty() {
        anyhow::bail!("Usage: @dig <direction> <name...>");
    }

    Ok(ParsedDigCommand {
        direction,
        name,
        options: parse_dig_options(&option_tokens),
    })
}

/// Parse `@link <direction> <target>` or `@link <from> <direction> <target>`.
pub fn parse_link_command(input: &str) -> anyhow::Result<ParsedLinkCommand> {
    let rest = strip_verb(input, "link").ok_or_else(|| {
        anyhow::anyhow!("Usage: @link <direction> <target>  or  @link <from> <direction> <target>")
    })?;
    if rest.is_empty() {
        anyhow::bail!("Usage: @link <direction> <target>");
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    let reciprocal = !tokens.contains(&"--one-way");
    let mut return_exit = None;
    let mut filtered = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == "--return" {
            return_exit = tokens.get(i + 1).map(|s| s.to_string());
            i += 2;
            continue;
        }
        if !matches!(tokens[i], "--one-way" | "--reciprocal") {
            filtered.push(tokens[i]);
        }
        i += 1;
    }

    match filtered.len() {
        2 => Ok(ParsedLinkCommand {
            from: None,
            direction: filtered[0].to_string(),
            target: filtered[1].to_string(),
            reciprocal,
            return_exit,
        }),
        3 => Ok(ParsedLinkCommand {
            from: Some(filtered[0].to_string()),
            direction: filtered[1].to_string(),
            target: filtered[2].to_string(),
            reciprocal,
            return_exit,
        }),
        _ => anyhow::bail!(
            "Usage: @link <direction> <target>  or  @link <from> <direction> <target>"
        ),
    }
}

/// Parse `@unlink <direction>` or `@unlink <from> <direction>`.
pub fn parse_unlink_command(input: &str) -> anyhow::Result<ParsedUnlinkCommand> {
    let rest =
        strip_verb(input, "unlink").ok_or_else(|| anyhow::anyhow!("Usage: @unlink <direction>"))?;
    if rest.is_empty() {
        anyhow::bail!("Usage: @unlink <direction>");
    }

    let tokens: Vec<&str> = rest.split_whitespace().collect();
    match tokens.len() {
        1 => Ok(ParsedUnlinkCommand {
            from: None,
            direction: tokens[0].to_string(),
        }),
        2 => Ok(ParsedUnlinkCommand {
            from: Some(tokens[0].to_string()),
            direction: tokens[1].to_string(),
        }),
        _ => anyhow::bail!("Usage: @unlink <direction>  or  @unlink <from> <direction>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dig_with_options() {
        let parsed = parse_dig_command(
            r#"@dig west "Storage Closet" description="Cramped shelves." type=room"#,
        )
        .unwrap();
        assert_eq!(parsed.direction, "west");
        assert_eq!(parsed.name, "Storage Closet");
        assert_eq!(parsed.options.place_type.as_deref(), Some("room"));
        assert_eq!(
            parsed.options.description.as_deref(),
            Some("Cramped shelves.")
        );
    }

    #[test]
    fn parse_link_from_current_location_form() {
        let parsed = parse_link_command("@link north forest-path").unwrap();
        assert!(parsed.from.is_none());
        assert_eq!(parsed.direction, "north");
        assert_eq!(parsed.target, "forest-path");
        assert!(parsed.reciprocal);
    }

    #[test]
    fn parse_link_one_way_flag() {
        let parsed = parse_link_command("@link --one-way north forest-path").unwrap();
        assert!(!parsed.reciprocal);
    }

    #[test]
    fn parse_link_explicit_from_form() {
        let parsed = parse_link_command("@link cottage-front in cottage-interior").unwrap();
        assert_eq!(parsed.from.as_deref(), Some("cottage-front"));
        assert_eq!(parsed.direction, "in");
        assert_eq!(parsed.target, "cottage-interior");
    }

    #[test]
    fn parse_unlink_two_token_form() {
        let parsed = parse_unlink_command("@unlink cottage-front rear").unwrap();
        assert_eq!(parsed.from.as_deref(), Some("cottage-front"));
        assert_eq!(parsed.direction, "rear");
    }
}
