//! Configuration document structure validation.

use super::*;

pub(super) fn validate_repository_shape(value: &Value) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let Some(root) = expect_object(value, "$", &mut issues) else {
        return issues;
    };
    validate_keys(
        root,
        "$",
        ObjectSchema {
            allowed: &["version", "skill_directories", "enablements"],
            required: &["version", "skill_directories", "enablements"],
        },
        &mut issues,
    );
    expect_unsigned(root.get("version"), "version", &mut issues);
    validate_directory_array(root.get("skill_directories"), &mut issues);
    validate_enablement_array(root.get("enablements"), &mut issues);
    issues
}

pub(super) fn validate_library_shape(value: &Value) -> Vec<ConfigIssue> {
    let mut issues = Vec::new();
    let Some(root) = expect_object(value, "$", &mut issues) else {
        return issues;
    };
    validate_keys(
        root,
        "$",
        ObjectSchema {
            allowed: &["version", "locations", "hidden_skills"],
            required: &["version", "locations"],
        },
        &mut issues,
    );
    expect_unsigned(root.get("version"), "version", &mut issues);
    validate_hidden_array(root.get("hidden_skills"), &mut issues);
    validate_location_array(root.get("locations"), &mut issues);
    issues
}

struct ObjectSchema<'a> {
    allowed: &'a [&'a str],
    required: &'a [&'a str],
}

fn validate_keys(
    object: &serde_json::Map<String, Value>,
    path: &str,
    schema: ObjectSchema<'_>,
    issues: &mut Vec<ConfigIssue>,
) {
    validate_unknown_keys(object, path, schema.allowed, issues);
    for key in schema.required {
        if !object.contains_key(*key) {
            issues.push(ConfigIssue {
                path: field_path(path, key),
                message: "missing required field".to_owned(),
            });
        }
    }
}

pub(super) fn field_path(parent: &str, field: &str) -> String {
    if parent == "$" {
        field.to_owned()
    } else {
        format!("{parent}.{field}")
    }
}

pub(super) fn expect_object<'a>(
    value: &'a Value,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a serde_json::Map<String, Value>> {
    match value.as_object() {
        Some(object) => Some(object),
        None => {
            issues.push(type_issue(path, "mapping"));
            None
        }
    }
}

pub(super) fn expect_object_value<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a serde_json::Map<String, Value>> {
    value.and_then(|value| expect_object(value, path, issues))
}

pub(super) fn expect_array<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a [Value]> {
    match value.and_then(Value::as_array) {
        Some(array) => Some(array),
        None if value.is_none() => None,
        None => {
            issues.push(type_issue(path, "sequence"));
            None
        }
    }
}

pub(super) fn expect_optional_array<'a>(
    value: Option<&'a Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) -> Option<&'a [Value]> {
    match value {
        None => Some(&[]),
        Some(value) => expect_array(Some(value), path, issues),
    }
}

pub(super) fn expect_string(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| !value.is_string()) {
        issues.push(type_issue(path, "string"));
    }
}

pub(super) fn expect_optional_string(
    value: Option<&Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) {
    if value.is_some_and(|value| !value.is_null() && !value.is_string()) {
        issues.push(type_issue(path, "string or null"));
    }
}

pub(super) fn expect_unsigned(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    if value.is_some_and(|value| value.as_u64().is_none()) {
        issues.push(type_issue(path, "non-negative integer"));
    }
}

pub(super) fn expect_optional_bool(
    value: Option<&Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) {
    if value.is_some_and(|value| !value.is_boolean()) {
        issues.push(type_issue(path, "boolean"));
    }
}

pub(super) fn expect_optional_string_array(
    value: Option<&Value>,
    path: &str,
    issues: &mut Vec<ConfigIssue>,
) {
    let Some(values) = expect_optional_array(value, path, issues) else {
        return;
    };
    for (index, value) in values.iter().enumerate() {
        if !value.is_string() {
            issues.push(type_issue(&format!("{path}[{index}]"), "string"));
        }
    }
}

pub(super) fn type_issue(path: &str, expected: &str) -> ConfigIssue {
    ConfigIssue {
        path: path.to_owned(),
        message: format!("expected {expected}"),
    }
}

fn validate_directory_array(value: Option<&Value>, issues: &mut Vec<ConfigIssue>) {
    if let Some(directories) = expect_array(value, "skill_directories", issues) {
        for (index, value) in directories.iter().enumerate() {
            let path = format!("skill_directories[{index}]");
            let Some(object) = expect_object(value, &path, issues) else {
                continue;
            };
            validate_keys(
                object,
                &path,
                ObjectSchema {
                    allowed: &["key", "path", "label"],
                    required: &["key", "path"],
                },
                issues,
            );
            expect_string(object.get("key"), &format!("{path}.key"), issues);
            expect_string(object.get("path"), &format!("{path}.path"), issues);
            expect_optional_string(object.get("label"), &format!("{path}.label"), issues);
        }
    }
}

