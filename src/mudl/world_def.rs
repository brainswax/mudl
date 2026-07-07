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
    /// When set, leaving via `scatter_direction` sends the player to one of these places.
    pub scatter_to: Vec<String>,
    pub scatter_direction: Option<String>,
    /// When set, entering this place silently sends the player to the named place.
    pub loop_to: Option<String>,
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
        scatter_to: Vec::new(),
        scatter_direction: None,
        loop_to: None,
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
                    scatter_to: Vec::new(),
                    scatter_direction: None,
                    loop_to: None,
                };
                in_exits = false;
            }
            continue;
        }
        if trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with("@include")
            || trimmed.starts_with("@import")
            || trimmed.starts_with("@include-world")
            || trimmed.starts_with("@expansion")
            || trimmed.starts_with("@universe")
            || trimmed.starts_with("@world")
            || trimmed.starts_with("@creature")
            || trimmed.starts_with("@body-plan")
            || trimmed.starts_with("@player-template")
            || trimmed.starts_with("@npc")
            || trimmed.starts_with("@spawner")
            || trimmed.starts_with("@spawn-template")
            || trimmed.starts_with("@loot-spawner")
            || trimmed.starts_with("@loot-template")
            || trimmed.starts_with("@entry ")
            || trimmed.starts_with("@behavior")
            || trimmed.starts_with("@stat")
            || trimmed.starts_with("@skill")
            || trimmed.starts_with("@effect")
            || trimmed.starts_with("@slot")
            || trimmed.starts_with("@prototype ")
            || trimmed.starts_with("@item ")
            || trimmed == "@end"
        {
            continue;
        }
        if trimmed == "exits:" {
            in_exits = true;
            continue;
        }
        if trimmed.contains(':') {
            let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
            if parts.len() == 2 {
                let key = parts[0].trim().to_lowercase();
                let value = parts[1].trim().to_string();
                if key == "scatter_to" || key == "scatter_direction" || key == "loop_to" {
                    in_exits = false;
                    match key.as_str() {
                        "scatter_to" => {
                            current.scatter_to = value
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                        }
                        "scatter_direction" => current.scatter_direction = Some(value),
                        "loop_to" => current.loop_to = Some(value),
                        _ => {}
                    }
                    continue;
                }
                if in_exits {
                    current.exits.insert(parts[0].trim().to_string(), value);
                    continue;
                }
                in_exits = false;
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

    #[test]
    fn parse_default_map_locations_and_exits() {
        let content = include_str!("../../modules/default/worlds/default_world/map.mudl");
        let (defs, _) = parse_world_file(content);
        let bases: Vec<_> = defs.iter().map(|d| d.base_name.as_str()).collect();
        assert_eq!(bases.len(), 7);
        assert!(bases.contains(&"the-void"));
        assert!(bases.contains(&"forest-path"));
        assert!(bases.contains(&"cottage-interior"));
        assert!(bases.contains(&"cottage-bedroom"));
        assert!(bases.contains(&"cottage-pantry"));

        let clearing = defs.iter().find(|d| d.base_name == "the-void").unwrap();
        assert_eq!(clearing.exits.get("north").map(String::as_str), Some("forest-path"));
        assert_eq!(clearing.exits.get("east").map(String::as_str), Some("cottage-rear"));

        let front = defs.iter().find(|d| d.base_name == "cottage-front").unwrap();
        assert_eq!(front.exits.get("in").map(String::as_str), Some("cottage-interior"));

        let bedroom = defs.iter().find(|d| d.base_name == "cottage-bedroom").unwrap();
        assert_eq!(bedroom.obj_type, "room");
        assert_eq!(bedroom.location.as_deref(), Some("cottage-interior"));
        assert_eq!(
            bedroom.exits.get("east").map(String::as_str),
            Some("cottage-interior")
        );

        let interior = defs.iter().find(|d| d.base_name == "cottage-interior").unwrap();
        assert_eq!(
            interior.exits.get("west").map(String::as_str),
            Some("cottage-bedroom")
        );
        assert_eq!(
            interior.exits.get("east").map(String::as_str),
            Some("cottage-pantry")
        );
    }

    #[test]
    fn parse_haunted_map_scatter_and_dead_ends() {
        let content = include_str!("../../modules/default/worlds/default_world/expansions/haunted_forest.mudl");
        let (defs, _) = parse_world_file(content);
        assert_eq!(defs.len(), 13);

        let heart = defs.iter().find(|d| d.base_name == "haunted-heart").unwrap();
        assert_eq!(
            heart.scatter_to,
            vec![
                "the-void".to_string(),
                "forest-path".to_string(),
                "cottage-rear".to_string()
            ]
        );
        assert_eq!(heart.scatter_direction.as_deref(), Some("out"));

        let wither = defs.iter().find(|d| d.base_name == "haunted-wither").unwrap();
        assert_eq!(
            wither.exits.get("out").map(String::as_str),
            Some("haunted-entry")
        );
        assert_eq!(wither.loop_to.as_deref(), Some("haunted-entry"));
    }
}
