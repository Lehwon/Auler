use std::collections::BTreeMap;

use auler_domain::code::{RoomKey, normalize_code};
use auler_domain::input::{CodeValue, InputDocument, SCHEMA_V1};
use auler_domain::model::SourceRef;
use serde_json::{Map, Value, json};

use super::{Diagnostic, ImportError, Problem, Row};

type Locations = BTreeMap<String, SourceRef>;
type Sheets = BTreeMap<&'static str, Vec<Row>>;

pub(super) fn assemble(mut sheets: Sheets) -> Result<InputDocument, ImportError> {
    let mut locations = Locations::new();
    let mut unresolved = Vec::new();
    let term_row = sheets
        .remove("term")
        .and_then(|mut rows| rows.pop())
        .expect("term sheet was validated");
    let term = take_fields(&term_row, "term", &["year", "season"], &mut locations);
    locations.insert("term".into(), term_row.source.clone());
    locations.insert("setup".into(), term_row.source.clone());
    let mut setup = take_fields(
        &term_row,
        "setup",
        &["start", "end", "days"],
        &mut locations,
    );
    let rules = take_fields(
        &term_row,
        "setup.rules",
        &[
            "required_course_overlap",
            "block_may_cross_break",
            "room_turnaround_minutes",
        ],
        &mut locations,
    );
    if !rules.is_empty() {
        setup.insert("rules".into(), Value::Object(rules));
    }
    for key in ["periods", "breaks"] {
        let mut rows = Vec::new();
        for (idx, row) in sheets
            .remove(key)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            register_locations(&row, &format!("setup.{key}[{idx}]"), &mut locations);
            rows.push(Value::Object(row.values));
        }
        setup.insert(key.into(), Value::Array(rows));
    }
    let mut out = Map::from_iter([
        ("schema".into(), json!(SCHEMA_V1)),
        ("term".into(), Value::Object(term)),
        ("setup".into(), Value::Object(setup)),
    ]);
    for key in [
        "equipment",
        "rooms",
        "instructors",
        "groups",
        "courses",
        "sections",
        "placements",
    ] {
        let mut records = Vec::new();
        for (idx, mut row) in sheets
            .remove(key)
            .unwrap_or_default()
            .into_iter()
            .enumerate()
        {
            let path = format!("{key}[{idx}]");
            register_locations(&row, &path, &mut locations);
            if key == "sections"
                && let Some(Value::Array(values)) = row.values.remove("instructors")
            {
                let source = row.field("instructors");
                let refs = values
                    .into_iter()
                    .enumerate()
                    .map(|(idx, code)| {
                        locations.insert(format!("{path}.instructors[{idx}].code"), source.clone());
                        json!({ "code": code })
                    })
                    .collect();
                row.values.insert("instructors".into(), Value::Array(refs));
            }
            if key == "placements" {
                let room = room_ref(&mut row, &format!("{path}.room"), &mut locations);
                if !room.is_empty() {
                    row.values.insert("room".into(), Value::Object(room));
                }
            }
            row.values.insert("source".into(), json!(row.source));
            records.push(Value::Object(row.values));
        }
        out.insert(key.into(), Value::Array(records));
    }

    for (sheet, parent, field) in [
        ("room_equipment", "rooms", "equipment"),
        ("section_groups", "sections", "groups"),
        ("section_equipment", "sections", "equipment"),
    ] {
        let children = sheets.remove(sheet).unwrap_or_default();
        let records = out
            .get_mut(parent)
            .and_then(Value::as_array_mut)
            .expect("parent array");
        let mut indices = BTreeMap::new();
        for (idx, record) in records.iter().enumerate() {
            if let Some((key, numeric)) = record
                .as_object()
                .and_then(|values| parent_key(parent, values))
            {
                indices
                    .entry(key)
                    .and_modify(|slot| *slot = ParentMatch::Duplicate)
                    .or_insert(if numeric {
                        ParentMatch::Numeric
                    } else {
                        ParentMatch::Unique(idx)
                    });
            }
        }
        for mut row in children {
            let fields = if parent == "rooms" {
                ["building", "number"]
            } else {
                ["course", "section"]
            };
            let numeric = fields
                .into_iter()
                .find(|field| row.values.get(*field).is_some_and(Value::is_number));
            let parent_status = match (numeric, parent_key(parent, &row.values)) {
                (Some(field), _) => Err(("numeric_child_key", field)),
                (None, None) => Err(("invalid_parent_key", fields[1])),
                (None, Some((key, _))) => match indices.get(&key) {
                    Some(ParentMatch::Unique(idx)) => Ok(*idx),
                    Some(ParentMatch::Numeric) => Err(("numeric_parent_key", fields[1])),
                    Some(ParentMatch::Duplicate) => Err(("duplicate_parent", fields[1])),
                    None => Err(("missing_parent", fields[1])),
                },
            };
            let idx = match parent_status {
                Ok(idx) => idx,
                Err((reason, source_field)) => {
                    let source = row.field(source_field);
                    let text = row
                        .values
                        .iter()
                        .map(|(key, value)| {
                            let value = value
                                .as_str()
                                .map(str::to_owned)
                                .unwrap_or_else(|| value.to_string());
                            format!("{key} = {value}")
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    unresolved.push(
                        json!({ "what": sheet, "text": text, "reason": reason, "source": source }),
                    );
                    continue;
                }
            };
            for key in fields {
                row.values.remove(key);
                row.cells.remove(key);
            }
            if field == "equipment" {
                if let Some(value) = row.values.remove("equipment") {
                    row.values.insert("code".into(), value);
                }
                if let Some(source) = row.cells.remove("equipment") {
                    row.cells.insert("code".into(), source);
                }
            }
            let record = records[idx].as_object_mut().expect("record object");
            let children = record
                .entry(field)
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("child array");
            let path = format!("{parent}[{idx}].{field}[{}]", children.len());
            register_locations(&row, &path, &mut locations);
            children.push(Value::Object(row.values));
        }
    }

    let constraints = constraints(
        sheets.remove("constraints").unwrap_or_default(),
        &mut locations,
    )?;
    out.insert("constraints".into(), Value::Array(constraints));
    out.insert("locations".into(), json!(locations));
    out.insert("unresolved".into(), Value::Array(unresolved));
    serde_json::from_value(Value::Object(out)).map_err(ImportError::Document)
}

enum ParentMatch {
    Unique(usize),
    Numeric,
    Duplicate,
}

fn parent_key(entity: &str, values: &Map<String, Value>) -> Option<((String, String), bool)> {
    let code = |key| serde_json::from_value::<CodeValue>(values.get(key)?.clone()).ok();
    if entity == "rooms" {
        let number = code("number")?;
        let key = RoomKey::new(values.get("building")?.as_str()?, &number.text).ok()?;
        Some(((key.building, key.number), number.from_number))
    } else {
        let course = code("course")?;
        let section = code("section")?;
        Some((
            (
                normalize_code(&course.text).ok()?,
                normalize_code(&section.text).ok()?,
            ),
            course.from_number || section.from_number,
        ))
    }
}

fn register_locations(row: &Row, path: &str, locations: &mut Locations) {
    locations.insert(path.to_owned(), row.source.clone());
    for (key, source) in &row.cells {
        locations.insert(format!("{path}.{key}"), source.clone());
    }
}

fn take_fields(
    row: &Row,
    path: &str,
    fields: &[&str],
    locations: &mut Locations,
) -> Map<String, Value> {
    fields
        .iter()
        .filter_map(|&key| {
            let value = row.values.get(key)?.clone();
            locations.insert(format!("{path}.{key}"), row.field(key));
            Some((key.to_owned(), value))
        })
        .collect()
}

fn room_ref(row: &mut Row, path: &str, locations: &mut Locations) -> Map<String, Value> {
    let mut out = Map::new();
    for key in ["building", "number"] {
        if let Some(value) = row.values.remove(key) {
            locations.insert(format!("{path}.{key}"), row.field(key));
            locations.insert(path.to_owned(), row.field(key));
            out.insert(key.into(), value);
        }
    }
    out
}

struct ConstraintRows {
    properties: Map<String, Value>,
    windows: Vec<Value>,
}

fn constraints(rows: Vec<Row>, locations: &mut Locations) -> Result<Vec<Value>, ImportError> {
    let mut groups: Vec<ConstraintRows> = Vec::new();
    let mut indices = BTreeMap::new();
    let mut issues = Vec::new();
    for row in rows {
        let key = row.values["key"].to_string();
        let kind = row.values["target_kind"]
            .as_str()
            .expect("target enum was checked");
        let value = &row.values["target"];
        let target = match kind {
            "instructor" => json!({ "instructor": { "code": value } }),
            "section" => {
                let section = row.values.get("target_section");
                if section.is_none() {
                    issues.push(Diagnostic {
                        source: row.field("target_section"),
                        kind: Problem::MissingValue {
                            key: "target_section",
                        },
                    });
                    continue;
                }
                json!({ "section": { "course": value, "section": section } })
            }
            "group" => json!({ "group": value }),
            "room" => {
                if value.is_string() {
                    json!({ "room": { "text": value } })
                } else {
                    json!({ "room": { "number": value } })
                }
            }
            _ => unreachable!("target enum was checked"),
        };
        let idx = *indices.entry(key.clone()).or_insert(groups.len());
        let path = format!("constraints[{idx}]");
        let mut properties: Map<_, _> = [
            "key",
            "strength",
            "weight",
            "status",
            "from",
            "until",
            "text",
            "confirmed_by",
            "note",
        ]
        .into_iter()
        .filter_map(|key| {
            row.values
                .get(key)
                .cloned()
                .map(|value| (key.to_owned(), value))
        })
        .collect();
        properties.insert("target".into(), target);
        if idx == groups.len() {
            locations.insert(path.clone(), row.source.clone());
            groups.push(ConstraintRows {
                properties: Map::from_iter([("source".into(), json!(row.source))]),
                windows: Vec::new(),
            });
        }
        let group = &mut groups[idx];
        for (field, value) in properties {
            if let Some(old) = group.properties.get(&field) {
                if old != &value {
                    issues.push(Diagnostic {
                        source: row.field(&field),
                        kind: Problem::ConflictingConstraint {
                            key: key.clone(),
                            field,
                        },
                    });
                }
            } else {
                locations.insert(format!("{path}.{field}"), row.field(&field));
                if field == "target" {
                    for child in [
                        "instructor.code",
                        "section.course",
                        "group",
                        "room",
                        "room.number",
                    ] {
                        locations.insert(format!("{path}.target.{child}"), row.field("target"));
                    }
                    if kind == "section" {
                        locations.insert(
                            format!("{path}.target.section.section"),
                            row.field("target_section"),
                        );
                    }
                }
                group.properties.insert(field, value);
            }
        }
        let window_path = format!("{path}.rule.windows[{}]", group.windows.len());
        let window = take_fields(
            &row,
            &window_path,
            &["day", "first_period", "last_period", "start", "end"],
            locations,
        );
        if !window.is_empty() {
            locations.insert(window_path, row.source.clone());
            if !window.contains_key("day") {
                issues.push(Diagnostic {
                    source: row.field("day"),
                    kind: Problem::MissingValue { key: "day" },
                });
            } else {
                group.windows.push(Value::Object(window));
            }
        }
    }
    if !issues.is_empty() {
        return Err(ImportError::Validation(issues));
    }
    Ok(groups
        .into_iter()
        .map(|mut group| {
            if !group.windows.is_empty() {
                group.properties.insert(
                    "rule".into(),
                    json!({ "kind": "avoid_times", "windows": group.windows }),
                );
            }
            Value::Object(group.properties)
        })
        .collect())
}
