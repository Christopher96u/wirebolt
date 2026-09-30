//! Reader for Bruno's `.bru` markup (request, folder, collection and
//! environment files), following the grammar in `usebruno/bruno`'s
//! `packages/bruno-lang/v2`.
//!
//! A file is a sequence of top-level blocks. Dictionary blocks hold
//! `key: value` pairs (a `~` key prefix disables the row and `'''` starts a
//! multiline value), text blocks hold raw content indented by two spaces, and
//! list blocks (`vars:secret [a, b]`) hold names. Every block closes with a `}`
//! or `]` in the first column.

use super::ImportError;

#[derive(Debug, Default)]
pub(super) struct BruFile {
    pub blocks: Vec<BruBlock>,
}

#[derive(Debug)]
pub(super) struct BruBlock {
    pub name: String,
    pub content: BruContent,
}

#[derive(Debug)]
pub(super) enum BruContent {
    Pairs(Vec<BruPair>),
    Text(String),
    List(Vec<BruPair>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct BruPair {
    pub name: String,
    pub value: String,
    pub enabled: bool,
}

impl BruFile {
    pub fn block(&self, name: &str) -> Option<&BruContent> {
        self.blocks
            .iter()
            .find(|block| block.name == name)
            .map(|block| &block.content)
    }

    /// Every pair of every block with this name; Bruno merges repeated blocks.
    pub fn pairs(&self, name: &str) -> Vec<BruPair> {
        self.blocks
            .iter()
            .filter(|block| block.name == name)
            .flat_map(|block| match &block.content {
                BruContent::Pairs(pairs) | BruContent::List(pairs) => pairs.clone(),
                BruContent::Text(_) => Vec::new(),
            })
            .collect()
    }

    /// The first value for `key` in a dictionary block, ignoring the disabled marker.
    pub fn value(&self, block: &str, key: &str) -> Option<String> {
        self.pairs(block)
            .into_iter()
            .find(|pair| pair.name == key)
            .map(|pair| pair.value)
    }

    pub fn text(&self, name: &str) -> Option<&str> {
        match self.block(name)? {
            BruContent::Text(text) => Some(text),
            BruContent::Pairs(_) | BruContent::List(_) => None,
        }
    }
}

/// Blocks whose body is raw text rather than `key: value` pairs.
fn is_text_block(name: &str) -> bool {
    matches!(
        name,
        "body"
            | "body:json"
            | "body:text"
            | "body:xml"
            | "body:sparql"
            | "body:graphql"
            | "body:graphql:vars"
            | "tests"
            | "docs"
            | "example"
    ) || name.starts_with("script:")
}

/// Parses one `.bru` document.
///
/// # Errors
///
/// Returns [`ImportError`] when a block is never closed.
pub(super) fn parse(source: &str) -> Result<BruFile, ImportError> {
    let lines = source.lines().collect::<Vec<_>>();
    let mut file = BruFile::default();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim_end();
        index += 1;
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let Some(open) = trimmed.find(['{', '[']) else {
            // Single-line environment attributes such as `color: #fff`.
            continue;
        };
        let name = trimmed[..open].trim().to_owned();
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        let rest = &trimmed[open + 1..];
        if trimmed[open..].starts_with('[') {
            let (items, next) = read_list(rest, &lines, index)?;
            index = next;
            file.blocks.push(BruBlock {
                name,
                content: BruContent::List(items),
            });
            continue;
        }
        let end = lines[index..]
            .iter()
            .position(|line| line.starts_with('}'))
            .map(|offset| index + offset)
            .ok_or_else(|| ImportError::new("Bruno file has an unterminated block"))?;
        let body = &lines[index..end];
        index = end + 1;
        let content = if is_text_block(&name) {
            BruContent::Text(text_block(body))
        } else {
            BruContent::Pairs(pairs(body))
        };
        file.blocks.push(BruBlock { name, content });
    }
    Ok(file)
}

/// Reads `vars:secret [a, ~b]`, which may span several lines.
fn read_list(
    first: &str,
    lines: &[&str],
    mut index: usize,
) -> Result<(Vec<BruPair>, usize), ImportError> {
    let mut text = String::new();
    let mut current = first;
    loop {
        if let Some(close) = current.find(']') {
            text.push_str(&current[..close]);
            break;
        }
        text.push_str(current);
        text.push('\n');
        current = lines
            .get(index)
            .ok_or_else(|| ImportError::new("Bruno file has an unterminated list"))?;
        index += 1;
    }
    let items = text
        .split([',', '\n'])
        .map(str::trim)
        .filter(|item| !item.is_empty() && !item.starts_with('@'))
        .map(|item| {
            let (name, enabled) = disabled_marker(item);
            BruPair {
                name: name.to_owned(),
                value: String::new(),
                enabled,
            }
        })
        .collect();
    Ok((items, index))
}

/// Outdents a text block by the two spaces Bruno writes.
fn text_block(lines: &[&str]) -> String {
    let start = lines
        .iter()
        .position(|line| !line.trim().is_empty())
        .unwrap_or(lines.len());
    lines[start..]
        .iter()
        .map(|line| line.strip_prefix("  ").unwrap_or(line))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pairs(lines: &[&str]) -> Vec<BruPair> {
    let mut pairs = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index].trim();
        index += 1;
        if line.is_empty() {
            continue;
        }
        if let Some(consumed) = annotation_lines(line, &lines[index..]) {
            index += consumed;
            continue;
        }
        let Some((key, raw_value)) = split_pair(line) else {
            continue;
        };
        let value = if raw_value.starts_with("'''") {
            let (value, consumed) = multiline_value(raw_value, &lines[index..]);
            index += consumed;
            value
        } else if raw_value == "[" {
            // A list value (`tags: [`), only used for metadata Wirebolt does not keep.
            index += lines[index..]
                .iter()
                .position(|line| line.trim() == "]")
                .map_or(lines.len() - index, |offset| offset + 1);
            String::new()
        } else {
            raw_value.to_owned()
        };
        let (name, enabled) = disabled_marker(&key);
        pairs.push(BruPair {
            name: name.to_owned(),
            value,
            enabled,
        });
    }
    pairs
}

