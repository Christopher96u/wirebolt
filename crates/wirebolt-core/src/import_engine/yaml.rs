//! Converts YAML documents into `serde_json` values for the importers.

use serde_json::{Map, Value};
use yaml_rust2::{Event, Yaml, YamlLoader, parser::Parser};

use super::ImportError;

const MAX_DEPTH: usize = 128;

/// Parses the first YAML document.
///
/// `serde_json` maps are sorted, so for every mapping with a `data` mapping
/// (Insomnia environments) the original key order is recorded in a
/// `dataPropertyOrder` entry, mirroring what Insomnia writes to JSON exports.
///
/// # Errors
///
/// Returns [`ImportError`] for invalid YAML, aliases or excessive nesting.
pub(super) fn parse(source: &str) -> Result<Value, ImportError> {
    reject_aliases_and_deep_nesting(source)?;
    let documents =
        YamlLoader::load_from_str(source).map_err(|_| ImportError::new("YAML is invalid"))?;
    let document = documents
        .into_iter()
        .next()
        .ok_or_else(|| ImportError::new("YAML document is empty"))?;
    convert(&document, 0)
}

/// Aliases are expanded by copying when loaded, so a small document could
/// otherwise grow exponentially. Exports never use them.
fn reject_aliases_and_deep_nesting(source: &str) -> Result<(), ImportError> {
    let mut parser = Parser::new_from_str(source);
    let mut depth = 0_usize;
    loop {
        let (event, _) = parser
            .next_token()
            .map_err(|_| ImportError::new("YAML is invalid"))?;
        match event {
            Event::StreamEnd => return Ok(()),
            Event::Alias(_) => return Err(ImportError::new("YAML aliases are not supported")),
            Event::SequenceStart(..) | Event::MappingStart(..) => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(ImportError::new("YAML is nested too deeply"));
                }
            }
            Event::SequenceEnd | Event::MappingEnd => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
}

fn convert(yaml: &Yaml, depth: usize) -> Result<Value, ImportError> {
    if depth > MAX_DEPTH {
        return Err(ImportError::new("YAML is nested too deeply"));
    }
    Ok(match yaml {
        Yaml::Real(text) => text
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map_or_else(|| Value::String(text.clone()), Value::Number),
        Yaml::Integer(number) => Value::from(*number),
        Yaml::String(text) => Value::String(text.clone()),
        Yaml::Boolean(flag) => Value::Bool(*flag),
        Yaml::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| convert(item, depth + 1))
                .collect::<Result<_, _>>()?,
        ),
        Yaml::Hash(hash) => {
            let mut object = Map::new();
            for (key, value) in hash {
                let Some(key) = key_text(key) else {
                    continue;
                };
                if key == "data"
                    && let Yaml::Hash(data) = value
                {
                    let order = data
                        .keys()
                        .filter_map(key_text)
                        .map(Value::String)
                        .collect();
                    object
                        .entry("dataPropertyOrder")
                        .or_insert_with(|| serde_json::json!({ "&": Value::Array(order) }));
                }
                object.insert(key, convert(value, depth + 1)?);
            }
            Value::Object(object)
        }
        Yaml::Alias(_) => return Err(ImportError::new("YAML aliases are not supported")),
        Yaml::Null | Yaml::BadValue => Value::Null,
    })
}

fn key_text(key: &Yaml) -> Option<String> {
    match key {
        Yaml::String(text) | Yaml::Real(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        Yaml::Boolean(flag) => Some(flag.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn keeps_environment_key_order_and_scalar_types() {
        let value = parse("environments:\n  data:\n    zeta: 1\n    alpha: true\n    beta: 1.5\n")
            .expect("parse");
        assert_eq!(
            value["environments"]["dataPropertyOrder"]["&"],
            serde_json::json!(["zeta", "alpha", "beta"])
        );
        assert_eq!(value["environments"]["data"]["alpha"], true);
        assert_eq!(value["environments"]["data"]["beta"], 1.5);
    }

    #[test]
    fn rejects_aliases_and_invalid_documents() {
        assert!(parse("a: &x 1\nb: *x\n").is_err());
        assert!(parse("a: [unterminated").is_err());
    }
}
