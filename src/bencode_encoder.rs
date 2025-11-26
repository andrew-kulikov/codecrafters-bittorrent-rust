use serde_json;

use crate::utils;

pub fn encode(data: &serde_json::Value) -> Vec<u8> {
    match data {
        serde_json::Value::Null => b"le".to_vec(),
        serde_json::Value::Bool(true) => b"i1e".to_vec(),
        serde_json::Value::Bool(false) => b"i0e".to_vec(),
        serde_json::Value::Number(num) => {
            if let Some(n) = num.as_i64() {
                format!("i{}e", n).into_bytes()
            } else {
                panic!("Only integer numbers are supported in bencode");
            }
        }
        serde_json::Value::String(s) => {
            let mut bytes = format!("{}:", s.chars().count()).as_bytes().to_vec();
            bytes.extend_from_slice(utils::raw_string_to_bytes(s).as_slice());
            bytes
        }
        serde_json::Value::Array(arr) => {
            let mut encoded = vec![b'l'];
            for item in arr {
                encoded.extend(encode(item));
            }
            encoded.push(b'e');
            encoded
        }
        serde_json::Value::Object(map) => {
            let mut encoded = vec![b'd'];
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();

            for key in keys {
                let value = &map[key];
                encoded.extend(encode(&serde_json::Value::String(key.clone())));
                encoded.extend(encode(value));
            }
            encoded.push(b'e');
            encoded
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encodes_int() {
        let value = json!(42);
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "i42e");
    }

    #[test]
    fn encodes_bool_true() {
        let value = json!(true);
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "i1e");
    }

    #[test]
    fn encodes_bool_false() {
        let value = json!(false);
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "i0e");
    }

    #[test]
    fn encodes_list() {
        let value = json!([1, "two", 3]);
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "li1e3:twoi3ee");
    }

    #[test]
    fn encodes_dictionary() {
        let value = json!({"age": 30, "name": "Alice"});
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "d3:agei30e4:name5:Alicee");
    }

    #[test]
    fn encodes_ascii_string_using_original_text() {
        let value = json!("hello");
        let encoded = encode(&value);
        assert_eq!(String::from_utf8(encoded).unwrap(), "5:hello");
    }

    #[test]
    fn encodes_multibyte_string_using_character_count() {
        let original = "éü😊";
        // Simulate 1:1 byte-to-char mapping (Strategy B)
        let s: String = utils::bytes_to_raw_string(original.as_bytes());
        let value = serde_json::Value::String(s);
        let encoded = encode(&value);

        // Expect correct Bencode: length prefix (bytes) + raw bytes
        let expected_len = original.len();
        let mut expected = format!("{}:", expected_len).into_bytes();
        expected.extend_from_slice(original.as_bytes());

        assert_eq!(encoded, expected);
    }
}