fn disabled_marker(key: &str) -> (&str, bool) {
    key.strip_prefix('~')
        .map_or((key, true), |stripped| (stripped, false))
}

/// Recognises a decorator line such as `@description('…')` above a pair and
/// returns how many following lines its multiline argument consumed.
fn annotation_lines(line: &str, following: &[&str]) -> Option<usize> {
    let rest = line.strip_prefix('@')?;
    let name_end = rest
        .find(|character: char| matches!(character, '(' | ')' | ':') || character.is_whitespace())
        .unwrap_or(rest.len());
    let after = rest[name_end..].trim_start();
    if name_end == 0 || after.starts_with(':') {
        // `@name: value` is a request-local variable, not a decorator.
        return None;
    }
    if after.starts_with("('''") && !after[4..].contains("'''") {
        let consumed = following
            .iter()
            .position(|line| line.contains("'''"))
            .map_or(following.len(), |offset| offset + 1);
        return Some(consumed);
    }
    Some(0)
}

fn split_pair(line: &str) -> Option<(String, &str)> {
    let quoted = line.strip_prefix('~').unwrap_or(line);
    if let Some(inner) = quoted.strip_prefix('"') {
        let mut key = String::new();
        let mut characters = inner.char_indices();
        let mut close = None;
        while let Some((offset, character)) = characters.next() {
            match character {
                '\\' if inner[offset + 1..].starts_with('"') => {
                    key.push('"');
                    characters.next();
                }
                '"' => {
                    close = Some(offset);
                    break;
                }
                other => key.push(other),
            }
        }
        let rest = inner[close? + 1..].trim_start().strip_prefix(':')?;
        let prefix = if line.starts_with('~') { "~" } else { "" };
        return Some((format!("{prefix}{key}"), rest.trim()));
    }
    let (key, value) = line.split_once(':')?;
    let key = key.trim();
    (!key.is_empty()).then(|| (key.to_owned(), value.trim()))
}

/// Reads a `'''` value: each content line is indented four spaces, and a
/// `@contentType(…)` suffix may follow the closing delimiter.
fn multiline_value(opening: &str, following: &[&str]) -> (String, usize) {
    let inline = opening.trim_start_matches("'''");
    if let Some(end) = inline.find("'''") {
        return (inline[..end].trim().to_owned(), 0);
    }
    let close = following
        .iter()
        .position(|line| line.trim_start().starts_with("'''"))
        .unwrap_or(following.len());
    let content = following[..close]
        .iter()
        .map(|line| {
            let skip = line
                .char_indices()
                .nth(4)
                .map_or(line.len(), |(offset, _)| offset);
            &line[skip..]
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut value = content.trim().to_owned();
    if let Some(suffix) = following
        .get(close)
        .map(|line| line.trim_start().trim_start_matches("'''").trim())
        .filter(|suffix| suffix.starts_with("@contentType("))
    {
        value.push(' ');
        value.push_str(suffix);
    }
    (value, (close + 1).min(following.len()))
}

#[cfg(test)]
mod tests {
    use super::{BruContent, parse};

    #[test]
    fn reads_pairs_disabled_rows_multiline_values_and_text_blocks() {
        let file = parse(concat!(
            "meta {\n  name: Create user\n  type: http\n  seq: 3\n}\n\n",
            "post {\n  url: {{baseUrl}}/users?page=1\n  body: json\n  auth: bearer\n}\n\n",
            "params:query {\n  page: 1\n  ~debug: true\n}\n\n",
            "headers {\n  @description('Tracing header: optional')\n  X-Trace: abc\n",
            "  \"Quoted: Key\": v\n  note: '''\n    first line\n    second line\n  '''\n}\n\n",
            "body:json {\n  {\n    \"name\": \"Ada\"\n  }\n}\n\n",
            "vars:pre-request {\n  @local: yes\n}\n",
        ))
        .expect("parse");

        assert_eq!(file.value("meta", "seq").as_deref(), Some("3"));
        assert_eq!(
            file.value("post", "url").as_deref(),
            Some("{{baseUrl}}/users?page=1")
        );
        let query = file.pairs("params:query");
        assert!(query[0].enabled);
        assert_eq!(query[1].name, "debug");
        assert!(!query[1].enabled);
        let headers = file.pairs("headers");
        assert_eq!(headers.len(), 3);
        assert_eq!(headers[1].name, "Quoted: Key");
        assert_eq!(headers[2].value, "first line\nsecond line");
        assert_eq!(file.text("body:json"), Some("{\n  \"name\": \"Ada\"\n}"));
        assert_eq!(file.pairs("vars:pre-request")[0].name, "@local");
    }

    #[test]
    fn reads_environment_secret_lists() {
        let file = parse(
            "vars {\n  host: https://api.example.test\n  ~legacy: 1\n}\nvars:secret [\n  token,\n  ~apiKey\n]\ncolor: #22aa88\n",
        )
        .expect("parse");
        let Some(BruContent::List(secrets)) = file.block("vars:secret") else {
            panic!("secret list");
        };
        assert_eq!(secrets.len(), 2);
        assert_eq!(secrets[1].name, "apiKey");
        assert!(!secrets[1].enabled);
        assert!(!file.pairs("vars")[1].enabled);
    }

    #[test]
    fn rejects_unterminated_blocks() {
        assert!(parse("meta {\n  name: x\n").is_err());
    }
}
