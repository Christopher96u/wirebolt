//! Foundation-compatible request JSON, with bounded, iterative container parsing.
use serde_json::{Map, Value};

#[derive(Clone, Copy)]
enum State {
    First,
    Key,
    Value,
    Separator,
}
struct Container {
    value: Value,
    state: State,
    key: Option<String>,
}
impl Container {
    fn append(&mut self, value: Value) {
        match &mut self.value {
            Value::Array(values) => values.push(value),
            Value::Object(values) => {
                values.insert(self.key.take().expect("object key precedes value"), value);
            }
            _ => unreachable!("container"),
        }
        self.state = State::Separator;
    }
    fn closing(&self) -> u8 {
        if self.value.is_object() { b'}' } else { b']' }
    }
}

pub(crate) fn parse(source: &str) -> Result<Value, ()> {
    let bytes = super::without_trailing_commas(source);
    let mut input = Input(bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(&bytes));
    let mut containers: Vec<Container> = Vec::new();
    let mut root = None;
    loop {
        input.whitespace();
        if let Some(container) = containers.last_mut() {
            match container.state {
                State::First | State::Separator
                    if input.0.first() == Some(&container.closing()) =>
                {
                    input.0 = &input.0[1..];
                    let value = containers.pop().expect("open container").value;
                    append(&mut containers, &mut root, value);
                    continue;
                }
                State::Separator => {
                    input.expect(b',')?;
                    container.state = if container.value.is_object() {
                        State::Key
                    } else {
                        State::Value
                    };
                    continue;
                }
                State::Key | State::First if container.value.is_object() => {
                    container.key = Some(input.string()?);
                    input.whitespace();
                    input.expect(b':')?;
                    container.state = State::Value;
                    continue;
                }
                _ => (),
            }
        } else if let Some(root) = root {
            return if input.0.is_empty() {
                Ok(root)
            } else {
                Err(())
            };
        }
        match input.0.first() {
            Some(b'{' | b'[') => {
                if containers.len() >= 512 {
                    return Err(());
                }
                let object = input.0[0] == b'{';
                input.0 = &input.0[1..];
                containers.push(Container {
                    value: if object {
                        Value::Object(Map::new())
                    } else {
                        Value::Array(vec![])
                    },
                    state: State::First,
                    key: None,
                });
            }
            _ => append(&mut containers, &mut root, input.scalar()?),
        }
    }
}
fn append(containers: &mut [Container], root: &mut Option<Value>, value: Value) {
    if let Some(container) = containers.last_mut() {
        container.append(value);
    } else {
        *root = Some(value);
    }
}
struct Input<'a>(&'a [u8]);
impl Input<'_> {
    fn whitespace(&mut self) {
        let length = self.0.iter().take_while(|b| b" \t\r\n".contains(b)).count();
        self.0 = &self.0[length..];
    }
    fn expect(&mut self, byte: u8) -> Result<(), ()> {
        if self.0.first() != Some(&byte) {
            return Err(());
        }
        self.0 = &self.0[1..];
        Ok(())
    }
    fn string(&mut self) -> Result<String, ()> {
        if self.0.first() != Some(&b'"') {
            return Err(());
        }
        let mut escaped = false;
        for (index, &byte) in self.0.iter().enumerate().skip(1) {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                let value = serde_json::from_slice(&self.0[..=index]).map_err(|_| ())?;
                self.0 = &self.0[index + 1..];
                return Ok(value);
            }
        }
        Err(())
    }
    fn scalar(&mut self) -> Result<Value, ()> {
        if self.0.first() == Some(&b'"') {
            return self.string().map(Value::String);
        }
        let length = self
            .0
            .iter()
            .take_while(|b| !b",]} \t\r\n".contains(b))
            .count();
        let token = &self.0[..length];
        let value: Value = serde_json::from_slice(token).map_err(|_| ())?;
        if value.is_number()
            && super::number::format(std::str::from_utf8(token).map_err(|_| ())?).is_none()
        {
            return Err(());
        }
        self.0 = &self.0[length..];
        Ok(value)
    }
}
