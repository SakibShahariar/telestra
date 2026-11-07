//! Strict, dependency-free decoding of TDLib JSON messages.
//!
//! This module deliberately decodes only the envelope fields needed by the
//! adapter. Unknown TDLib objects remain available as their original JSON
//! payload, so adding a new TDLib update does not require a schema migration.

use std::collections::BTreeMap;

use super::{AuthorizationState, TdlibError};

/// A decoded TDLib message envelope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodedMessage {
    Response { extra: Option<String>, json: String },
    Error { extra: Option<String>, error: TdlibError },
    Update { json: String },
    AuthorizationStateChanged(AuthorizationState),
}

/// Errors encountered while decoding a TDLib JSON message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodeError {
    offset: usize,
    message: &'static str,
}

impl DecodeError {
    /// Byte offset at which decoding failed.
    pub const fn offset(&self) -> usize { self.offset }

    /// Stable diagnostic for the decoding failure.
    pub const fn message(&self) -> &'static str { self.message }
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} at byte {}", self.message, self.offset)
    }
}

impl std::error::Error for DecodeError {}

/// Decodes one complete TDLib JSON object without discarding unknown fields.
pub fn decode_message(json: &str) -> Result<DecodedMessage, DecodeError> {
    let mut parser = Parser { input: json.as_bytes(), offset: 0 };
    let value = parser.parse_value(0)?;
    parser.skip_whitespace();
    if parser.offset != parser.input.len() {
        return Err(parser.error("trailing data after JSON value"));
    }
    let object = match value {
        Value::Object(object) => object,
        _ => return Err(parser.error("TDLib message must be a JSON object")),
    };
    let kind = object.get("@type").and_then(Value::as_str)
        .ok_or_else(|| parser.error("TDLib message is missing string @type"))?;
    let extra = object.get("@extra").and_then(Value::as_str).map(str::to_owned);

    if kind == "error" {
        let code = object.get("code").and_then(Value::as_i64)
            .and_then(|value| i32::try_from(value).ok())
            .ok_or_else(|| parser.error("TDLib error is missing a valid integer code"))?;
        let message = object.get("message").and_then(Value::as_str)
            .ok_or_else(|| parser.error("TDLib error is missing string message"))?;
        return Ok(DecodedMessage::Error {
            extra,
            error: TdlibError::new(code, message),
        });
    }

    if kind == "updateAuthorizationState" {
        let state = object.get("authorization_state")
            .and_then(Value::as_object)
            .and_then(|state| state.get("@type"))
            .and_then(Value::as_str)
            .ok_or_else(|| parser.error("authorization update is missing authorization_state.@type"))?;
        return Ok(DecodedMessage::AuthorizationStateChanged(decode_authorization_state(state)));
    }

    if kind.starts_with("update") {
        return Ok(DecodedMessage::Update { json: json.to_owned() });
    }

    Ok(DecodedMessage::Response { extra, json: json.to_owned() })
}

