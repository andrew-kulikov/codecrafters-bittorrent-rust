use serde_json;

use crate::utils::{RawBytesExt, RawStringExt};

pub struct BencodeParser {
    input: Vec<u8>,
    index: usize,
}

pub fn parse_string(encoded_value: &str) -> serde_json::Value {
    let mut parser = BencodeParser::new(encoded_value.as_bytes().to_vec());
    let value = parser.parse_value();
    parser.ensure_consumed();
    value
}

pub fn parse_bytes(encoded_value: Vec<u8>) -> serde_json::Value {
    let mut parser = BencodeParser::new(encoded_value);
    let value = parser.parse_value();
    parser.ensure_consumed();
    value
}

pub fn encode(data: &serde_json::Value) -> anyhow::Result<Vec<u8>> {
    match data {
        serde_json::Value::Null => Ok(b"le".to_vec()),
        serde_json::Value::Bool(true) => Ok(b"i1e".to_vec()),
        serde_json::Value::Bool(false) => Ok(b"i0e".to_vec()),
        serde_json::Value::Number(num) => {
            if let Some(n) = num.as_i64() {
                Ok(format!("i{}e", n).into_bytes())
            } else {
                Err(anyhow::anyhow!("Only integer numbers are supported in bencode"))
            }
        }
        serde_json::Value::String(s) => {
            let mut bytes = format!("{}:", s.chars().count()).as_bytes().to_vec();
            bytes.extend_from_slice(s.to_raw_bytes().as_slice());
            Ok(bytes)
        }
        serde_json::Value::Array(arr) => {
            let mut encoded = vec![b'l'];
            for item in arr {
                encoded.extend(encode(item)?);
            }
            encoded.push(b'e');
            Ok(encoded)
        }
        serde_json::Value::Object(map) => {
            let mut encoded = vec![b'd'];
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();

            for key in keys {
                let value = &map[key];
                encoded.extend(encode(&serde_json::Value::String(key.clone()))?);
                encoded.extend(encode(value)?);
            }
            encoded.push(b'e');
            Ok(encoded)
        }
    }
}

impl BencodeParser {
    pub fn new(input: Vec<u8>) -> Self {
        Self { input, index: 0 }
    }

    pub fn parse_value(&mut self) -> serde_json::Value {
        match self.peek() {
            Some(b'i') => self.parse_integer(),
            Some(b'l') => self.parse_list(),
            Some(b'd') => self.parse_dictionary(),
            Some(c) if c.is_ascii_digit() => self.parse_string(),
            Some(other) => panic!("Unhandled encoded prefix: {}", other as char),
            None => panic!("Unexpected end of input"),
        }
    }

    fn parse_string(&mut self) -> serde_json::Value {
        let slice = self.remaining_slice();
        let colon_offset = slice
            .iter()
            .position(|&b| b == b':')
            .expect("Missing ':' in string encoding");
        let length_str =
            std::str::from_utf8(&slice[..colon_offset]).expect("Invalid UTF-8 in string length");
        let byte_length = length_str.parse::<usize>().expect("Invalid string length");
        self.index += colon_offset + 1; // Skip length and ':'

        if self.index + byte_length > self.input.len() {
            panic!("String length exceeds input bounds");
        }

        let end = self.index + byte_length;
        let value = &self.input[self.index..end];
        self.index = end;

        // Use 1:1 byte-to-char mapping to preserve raw bytes
        let result = serde_json::Value::String(value.to_raw_string());
        result
    }

    fn parse_integer(&mut self) -> serde_json::Value {
        self.expect_byte(b'i');
        let slice = self.remaining_slice();
        let end_offset = slice
            .iter()
            .position(|&b| b == b'e')
            .expect("Missing 'e' terminator for integer");
        let number_slice =
            std::str::from_utf8(&slice[..end_offset]).expect("Invalid UTF-8 in integer value");
        let number = number_slice.parse::<i64>().expect("Invalid integer value");
        self.index += end_offset + 1; // Consume digits and terminating 'e'

        serde_json::Value::Number(serde_json::Number::from(number))
    }

    fn parse_list(&mut self) -> serde_json::Value {
        self.expect_byte(b'l');
        let mut items = Vec::new();

        loop {
            match self.peek() {
                Some(b'e') => {
                    self.index += 1; // consume list terminator
                    break;
                }
                Some(_) => items.push(self.parse_value()),
                None => panic!("Unterminated list"),
            }
        }

        serde_json::Value::Array(items)
    }

