use super::*;

fn dump<'a>(
    status: u16,
    ct: Option<&'a str>,
    final_url: &'a str,
    body: &'a str,
) -> ResponseDump<'a> {
    ResponseDump {
        method: "GET",
        url: "https://ctf.example.com/api/v1/users/me",
        final_url,
        status: reqwest::StatusCode::from_u16(status).unwrap(),
        content_type: ct,
        body,
    }
}

#[test]
fn renders_html_body_in_frame_with_redirect_and_hint() {
    let html = "<!DOCTYPE html>\n<html>\n<title>Login</title>\n</html>";
    let out = dump(
        200,
        Some("text/html"),
        "https://ctf.example.com/login?next=x",
        html,
    )
    .render("Headline");
    assert!(out.starts_with("Headline\n"));
    assert!(out.contains("Request:  GET https://ctf.example.com/api/v1/users/me"));
    assert!(out.contains("Redirect: -> https://ctf.example.com/login?next=x"));
    assert!(out.contains("HTTP 200 OK, text/html"));
    assert!(out.contains("╭─ response body"));
    assert!(out.contains("│ <!DOCTYPE html>"));
    assert!(out.contains("Hint: create a new token"));
}

#[test]
fn pretty_prints_json_and_truncates() {
    let body = r#"{"message":"Your access token is invalid"}"#;
    let url = "https://ctf.example.com/api/v1/users/me";
    let out = dump(401, Some("application/json"), url, body).render("H");
    assert!(out.contains("│   \"message\": \"Your access token is invalid\""));
    assert!(!out.contains("Redirect:"));

    let long: String = (0..50).map(|i| format!("line {i}\n")).collect();
    let out = dump(200, None, url, &long).render("H");
    assert!(out.contains("(first 20 of 50 lines)"));
    assert!(out.contains("│ line 19"));
    assert!(!out.contains("│ line 20"));
}

#[test]
fn classifies_ctf_state_refusals() {
    let (k, h) = classify_denial(403, Some("Hack N' Flag 2026 has not started yet"), false);
    assert_eq!(k, DenyKind::NotStarted);
    assert_eq!(h, "Hack N' Flag 2026 has not started yet.");
    assert_eq!(
        classify_denial(403, Some("MyCTF has ended"), false).0,
        DenyKind::Ended
    );
    assert_eq!(
        classify_denial(403, Some("MyCTF is paused"), false).0,
        DenyKind::Paused
    );
    assert_eq!(
        classify_denial(401, Some("Your access token is invalid"), false).0,
        DenyKind::BadToken
    );
    assert_eq!(
        classify_denial(403, Some("Your access token has expired"), false).0,
        DenyKind::BadToken
    );
    assert_eq!(classify_denial(403, None, false).0, DenyKind::Forbidden);
    assert_eq!(classify_denial(200, None, true).0, DenyKind::BadToken);
}

#[test]
fn extracts_server_messages() {
    assert_eq!(server_message(r#"{"message": "x"}"#).as_deref(), Some("x"));
    assert_eq!(
        server_message(r#"{"success": false, "errors": ["y"]}"#).as_deref(),
        Some("y")
    );
    assert_eq!(
        server_message(r#"{"errors": {"field": ["z"]}}"#).as_deref(),
        Some("z")
    );
    assert_eq!(server_message("<html>"), None);
}

#[test]
fn not_started_message_is_clean() {
    let body = r#"{"message": "Hack N' Flag 2026 has not started yet"}"#;
    let d = dump(
        403,
        Some("application/json"),
        "https://ctf.example.com/api/v1/users/me",
        body,
    );
    let CtfdError::Denied { kind, message } = d.denied(server_message(body).as_deref(), false)
    else {
        panic!("expected Denied");
    };
    assert_eq!(kind, DenyKind::NotStarted);
    assert!(message.starts_with("Hack N' Flag 2026 has not started yet.\n"));
    assert!(message.contains("Your API token works"));
    assert!(message.contains("-> HTTP 403 Forbidden"));
    assert!(!message.contains("response body"));
    assert!(!message.contains("check the token"));
}

#[test]
fn strips_terminal_escapes_and_handles_empty_body() {
    assert_eq!(sanitize_line("a\x1b[31mred"), "a?[31mred");
    let url = "https://ctf.example.com/api/v1/users/me";
    assert!(
        dump(500, None, url, "")
            .render("H")
            .contains("Body:     (empty)")
    );
}