fn validate_enablement_array(value: Option<&Value>, issues: &mut Vec<ConfigIssue>) {
    if let Some(enablements) = expect_array(value, "enablements", issues) {
        for (index, value) in enablements.iter().enumerate() {
            let path = format!("enablements[{index}]");
            let Some(object) = expect_object(value, &path, issues) else {
                continue;
            };
            validate_keys(
                object,
                &path,
                ObjectSchema {
                    allowed: &["directory", "skill", "materialization"],
                    required: &["directory", "skill", "materialization"],
                },
                issues,
            );
            expect_string(
                object.get("directory"),
                &format!("{path}.directory"),
                issues,
            );
            expect_string(
                object.get("materialization"),
                &format!("{path}.materialization"),
                issues,
            );
            let skill_path = format!("{path}.skill");
            if let Some(skill) = expect_object_value(object.get("skill"), &skill_path, issues) {
                validate_keys(
                    skill,
                    &skill_path,
                    ObjectSchema {
                        allowed: &["source", "path"],
                        required: &["source", "path"],
                    },
                    issues,
                );
                expect_string(skill.get("source"), &format!("{skill_path}.source"), issues);
                expect_string(skill.get("path"), &format!("{skill_path}.path"), issues);
            }
        }
    }
}

fn validate_hidden_array(value: Option<&Value>, issues: &mut Vec<ConfigIssue>) {
    if let Some(hidden) = expect_optional_array(value, "hidden_skills", issues) {
        for (index, skill) in hidden.iter().enumerate() {
            let path = format!("hidden_skills[{index}]");
            if let Some(skill) = expect_object(skill, &path, issues) {
                validate_keys(
                    skill,
                    &path,
                    ObjectSchema {
                        allowed: &["source", "path"],
                        required: &["source", "path"],
                    },
                    issues,
                );
                expect_string(skill.get("source"), &format!("{path}.source"), issues);
                expect_string(skill.get("path"), &format!("{path}.path"), issues);
            }
        }
    }
}

fn validate_location_array(value: Option<&Value>, issues: &mut Vec<ConfigIssue>) {
    if let Some(locations) = expect_array(value, "locations", issues) {
        for (location_index, value) in locations.iter().enumerate() {
            let path = format!("locations[{location_index}]");
            let Some(location) = expect_object(value, &path, issues) else {
                continue;
            };
            validate_keys(
                location,
                &path,
                ObjectSchema {
                    allowed: &["path", "exclusions", "allow_overlap", "sources"],
                    required: &["path"],
                },
                issues,
            );
            expect_string(location.get("path"), &format!("{path}.path"), issues);
            expect_optional_string_array(
                location.get("exclusions"),
                &format!("{path}.exclusions"),
                issues,
            );
            expect_optional_bool(
                location.get("allow_overlap"),
                &format!("{path}.allow_overlap"),
                issues,
            );
            validate_source_array(location.get("sources"), &path, issues);
        }
    }
}

fn validate_source_array(value: Option<&Value>, path: &str, issues: &mut Vec<ConfigIssue>) {
    let Some(sources) = expect_optional_array(value, &format!("{path}.sources"), issues) else {
        return;
    };
    for (source_index, value) in sources.iter().enumerate() {
        let source_path = format!("{path}.sources[{source_index}]");
        let Some(source) = expect_object(value, &source_path, issues) else {
            continue;
        };
        validate_keys(
            source,
            &source_path,
            ObjectSchema {
                allowed: &["key", "path", "skills"],
                required: &["key", "path"],
            },
            issues,
        );
        expect_string(source.get("key"), &format!("{source_path}.key"), issues);
        expect_string(source.get("path"), &format!("{source_path}.path"), issues);
        validate_skill_array(source.get("skills"), &source_path, issues);
    }
}

fn validate_skill_array(value: Option<&Value>, source_path: &str, issues: &mut Vec<ConfigIssue>) {
    let Some(skills) = expect_optional_array(value, &format!("{source_path}.skills"), issues)
    else {
        return;
    };
    for (skill_index, value) in skills.iter().enumerate() {
        let skill_path = format!("{source_path}.skills[{skill_index}]");
        let Some(skill) = expect_object(value, &skill_path, issues) else {
            continue;
        };
        validate_keys(
            skill,
            &skill_path,
            ObjectSchema {
                allowed: &["path"],
                required: &["path"],
            },
            issues,
        );
        expect_string(skill.get("path"), &format!("{skill_path}.path"), issues);
    }
}

fn validate_unknown_keys(
    object: &serde_json::Map<String, Value>,
    path: &str,
    allowed: &[&str],
    issues: &mut Vec<ConfigIssue>,
) {
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            issues.push(ConfigIssue {
                path: field_path(path, key),
                message: "unknown field".to_owned(),
            });
        }
    }
}
