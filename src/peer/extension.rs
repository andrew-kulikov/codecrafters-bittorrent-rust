use serde::{Deserialize, Serialize};
use serde_bencode;
use std::collections::BTreeMap;

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

pub struct ExtensionMessage {
    // Id of particular extension received on handshake
    pub msg_id: u8,
    pub payload: Vec<u8>,
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
        let extensions = self
            .extensions
            .iter()
            .map(|(name, id)| (name.clone(), *id))
            .collect::<BTreeMap<_, _>>();

        let payload = ExtensionHandshakeSerde {
            extensions,
            metadata_size: self.metadata_size,
            client_name: self.client_name.clone(),
        };

        Ok(serde_bencode::to_bytes(&payload)?)
    }

    pub fn decode(bytes: &[u8]) -> anyhow::Result<Self> {
        let payload: ExtensionHandshakeSerde = serde_bencode::from_bytes(bytes)?;

        let extensions = payload.extensions.into_iter().collect::<Vec<_>>();

        Ok(ExtensionHandshakePayload {
            extensions,
            metadata_size: payload.metadata_size,
            client_name: payload.client_name,
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

impl ExtensionMessage {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + self.payload.len());
        buf.push(self.msg_id as u8);
        buf.extend_from_slice(&self.payload);
        buf
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ExtensionHandshakeSerde {
    #[serde(rename = "m")]
    extensions: BTreeMap<String, u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    client_name: Option<String>,
}
