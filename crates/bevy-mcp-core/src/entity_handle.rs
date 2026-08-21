use serde::{Deserialize, Serialize};
use std::fmt;

/// Opaque handle to a Bevy entity, generation-checked.
///
/// Format: `entity://<instance>/<world>/<id>/<generation>`
///
/// The generation ensures a stale handle doesn't accidentally refer
/// to a newly recycled entity slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityHandle {
    pub instance: String,
    pub world: String,
    pub id: u64,
    pub generation: u64,
}

impl EntityHandle {
    pub fn new(
        instance: impl Into<String>,
        world: impl Into<String>,
        id: u64,
        generation: u64,
    ) -> Self {
        Self {
            instance: instance.into(),
            world: world.into(),
            id,
            generation,
        }
    }

    pub fn to_uri(&self) -> String {
        format!(
            "entity://{}/{}/{}/{}",
            self.instance, self.world, self.id, self.generation
        )
    }

    pub fn from_uri(uri: &str) -> Result<Self, String> {
        let rest = uri
            .strip_prefix("entity://")
            .ok_or("missing entity:// prefix")?;

        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() != 4 {
            return Err(format!(
                "expected 4 parts (instance/world/id/gen), got {}",
                parts.len()
            ));
        }

        Ok(Self {
            instance: parts[0].to_string(),
            world: parts[1].to_string(),
            id: parts[2].parse().map_err(|e| format!("invalid id: {e}"))?,
            generation: parts[3]
                .parse()
                .map_err(|e| format!("invalid generation: {e}"))?,
        })
    }
}

impl fmt::Display for EntityHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_uri())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_uri() {
        let handle = EntityHandle::new("default", "main", 143, 6);
        let uri = handle.to_uri();
        assert_eq!(uri, "entity://default/main/143/6");

        let parsed = EntityHandle::from_uri(&uri).unwrap();
        assert_eq!(handle, parsed);
    }

    #[test]
    fn reject_bad_uri() {
        assert!(EntityHandle::from_uri("not-a-uri").is_err());
        assert!(EntityHandle::from_uri("entity://a/b").is_err());
        assert!(EntityHandle::from_uri("entity://a/b/c/d").is_err());
    }
}
