// uuri.rs — Universal URI: canonical address scheme for all Worlds agents can enter
// nostr://<relay>/<npub>  |  sui://<object_id>  |  freenet://<key>
// zima://<server>/<app>   |  mesh://<node_id>   |  https://<host>/<path>
// libp2p://<peer_id>      |  email://<address>

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum World {
    Nostr,
    Sui,
    Freenet,
    Zima,
    Mesh,
    Web,
    LibP2P,
    Email,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UURI {
    Nostr { relay: String, npub: String },
    Sui { object_id: String },
    Freenet { contract_key: String },
    Zima { server_id: String, app_name: String },
    Mesh { node_id: String },
    Web { url: String },
    LibP2P { peer_id: String },
    Email { address: String },
}

#[derive(Debug, Error)]
pub enum UuriError {
    #[error("empty UURI string")]
    Empty,
    #[error("unknown scheme: {0}")]
    UnknownScheme(String),
    #[error("malformed UURI: {0}")]
    Malformed(String),
}

impl UURI {
    pub fn world(&self) -> World {
        match self {
            UURI::Nostr { .. } => World::Nostr,
            UURI::Sui { .. } => World::Sui,
            UURI::Freenet { .. } => World::Freenet,
            UURI::Zima { .. } => World::Zima,
            UURI::Mesh { .. } => World::Mesh,
            UURI::Web { .. } => World::Web,
            UURI::LibP2P { .. } => World::LibP2P,
            UURI::Email { .. } => World::Email,
        }
    }

    pub fn parse(s: &str) -> Result<Self, UuriError> {
        if s.is_empty() {
            return Err(UuriError::Empty);
        }
        if let Some(rest) = s.strip_prefix("nostr://") {
            // nostr://<relay>/<npub>  OR  nostr://<npub> (relay-less)
            if let Some(slash) = rest.find('/') {
                let relay = rest[..slash].to_string();
                let npub = rest[slash + 1..].to_string();
                return Ok(UURI::Nostr { relay, npub });
            }
            return Ok(UURI::Nostr {
                relay: String::new(),
                npub: rest.to_string(),
            });
        }
        if let Some(rest) = s.strip_prefix("sui://") {
            return Ok(UURI::Sui { object_id: rest.to_string() });
        }
        if let Some(rest) = s.strip_prefix("freenet://") {
            return Ok(UURI::Freenet { contract_key: rest.to_string() });
        }
        if let Some(rest) = s.strip_prefix("zima://") {
            if let Some(slash) = rest.find('/') {
                let server_id = rest[..slash].to_string();
                let app_name = rest[slash + 1..].to_string();
                return Ok(UURI::Zima { server_id, app_name });
            }
            return Err(UuriError::Malformed(
                "zima:// requires <server>/<app>".to_string(),
            ));
        }
        if let Some(rest) = s.strip_prefix("mesh://") {
            return Ok(UURI::Mesh { node_id: rest.to_string() });
        }
        if let Some(rest) = s.strip_prefix("libp2p://") {
            return Ok(UURI::LibP2P { peer_id: rest.to_string() });
        }
        if let Some(rest) = s.strip_prefix("email://") {
            return Ok(UURI::Email { address: rest.to_string() });
        }
        if s.starts_with("https://") || s.starts_with("http://") {
            return Ok(UURI::Web { url: s.to_string() });
        }
        let scheme = s.split("://").next().unwrap_or(s);
        Err(UuriError::UnknownScheme(scheme.to_string()))
    }

    pub fn as_str(&self) -> String {
        self.to_string()
    }
}

impl std::fmt::Display for UURI {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UURI::Nostr { relay, npub } if relay.is_empty() => write!(f, "nostr://{npub}"),
            UURI::Nostr { relay, npub } => write!(f, "nostr://{relay}/{npub}"),
            UURI::Sui { object_id } => write!(f, "sui://{object_id}"),
            UURI::Freenet { contract_key } => write!(f, "freenet://{contract_key}"),
            UURI::Zima { server_id, app_name } => write!(f, "zima://{server_id}/{app_name}"),
            UURI::Mesh { node_id } => write!(f, "mesh://{node_id}"),
            UURI::Web { url } => write!(f, "{url}"),
            UURI::LibP2P { peer_id } => write!(f, "libp2p://{peer_id}"),
            UURI::Email { address } => write!(f, "email://{address}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nostr_roundtrip() {
        let u = UURI::Nostr {
            relay: "relay.damus.io".into(),
            npub: "npub1abc123".into(),
        };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn sui_roundtrip() {
        let u = UURI::Sui { object_id: "0xdeadbeef".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn zima_roundtrip() {
        let u = UURI::Zima { server_id: "my-zima".into(), app_name: "vantage".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn mesh_roundtrip() {
        let u = UURI::Mesh { node_id: "!aabbccdd".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn web_roundtrip() {
        let u = UURI::Web { url: "https://example.com/path".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn libp2p_roundtrip() {
        let u = UURI::LibP2P { peer_id: "QmAbCdEfGh".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn email_roundtrip() {
        let u = UURI::Email { address: "agent@omokoda.space".into() };
        assert_eq!(UURI::parse(&u.to_string()).unwrap(), u);
    }

    #[test]
    fn unknown_scheme_errors() {
        assert!(matches!(UURI::parse("ftp://foo"), Err(UuriError::UnknownScheme(_))));
    }

    #[test]
    fn empty_errors() {
        assert!(matches!(UURI::parse(""), Err(UuriError::Empty)));
    }

    #[test]
    fn world_mapping() {
        assert_eq!(UURI::Sui { object_id: "x".into() }.world(), World::Sui);
        assert_eq!(UURI::Mesh { node_id: "x".into() }.world(), World::Mesh);
    }
}