fn decode_authorization_state(state: &str) -> AuthorizationState {
    match state {
        "authorizationStateWaitTdlibParameters" => AuthorizationState::WaitTdlibParameters,
        "authorizationStateWaitEncryptionKey" => AuthorizationState::WaitEncryptionKey,
        "authorizationStateWaitPhoneNumber" => AuthorizationState::WaitPhoneNumber,
        "authorizationStateWaitCode" => AuthorizationState::WaitCode,
        "authorizationStateWaitPassword" => AuthorizationState::WaitPassword,
        "authorizationStateReady" => AuthorizationState::Ready,
        "authorizationStateLoggingOut" => AuthorizationState::LoggingOut,
        "authorizationStateClosing" => AuthorizationState::Closing,
        "authorizationStateClosed" => AuthorizationState::Closed,
        other => AuthorizationState::Unknown(other.to_owned()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Value {
    Null,
    Bool,
    Number(Option<i64>),
    String(String),
    Array,
    Object(BTreeMap<String, Value>),
}

impl Value {
    fn as_str(&self) -> Option<&str> {
        if let Self::String(value) = self { Some(value) } else { None }
    }

    fn as_i64(&self) -> Option<i64> {
        if let Self::Number(value) = self { *value } else { None }
    }

    fn as_object(&self) -> Option<&BTreeMap<String, Value>> {
        if let Self::Object(value) = self { Some(value) } else { None }
    }
}

struct Parser<'a> {
    input: &'a [u8],
    offset: usize,
}

impl Parser<'_> {
    fn error(&self, message: &'static str) -> DecodeError {
        DecodeError { offset: self.offset, message }
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.input.get(self.offset), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.offset += 1;
        }
    }

    fn consume(&mut self, expected: u8) -> Result<(), DecodeError> {
        self.skip_whitespace();
        if self.input.get(self.offset) == Some(&expected) {
            self.offset += 1;
            Ok(())
        } else {
            Err(self.error("unexpected JSON token"))
        }
    }

    fn parse_value(&mut self, depth: usize) -> Result<Value, DecodeError> {
        if depth > 128 { return Err(self.error("JSON nesting limit exceeded")); }
        self.skip_whitespace();
        match self.input.get(self.offset).copied() {
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b'{') => self.parse_object(depth + 1),
            Some(b'[') => self.parse_array(depth + 1),
            Some(b't') => { self.parse_literal(b"true")?; Ok(Value::Bool) }
            Some(b'f') => { self.parse_literal(b"false")?; Ok(Value::Bool) }
            Some(b'n') => { self.parse_literal(b"null")?; Ok(Value::Null) }
            Some(b'-' | b'0'..=b'9') => self.parse_number(),
            _ => Err(self.error("invalid or incomplete JSON value")),
        }
    }

    fn parse_literal(&mut self, literal: &[u8]) -> Result<(), DecodeError> {
        let end = self.offset.saturating_add(literal.len());
        if self.input.get(self.offset..end) == Some(literal) {
            self.offset = end;
            Ok(())
        } else {
            Err(self.error("invalid JSON literal"))
        }
    }

    fn parse_string(&mut self) -> Result<String, DecodeError> {
        self.consume(b'"')?;
        let mut output = String::new();
        let mut segment_start = self.offset;
        loop {
            let byte = *self.input.get(self.offset).ok_or_else(|| self.error("unterminated JSON string"))?;
            match byte {
                b'"' => {
                    let segment = std::str::from_utf8(&self.input[segment_start..self.offset])
                        .map_err(|_| self.error("invalid UTF-8 in JSON string"))?;
                    output.push_str(segment);
                    self.offset += 1;
                    return Ok(output);
                }
                b'\\' => {
                    let segment = std::str::from_utf8(&self.input[segment_start..self.offset])
                        .map_err(|_| self.error("invalid UTF-8 in JSON string"))?;
                    output.push_str(segment);
                    self.offset += 1;
                    let escaped = *self.input.get(self.offset).ok_or_else(|| self.error("incomplete JSON escape"))?;
                    self.offset += 1;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{0008}'),
                        b'f' => output.push('\u{000c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => {
                            let first = self.parse_hex_quad()?;
                            let scalar = if (0xD800..=0xDBFF).contains(&first) {
                                if self.input.get(self.offset..self.offset + 2) != Some(b"\\u") {
                                    return Err(self.error("missing low surrogate in JSON escape"));
                                }
                                self.offset += 2;
                                let second = self.parse_hex_quad()?;
                                if !(0xDC00..=0xDFFF).contains(&second) {
                                    return Err(self.error("invalid low surrogate in JSON escape"));
                                }
                                0x10000 + (((first - 0xD800) as u32) << 10) + (second - 0xDC00) as u32
                            } else if (0xDC00..=0xDFFF).contains(&first) {
                                return Err(self.error("unexpected low surrogate in JSON escape"));
                            } else { first as u32 };
                            output.push(char::from_u32(scalar).ok_or_else(|| self.error("invalid Unicode scalar"))?);
                        }
                        _ => return Err(self.error("invalid JSON string escape")),
                    }
                    segment_start = self.offset;
                }
                0x00..=0x1F => return Err(self.error("unescaped control character in JSON string")),
                _ => self.offset += 1,
            }
        }
    }

    fn parse_hex_quad(&mut self) -> Result<u16, DecodeError> {
        let end = self.offset.saturating_add(4);
        let digits = self.input.get(self.offset..end).ok_or_else(|| self.error("incomplete Unicode escape"))?;
        let mut value = 0u16;
        for digit in digits {
            let nibble = match digit {
                b'0'..=b'9' => digit - b'0',
                b'a'..=b'f' => digit - b'a' + 10,
                b'A'..=b'F' => digit - b'A' + 10,
                _ => return Err(self.error("invalid Unicode escape")),
            };
            value = (value << 4) | u16::from(nibble);
        }
        self.offset = end;
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<Value, DecodeError> {
        let start = self.offset;
        if self.input.get(self.offset) == Some(&b'-') { self.offset += 1; }
        match self.input.get(self.offset) {
            Some(b'0') => self.offset += 1,
            Some(b'1'..=b'9') => {
                self.offset += 1;
                while matches!(self.input.get(self.offset), Some(b'0'..=b'9')) { self.offset += 1; }
            }
            _ => return Err(self.error("invalid JSON number")),
        }
        if self.input.get(self.offset) == Some(&b'.') {
            self.offset += 1;
            let fraction_start = self.offset;
            while matches!(self.input.get(self.offset), Some(b'0'..=b'9')) { self.offset += 1; }
            if self.offset == fraction_start { return Err(self.error("invalid JSON fraction")); }
        }
        if matches!(self.input.get(self.offset), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.input.get(self.offset), Some(b'+' | b'-')) { self.offset += 1; }
            let exponent_start = self.offset;
            while matches!(self.input.get(self.offset), Some(b'0'..=b'9')) { self.offset += 1; }
            if self.offset == exponent_start { return Err(self.error("invalid JSON exponent")); }
        }
        let number = std::str::from_utf8(&self.input[start..self.offset])
            .ok().and_then(|text| text.parse::<i64>().ok());
        Ok(Value::Number(number))
    }

    fn parse_array(&mut self, depth: usize) -> Result<Value, DecodeError> {
        self.consume(b'[')?;
        self.skip_whitespace();
        if self.input.get(self.offset) == Some(&b']') { self.offset += 1; return Ok(Value::Array); }
        loop {
            self.parse_value(depth)?;
            self.skip_whitespace();
            match self.input.get(self.offset) {
                Some(b',') => self.offset += 1,
                Some(b']') => { self.offset += 1; return Ok(Value::Array); }
                _ => return Err(self.error("expected comma or closing array bracket")),
            }
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<Value, DecodeError> {
        self.consume(b'{')?;
        let mut object = BTreeMap::new();
        self.skip_whitespace();
        if self.input.get(self.offset) == Some(&b'}') { self.offset += 1; return Ok(Value::Object(object)); }
        loop {
            self.skip_whitespace();
            if self.input.get(self.offset) != Some(&b'"') { return Err(self.error("object key must be a string")); }
            let key = self.parse_string()?;
            self.consume(b':')?;
            let value = self.parse_value(depth)?;
            if object.insert(key, value).is_some() { return Err(self.error("duplicate JSON object key")); }
            self.skip_whitespace();
            match self.input.get(self.offset) {
                Some(b',') => self.offset += 1,
                Some(b'}') => { self.offset += 1; return Ok(Value::Object(object)); }
                _ => return Err(self.error("expected comma or closing object brace")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_message, DecodedMessage};
    use crate::telegram::{AuthorizationState, TdlibError};

    #[test]
    fn decodes_correlated_success_response_and_preserves_payload() {
        let json = r#"{"@type":"ok","@extra":"telestra:7","nested":{"text":"a \"quote\""} }"#;
        assert_eq!(decode_message(json), Ok(DecodedMessage::Response {
            extra: Some("telestra:7".to_owned()), json: json.to_owned()
        }));
    }

    #[test]
    fn decodes_error_code_message_and_correlation() {
        assert_eq!(decode_message(r#"{"@type":"error","code":401,"message":"Unauthorized","@extra":"telestra:8"}"#),
            Ok(DecodedMessage::Error { extra: Some("telestra:8".to_owned()), error: TdlibError::new(401, "Unauthorized") }));
    }

    #[test]
    fn decodes_authorization_update_and_future_states() {
        assert_eq!(decode_message(r#"{"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateReady"}}"#),
            Ok(DecodedMessage::AuthorizationStateChanged(AuthorizationState::Ready)));
        assert_eq!(decode_message(r#"{"@type":"updateAuthorizationState","authorization_state":{"@type":"authorizationStateFuture"}}"#),
            Ok(DecodedMessage::AuthorizationStateChanged(AuthorizationState::Unknown("authorizationStateFuture".to_owned()))));
    }

    #[test]
    fn preserves_unknown_update_payloads() {
        let json = r#"{"@type":"updateNewChat","chat":{"id":12}}"#;
        assert_eq!(decode_message(json), Ok(DecodedMessage::Update { json: json.to_owned() }));
    }

    #[test]
    fn rejects_malformed_non_object_and_trailing_json() {
        assert!(decode_message("{").is_err());
        assert!(decode_message("[]").is_err());
        assert!(decode_message(r#"{"@type":"ok"} false"#).is_err());
    }

    #[test]
    fn rejects_invalid_error_fields_and_duplicate_keys() {
        assert!(decode_message(r#"{"@type":"error","code":"401","message":"bad"}"#).is_err());
        assert!(decode_message(r#"{"@type":"ok","@type":"error"}"#).is_err());
    }

    #[test]
    fn parses_escaped_unicode_and_nested_values() {
        let json = r#"{"@type":"ok","text":"\uD83D\uDE80","items":[true,null,{"n":-4.25e2}]}"#;
        assert_eq!(decode_message(json), Ok(DecodedMessage::Response { extra: None, json: json.to_owned() }));
    }
}
