//! Strict YAML syntax and document loading.

use super::*;

pub(super) fn parse_document<T>(
    bytes: &[u8],
    parser: impl FnOnce(&str) -> Result<T, Vec<ConfigIssue>>,
) -> LoadResult<T> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => {
            return LoadResult::Invalid {
                issues: vec![ConfigIssue {
                    path: "$".to_owned(),
                    message: format!("configuration is not UTF-8: {error}"),
                }],
            };
        }
    };
    if let Some(version) = top_level_version(text)
        && version != VERSION
    {
        return LoadResult::Unsupported {
            version,
            bytes: bytes.to_vec(),
        };
    }
    match parser(text) {
        Ok(value) => LoadResult::Valid(Loaded {
            value,
            fingerprint: Fingerprint::for_bytes(bytes),
        }),
        Err(issues) => LoadResult::Invalid { issues },
    }
}

pub(super) fn top_level_version(text: &str) -> Option<u64> {
    text.lines().find_map(|line| {
        let line = line.trim_end();
        if line.starts_with(char::is_whitespace) || !line.starts_with("version:") {
            return None;
        }
        line["version:".len()..]
            .split('#')
            .next()
            .and_then(|value| value.trim().parse().ok())
    })
}

pub(crate) fn parse_yaml<T: for<'de> Deserialize<'de>>(text: &str) -> Result<T, Vec<ConfigIssue>> {
    if let Some(message) = forbidden_yaml_feature(text) {
        return Err(vec![ConfigIssue {
            path: "$".to_owned(),
            message,
        }]);
    }
    let options = serde_saphyr::options! {
        budget: serde_saphyr::budget! { max_documents: 1 },
        duplicate_keys: serde_saphyr::DuplicateKeyPolicy::Error,
        merge_keys: serde_saphyr::MergeKeyPolicy::Error,
        strict_booleans: true,
        emit_comments: false,
    };
    serde_saphyr::from_str_with_options(text, options).map_err(|error| {
        vec![ConfigIssue {
            path: "$".to_owned(),
            message: error.to_string(),
        }]
    })
}

fn forbidden_yaml_feature(text: &str) -> Option<String> {
    if has_multiple_documents(text) {
        return Some("multiple YAML documents are not allowed".to_owned());
    }
    text.lines().find_map(forbidden_line_feature)
}

fn has_multiple_documents(text: &str) -> bool {
    let mut starts = text
        .lines()
        .enumerate()
        .filter(|(_, line)| line.trim() == "---");
    match starts.next() {
        None => false,
        Some((index, _)) => index != 0 || starts.next().is_some(),
    }
}

fn forbidden_line_feature(line: &str) -> Option<String> {
    let content = line.split('#').next().unwrap_or_default();
    if content.trim_start().starts_with("<<:") {
        return Some("YAML merge keys are not allowed".to_owned());
    }
    if contains_yaml_indicator(content, '&') || contains_yaml_indicator(content, '*') {
        return Some("YAML anchors and aliases are not allowed".to_owned());
    }
    contains_yaml_indicator(content, '!').then(|| "YAML tags are not allowed".to_owned())
}

fn contains_yaml_indicator(line: &str, sought: char) -> bool {
    let mut quotes = QuoteState::default();
    for (index, character) in line.char_indices() {
        if quotes.consume(character) {
            continue;
        }
        if quotes.matches_indicator(character, sought) && indicator_boundary(line, index) {
            return true;
        }
    }
    false
}

fn indicator_boundary(line: &str, index: usize) -> bool {
    line[..index]
        .chars()
        .next_back()
        .is_none_or(|value| value.is_whitespace() || value == ':')
}

#[derive(Default)]
struct QuoteState {
    single: bool,
    double: bool,
    escaped: bool,
}

impl QuoteState {
    fn matches_indicator(&self, character: char, sought: char) -> bool {
        character == sought && self.unquoted()
    }

    fn unquoted(&self) -> bool {
        !self.single && !self.double
    }

    fn starts_escape(&self, character: char) -> bool {
        self.double && character == '\\' && !self.escaped
    }

    fn starts_single_quote(&self, character: char) -> bool {
        character == '\'' && !self.double
    }

    fn starts_double_quote(&self, character: char) -> bool {
        character == '"' && !self.single && !self.escaped
    }

    fn consume(&mut self, character: char) -> bool {
        if self.starts_escape(character) {
            self.escaped = true;
            return true;
        }
        let consumed = if self.starts_single_quote(character) {
            self.single = !self.single;
            true
        } else if self.starts_double_quote(character) {
            self.double = !self.double;
            true
        } else {
            false
        };
        self.escaped = false;
        consumed
    }
}
