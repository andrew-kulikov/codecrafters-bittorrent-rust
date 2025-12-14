use serde_json::{json, Value};

use crate::bencode;

pub const METADATA_EXTENSION_NAME: &str = "ut_metadata";
pub const METADATA_EXTENSION_ID: u8 = 17;

/// Placeholder for incoming/outgoing BEP-10 extension handshake payload.
/// See: https://www.bittorrent.org/beps/bep_0010.html
/// TODO: Implement encode/decode when enabling extensions.
#[derive(Debug, Clone)]
pub struct ExtensionHandshakePayload {
    /// Map of extension name -> extension message ID ("m" in spec)
    pub extensions: Vec<(String, u8)>,
    /// Optional metadata size, client name, etc.
    pub metadata_size: Option<u64>,
    pub client_name: Option<String>,
}

impl ExtensionHandshakePayload {
    pub fn new() -> Self {
        Self {
            extensions: Vec::new(),
            metadata_size: None,
            client_name: None,
        }
    }

    pub fn default_extensions() -> ExtensionHandshakePayload {
        ExtensionHandshakePayload {
            extensions: vec![(METADATA_EXTENSION_NAME.to_string(), METADATA_EXTENSION_ID)],
            metadata_size: None,
            client_name: None,
        }
    }

    pub fn encode(&self) -> anyhow::Result<Vec<u8>> {
        let extensions_dict = self
            .extensions
            .iter()
            .map(|(key, val)| (key.to_owned(), Value::Number((*val as i64).into())))
            .collect::<serde_json::Map<_, _>>();

        let json = json!({
            "m": extensions_dict
        });
        let encoded = bencode::encode(&json)?;
        Ok(encoded)
    }

    pub fn decode(_bytes: &[u8]) -> anyhow::Result<Self> {
        let json = bencode::parse_bytes(_bytes.to_vec());

        let extensions = if let Some(m_dict) = json.get("m").and_then(|v| v.as_object()) {
            m_dict
                .iter()
                .filter_map(|(key, val)| val.as_i64().map(|num| (key.clone(), num as u8)))
                .collect::<Vec<(String, u8)>>()
        } else {
            Vec::new()
        };

        let metadata_size = json.get("metadata_size").and_then(|v| v.as_u64());

        let client_name = json
            .get("client_name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(ExtensionHandshakePayload {
            extensions,
            metadata_size,
            client_name,
        })
    }

    pub fn get_metadata_extension_id(&self) -> Option<u8> {
        for (name, ext_id) in &self.extensions {
            if name == METADATA_EXTENSION_NAME {
                return Some(*ext_id);
            }
        }
        None
    }
}
