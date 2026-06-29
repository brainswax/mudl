use std::collections::HashMap;

/// Parsed object/room definition from declarative MUDL files.
#[derive(Debug, Clone)]
pub struct WorldDef {
    pub obj_type: String,
    pub base_name: String,
    pub name: String,
    pub description: Option<String>,
    pub exits: HashMap<String, String>,
    pub location: Option<String>,
    pub starting_location: Option<String>,
}

/// Parse room/object definitions from MUDL source (legacy declarative format).
pub fn parse_world_file(content: &str) -> (Vec<WorldDef>, Option<String>) {
    let mut defs: Vec<WorldDef> = Vec::new();
    let mut starting_location: Option<String> = None;
    let mut current = WorldDef {
        obj_type: "room".to_string(),
        base_name: "unknown".to_string(),
        name: "Unknown".to_string(),
        description: None,
        exits: HashMap::new(),
        location: None,
        starting_location: None,
    };
    let mut in_exits = false;

    for line in content.lines() {
        let trimmed = line.split(';').next().unwrap_or(line).trim();
        if trimmed.is_empty() {
            if current.base_name != "unknown" {
                if current.obj_type == "config" {
                    starting_location = current.starting_location.clone();
                } else {
                    defs.push(current);
                }
                current = WorldDef {
                    obj_type: "room".to_string(),
                    base_name: "unknown".to_string(),
                    name: "Unknown".to_string(),
                    description: None,
                    exits: HashMap::new(),
                    location: None,
                    starting_location: None,
                };
                in_exits = false;
            }
            continue;
        }
        if trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with("@include")
            || trimmed.starts_with("@include-world")
            || trimmed.starts_with("@universe")
            || trimmed.starts_with("@world")
            || trimmed.starts_with("@creature")
            || trimmed.starts_with("@body-plan")
            || trimmed.starts_with("@player-template")
            || trimmed.starts_with("@slot")
            || trimmed == "@end"
        {
            continue;
        }
        if trimmed == "exits:" {
            in_exits = true;
            continue;
        }
        if in_exits && trimmed.contains(':') {
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                current
                    .exits
                    .insert(parts[0].trim().to_string(), parts[1].trim().to_string());
            }
            continue;
        }
        if trimmed.contains(':') {
            in_exits = false;
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim().to_string();
                match key.as_str() {
                    "type" => current.obj_type = value,
                    "base_name" => current.base_name = value,
                    "name" => current.name = value,
                    "description" => current.description = Some(value),
                    "location" => current.location = Some(value),
                    "starting_location" => current.starting_location = Some(value),
                    _ => {}
                }
            }
        }
    }

    if current.base_name != "unknown" {
        if current.obj_type == "config" {
            starting_location = current.starting_location.clone();
        } else {
            defs.push(current);
        }
    }

    (defs, starting_location)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_world_block_ignored_by_world_def_parser() {
        let content = include_str!("../../modules/default/worlds/default_world/world.mudl");
        let (defs, start) = parse_world_file(content);
        assert!(defs.is_empty());
        assert!(start.is_none());
    }
}
