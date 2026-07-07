//! MUDL `@schedule` definitions — periodic timed events on rooms and objects.

/// A recurring schedule that fires a named event every N scope ticks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduleDef {
    pub base_name: String,
    pub target: String,
    pub interval: u32,
    pub event: String,
}

fn strip_comment(line: &str) -> &str {
    line.split(';').next().unwrap_or(line).trim()
}

/// Parse `@schedule` blocks from MUDL source.
pub fn parse_schedule_file(content: &str) -> Vec<ScheduleDef> {
    let mut schedules = Vec::new();
    let mut current: Option<ScheduleDef> = None;

    for raw_line in content.lines() {
        let line = strip_comment(raw_line);
        if line.is_empty() {
            continue;
        }
        if line == "@end" {
            if let Some(schedule) = current.take() {
                schedules.push(schedule);
            }
            continue;
        }

        if let Some(name) = line.strip_prefix("@schedule ") {
            if let Some(schedule) = current.take() {
                schedules.push(schedule);
            }
            current = Some(ScheduleDef {
                base_name: name.trim().to_string(),
                target: String::new(),
                interval: 1,
                event: String::new(),
            });
            continue;
        }

        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim().to_lowercase();
            let value = value.trim();
            if let Some(schedule) = &mut current {
                match key.as_str() {
                    "target" | "location" | "attach" => schedule.target = value.to_string(),
                    "interval" | "every" | "periodic_interval" => {
                        schedule.interval = value.parse().unwrap_or(1).max(1)
                    }
                    "event" | "on_tick" | "trigger" => schedule.event = value.to_lowercase(),
                    _ => {}
                }
            }
        }
    }

    if let Some(schedule) = current {
        schedules.push(schedule);
    }

    schedules
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_schedule_block() {
        let content = r#"
@schedule mist-weather
  target=haunted-mist
  interval=2
  event=on_weather
@end
"#;
        let schedules = parse_schedule_file(content);
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].target, "haunted-mist");
        assert_eq!(schedules[0].interval, 2);
        assert_eq!(schedules[0].event, "on_weather");
    }
}