    fn parse_dictionary(&mut self) -> serde_json::Value {
        self.expect_byte(b'd');
        let mut items = serde_json::Map::new();

        loop {
            match self.peek() {
                Some(b'e') => {
                    self.index += 1; // consume dictionary terminator
                    break;
                }
                Some(_) => {
                    let key = match self.parse_value() {
                        serde_json::Value::String(string_key) => string_key,
                        _ => panic!("Dictionary keys must be strings"),
                    };
                    let value = self.parse_value();
                    items.insert(key, value);
                }
                None => panic!("Unterminated dictionary"),
            }
        }

        serde_json::Value::Object(items)
    }

    pub fn ensure_consumed(&self) {
        if self.index != self.input.len() {
            panic!("Trailing data after parsing bencoded value");
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.index).copied()
    }

    fn remaining_slice(&self) -> &[u8] {
        &self.input[self.index..]
    }

    fn expect_byte(&mut self, expected: u8) {
        match self.peek() {
            Some(c) if c == expected => self.index += 1,
            Some(other) => panic!("Expected '{}', found '{}'", expected as char, other as char),
            None => panic!("Expected '{}', found end of input", expected as char),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn decode_parses_simple_string() {
        assert_eq!(parse_string("5:hello"), json!("hello"));
    }

    #[test]
    fn decode_parses_negative_integer() {
        assert_eq!(parse_string("i-42e"), json!(-42));
    }

    #[test]
    fn decode_parses_mixed_list() {
        assert_eq!(parse_string("l5:helloi42ee"), json!(["hello", 42]));
    }

    #[test]
    fn decode_parses_dictionary_with_multiple_value_types() {
        assert_eq!(
            parse_string("d3:bar4:spam3:fooi42ee"),
            json!({"bar": "spam", "foo": 42})
        );
    }

    #[test]
    fn decode_parses_nested_structures() {
        assert_eq!(
            parse_string("d4:listl4:spam4:eggse4:nestd3:key5:valueee"),
            json!({
                "list": ["spam", "eggs"],
                "nest": {"key": "value"}
            })
        );
    }

    #[test]
    #[should_panic(expected = "Dictionary keys must be strings")]
    fn decode_dictionary_requires_string_keys() {
        let mut parser = BencodeParser::new(b"di1ei1ee".to_vec());
        parser.parse_value();
    }

    #[test]
    #[should_panic(expected = "Trailing data after parsing bencoded value")]
    fn decode_ensure_consumed_detects_trailing_data() {
        let mut parser = BencodeParser::new(b"5:helloi1e".to_vec());
        parser.parse_value();
        parser.ensure_consumed();
    }

    #[test]
    fn encodes_int() {
        let value = json!(42);
        let encoded = encode(&value).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "i42e");
    }

    #[test]
    fn encodes_bool_true() {
        let value = json!(true);
        let encoded = encode(&value).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "i1e");
    }

    #[test]
    fn encodes_bool_false() {
        let value = json!(false);
        let encoded = encode(&value).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "i0e");
    }

    #[test]
    fn encodes_list() {
        let value = json!([1, "two", 3]);
        let encoded = encode(&value).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "li1e3:twoi3ee");
    }

    #[test]
    fn encodes_dictionary() {
        let value = json!({"age": 30, "name": "Alice"});
        let encoded = encode(&value).unwrap();
        assert_eq!(
            String::from_utf8(encoded).unwrap(),
            "d3:agei30e4:name5:Alicee"
        );
    }

    #[test]
    fn encodes_ascii_string_using_original_text() {
        let value = json!("hello");
        let encoded = encode(&value).unwrap();
        assert_eq!(String::from_utf8(encoded).unwrap(), "5:hello");
    }

    #[test]
    fn encodes_multibyte_string_using_character_count() {
        let original = "éü😊";
        // Simulate 1:1 byte-to-char mapping (Strategy B)
        let s: String = original.as_bytes().to_raw_string();
        let value = serde_json::Value::String(s);
        let encoded = encode(&value);

        // Expect correct Bencode: length prefix (bytes) + raw bytes
        let expected_len = original.len();
        let mut expected = format!("{}:", expected_len).into_bytes();
        expected.extend_from_slice(original.as_bytes());

        assert_eq!(encoded.unwrap(), expected);
    }
}
