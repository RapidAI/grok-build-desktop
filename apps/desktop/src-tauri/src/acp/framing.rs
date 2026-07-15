//! Newline-delimited JSON-RPC framing for ACP stdio.

use serde_json::Value;

pub fn encode_line(value: &Value) -> String {
    let mut s = value.to_string();
    s.push('\n');
    s
}

pub fn decode_line(line: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(line.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn encode_adds_single_newline() {
        let line = encode_line(&json!({"jsonrpc":"2.0","id":1,"method":"initialize"}));
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1);
    }

    #[test]
    fn decode_roundtrip() {
        let v = json!({"jsonrpc":"2.0","id":1,"result":{"ok":true}});
        let line = encode_line(&v);
        let back = decode_line(&line).unwrap();
        assert_eq!(back["result"]["ok"], true);
    }
}
