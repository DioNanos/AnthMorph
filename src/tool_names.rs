use sha1::{Digest, Sha1};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct ToolNameMap {
    forward: HashMap<String, String>,
    reverse: HashMap<String, String>,
}

impl ToolNameMap {
    pub fn identity() -> Self {
        Self::default()
    }

    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>, max_len: usize) -> Self {
        let mut map = Self::default();
        for name in names {
            let backend = shorten_tool_name(name, max_len);
            map.forward.insert(name.to_string(), backend.clone());
            map.reverse.insert(backend, name.to_string());
        }
        map
    }

    pub fn to_backend<'a>(&self, name: &'a str) -> String {
        self.forward
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    pub fn to_client<'a>(&self, name: &'a str) -> String {
        self.reverse
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }
}

pub fn shorten_tool_name(name: &str, max_len: usize) -> String {
    let safe = sanitize_tool_name(name);
    if safe.len() <= max_len {
        return safe;
    }

    let mut hasher = Sha1::new();
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let hash = format!(
        "{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3]
    );
    let suffix = format!("__{hash}");
    let keep = max_len.saturating_sub(suffix.len());
    let mut prefix = safe.chars().take(keep).collect::<String>();
    if prefix.is_empty() {
        prefix = "tool".to_string();
    }
    format!("{prefix}{suffix}")
}

fn sanitize_tool_name(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaves_short_name_unchanged() {
        assert_eq!(
            shorten_tool_name("mcp__memory__read", 64),
            "mcp__memory__read"
        );
    }

    #[test]
    fn shortens_long_name_stably() {
        let input = "mcp__memory__memory_read__this_name_is_definitely_way_beyond_sixty_four";
        let one = shorten_tool_name(input, 64);
        let two = shorten_tool_name(input, 64);
        assert_eq!(one, two);
        assert!(one.len() <= 64);
        assert!(one.contains("__"));
    }
}
