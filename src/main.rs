use serde_json;
use std::{env, iter::Map};

// Available if you need it!
// use serde_bencode

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> serde_json::Value {
    let mut parser = BencodeParser::new(encoded_value);
    let value = parser.parse_value();
    parser.ensure_consumed();
    value
}

struct BencodeParser<'a> {
    input: &'a str,
    index: usize,
}

impl<'a> BencodeParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, index: 0 }
    }

    fn parse_value(&mut self) -> serde_json::Value {
        match self.peek() {
            Some('i') => self.parse_integer(),
            Some('l') => self.parse_list(),
            Some('d') => self.parse_dictionary(),
            Some(c) if c.is_ascii_digit() => self.parse_string(),
            Some(other) => panic!("Unhandled encoded prefix: {}", other),
            None => panic!("Unexpected end of input"),
        }
    }

    fn parse_string(&mut self) -> serde_json::Value {
        let colon_offset = self.remaining_slice().find(':').expect("Missing ':' in string encoding");
        let length_str = &self.remaining_slice()[..colon_offset];
        let byte_length = length_str.parse::<usize>().expect("Invalid string length");
        self.index += colon_offset + 1; // Skip length and ':'

        if self.index + byte_length > self.input.len() {
            panic!("String length exceeds input bounds");
        }

        let value = &self.input[self.index..self.index + byte_length];
        self.index += byte_length;

        serde_json::Value::String(value.to_string())
    }

    fn parse_integer(&mut self) -> serde_json::Value {
        self.expect_char('i');
        let end_offset = self.remaining_slice().find('e').expect("Missing 'e' terminator for integer");
        let number_slice = &self.remaining_slice()[..end_offset];
        let number = number_slice.parse::<i64>().expect("Invalid integer value");
        self.index += end_offset + 1; // Consume digits and terminating 'e'

        serde_json::Value::Number(serde_json::Number::from(number))
    }

    fn parse_list(&mut self) -> serde_json::Value {
        self.expect_char('l');
        let mut items = Vec::new();

        loop {
            match self.peek() {
                Some('e') => {
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
        self.expect_char('d');
        let mut items = serde_json::Map::new();

        loop {
            match self.peek() {
                Some('e') => {
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

    fn ensure_consumed(&self) {
        if self.index != self.input.len() {
            panic!("Trailing data after parsing bencoded value");
        }
    }

    fn peek(&self) -> Option<char> {
        self.remaining_slice().chars().next()
    }

    fn remaining_slice(&self) -> &str {
        &self.input[self.index..]
    }

    fn expect_char(&mut self, expected: char) {
        match self.peek() {
            Some(c) if c == expected => self.index += expected.len_utf8(),
            Some(other) => panic!("Expected '{}', found '{}'", expected, other),
            None => panic!("Expected '{}', found end of input", expected),
        }
    }
}

// Usage: your_program.sh decode "<encoded_value>"
fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];

    if command == "decode" {
        // You can use print statements as follows for debugging, they'll be visible when running tests.
        eprintln!("Logs from your program will appear here!");

        // TODO: Uncomment the code below to pass the first stage
        let encoded_value = &args[2];
        let decoded_value = decode_bencoded_value(encoded_value);
        println!("{}", decoded_value.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }
}
