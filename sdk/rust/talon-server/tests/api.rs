use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::Value;
use std::time::Duration;
use talon_server::{authorization_header, mint_jwt, JwtOptions, Options};

#[test]
fn options_default_has_startup_timeout() {
    assert!(Options::default().startup_timeout.as_secs() > 0);
}

#[test]
fn mint_jwt_creates_scoped_talon_token() {
    let token = mint_jwt(
        "secret",
        JwtOptions {
            subject: "browser-demo".to_string(),
            ttl: Duration::from_secs(60),
            namespace: Some("demo".to_string()),
            agent: Some("copilot".to_string()),
            session: None,
            channel: Some("chat".to_string()),
        },
    )
    .unwrap();
    let segments: Vec<&str> = token.split('.').collect();
    assert_eq!(segments.len(), 3);

    let header: Value = decode_segment(segments[0]);
    let claims: Value = decode_segment(segments[1]);
    assert_eq!(header["alg"], "HS256");
    assert_eq!(header["typ"], "JWT");
    assert_eq!(claims["sub"], "browser-demo");
    assert_eq!(claims["aud"], "talon");
    assert_eq!(claims["talon:ns"], "demo");
    assert_eq!(claims["talon:agent"], "copilot");
    assert_eq!(claims["talon:channel"], "chat");
    assert_eq!(
        authorization_header(&token).unwrap(),
        format!("Bearer {token}")
    );
}

#[test]
fn mint_jwt_requires_namespace_for_channel_scope() {
    let err = mint_jwt(
        "secret",
        JwtOptions {
            channel: Some("chat".to_string()),
            ..JwtOptions::default()
        },
    )
    .unwrap_err();
    assert!(err.to_string().contains("namespace"));
}

fn decode_segment(segment: &str) -> Value {
    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segment).unwrap()).unwrap()
}
