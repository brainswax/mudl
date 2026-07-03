//! REPL / command-line parsing with `@` meta-command detection.

use crate::object::ObjectId;

/// Parsed user input line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandLine {
    /// True when input begins with `@` (wizard/builder meta-command).
    pub is_meta: bool,
    /// Verb name, lowercased, without the `@` prefix.
    pub verb: String,
    /// Arguments after the verb (preserves original casing).
    pub args: Vec<String>,
}

/// Parse a trimmed command line into verb and arguments.
///
/// Leading `@` marks a meta-command: `@examine sword` → `is_meta=true`, `verb="examine"`,
/// `args=["sword"]`.
pub fn parse_command_line(input: &str) -> CommandLine {
    let input = input.trim();
    let is_meta = input.starts_with('@');
    let body = if is_meta { input[1..].trim_start() } else { input };

    let mut parts = body.split_whitespace();
    let verb = parts.next().unwrap_or_default().to_ascii_lowercase();
    let args: Vec<String> = parts.map(str::to_string).collect();

    CommandLine {
        is_meta,
        verb,
        args,
    }
}

/// Whether this parsed line is a wizard/builder meta-command.
pub fn is_meta_command(line: &CommandLine) -> bool {
    line.is_meta
}

/// Stub wizard permission check until RBAC is wired to player roles.
///
/// REPL sessions treat the default player as wizard-capable.
pub fn has_wizard_permission(_actor: &ObjectId) -> bool {
    true
}

/// Player-facing message when a meta-command is denied.
pub fn wizard_access_denied() -> &'static str {
    "You lack the wizard privilege to use @-commands."
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_regular_examine() {
        let line = parse_command_line("examine coins");
        assert_eq!(
            line,
            CommandLine {
                is_meta: false,
                verb: "examine".to_string(),
                args: vec!["coins".to_string()],
            }
        );
    }

    #[test]
    fn parse_meta_examine_strips_prefix() {
        let line = parse_command_line("@examine coins");
        assert_eq!(
            line,
            CommandLine {
                is_meta: true,
                verb: "examine".to_string(),
                args: vec!["coins".to_string()],
            }
        );
    }

    #[test]
    fn parse_meta_create() {
        let line = parse_command_line("@create container purse capacity=3");
        assert!(line.is_meta);
        assert_eq!(line.verb, "create");
        assert_eq!(line.args[0], "container");
    }

    #[test]
    fn meta_command_detection() {
        let regular = parse_command_line("look");
        let meta = parse_command_line("@dump");
        assert!(!is_meta_command(&regular));
        assert!(is_meta_command(&meta));
    }
}