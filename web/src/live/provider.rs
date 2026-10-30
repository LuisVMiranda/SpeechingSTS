use super::{receive_event, Receiver, UpstreamMessage};
use crate::protocol;
use axum::extract::ws::{Message, WebSocket};
use futures_util::SinkExt;
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::{protocol::CloseFrame, Error};

struct Diagnostic {
    code: &'static str,
    message: &'static str,
}

pub(super) async fn connection_failed(
    socket: &mut WebSocket,
    practice: &protocol::Practice,
    failure: Option<&Error>,
) {
    let status = match failure {
        Some(Error::Http(response)) => Some(response.status().as_u16()),
        _ => None,
    };
    let diagnostic = classify_connection(status);
    tracing::warn!(provider=%practice.provider, category=diagnostic.code, ?status, timed_out=failure.is_none(), "provider connection failed");
    let event = json!({"type":"error","code":diagnostic.code,"message":diagnostic.message});
    let _ = timeout(
        Duration::from_secs(3),
        socket.send(Message::Text(event.to_string().into())),
    )
    .await;
}

fn classify_connection(status: Option<u16>) -> Diagnostic {
    match status {
        Some(401) => classify("invalid_api_key", true),
        Some(403) => classify("permission_denied", true),
        Some(429) => classify("quota", true),
        _ => Diagnostic {
            code: "providerConnectionFailed",
            message:
                "Could not connect to the provider. Check the network and server configuration.",
        },
    }
}

pub(super) async fn receive(
    incoming: Option<Result<UpstreamMessage, Error>>,
    receiver: &mut Receiver<'_>,
) -> bool {
    match incoming {
        Some(Ok(UpstreamMessage::Text(text))) => receive_json(text.as_bytes(), receiver).await,
        Some(Ok(UpstreamMessage::Binary(bytes))) => receive_json(&bytes, receiver).await,
        Some(Ok(UpstreamMessage::Ping(_) | UpstreamMessage::Pong(_))) => true,
        Some(Ok(UpstreamMessage::Close(frame))) => {
            report_close(frame.as_ref(), receiver).await;
            false
        }
        _ => {
            report("", None, receiver).await;
            false
        }
    }
}

async fn receive_json(bytes: &[u8], receiver: &mut Receiver<'_>) -> bool {
    let Ok(data) = serde_json::from_slice::<Value>(bytes) else {
        return true;
    };
    if let Some(error) = data.get("error") {
        report(&error.to_string(), None, receiver).await;
        return *receiver.ready;
    }
    for event in protocol::parse(&receiver.practice.provider, &data) {
        if !receive_event(event, receiver).await {
            return false;
        }
    }
    true
}

async fn report_close(frame: Option<&CloseFrame>, receiver: &mut Receiver<'_>) {
    let code = frame.map(|frame| u16::from(frame.code));
    let reason = frame.map_or("", |frame| frame.reason.as_str());
    report(reason, code, receiver).await;
}

async fn report(reason: &str, close_code: Option<u16>, receiver: &mut Receiver<'_>) {
    let diagnostic = classify(reason, !*receiver.ready);
    tracing::warn!(provider=%receiver.practice.provider, category=diagnostic.code, ?close_code, during_setup=!*receiver.ready, "provider connection reported an error");
    let event = json!({"type":"error","code":diagnostic.code,"message":diagnostic.message});
    let _ = timeout(
        Duration::from_secs(3),
        receiver
            .client_tx
            .send(Message::Text(event.to_string().into())),
    )
    .await;
}

fn classify(reason: &str, during_setup: bool) -> Diagnostic {
    let value = reason.to_ascii_lowercase();
    if value.contains("api key")
        && contains_any(&value, &["invalid", "not valid", "expired", "revoked"])
        || contains_any(&value, &["invalid_api_key", "unauthenticated"])
    {
        return Diagnostic {
            code: "providerInvalidKey",
            message: "The provider rejected the API key. Check or replace it in settings.",
        };
    }
    if contains_any(
        &value,
        &[
            "quota",
            "rate limit",
            "rate_limit",
            "resource_exhausted",
            "429",
        ],
    ) {
        return Diagnostic { code:"providerQuota", message:"The provider's quota or rate limit was reached. Check your provider account and try again later." };
    }
    if value.contains("model")
        && contains_any(
            &value,
            &[
                "not found",
                "not supported",
                "unavailable",
                "invalid",
                "does not exist",
            ],
        )
    {
        return Diagnostic { code:"providerModelUnavailable", message:"The configured model is unavailable for this provider or account. Check the server's model configuration." };
    }
    if contains_any(
        &value,
        &["permission_denied", "permission denied", "forbidden", "403"],
    ) {
        return Diagnostic {
            code: "providerAccessDenied",
            message:
                "The provider denied access. Check the key's project permissions and model access.",
        };
    }
    if during_setup {
        return Diagnostic { code:"providerSetupRejected", message:"The provider ended the connection during setup. Check the key, model, and provider settings." };
    }
    Diagnostic {
        code: "providerDisconnected",
        message:
            "The provider connection ended. You can continue this conversation by starting again.",
    }
}

fn contains_any(value: &str, options: &[&str]) -> bool {
    options.iter().any(|option| value.contains(option))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_errors_are_classified_without_copying_untrusted_text() {
        for (reason, expected) in [
            ("API key not valid. Secret marker", "providerInvalidKey"),
            ("RESOURCE_EXHAUSTED", "providerQuota"),
            ("model is not found", "providerModelUnavailable"),
            ("PERMISSION_DENIED", "providerAccessDenied"),
            ("unexpected confidential message", "providerSetupRejected"),
        ] {
            let diagnostic = classify(reason, true);
            assert_eq!(diagnostic.code, expected);
            assert!(!diagnostic.message.contains("Secret marker"));
            assert!(!diagnostic.message.contains("confidential"));
        }
        assert_eq!(classify("", false).code, "providerDisconnected");
    }

    #[test]
    fn connection_statuses_produce_safe_actionable_diagnostics() {
        for (status, expected) in [
            (Some(401), "providerInvalidKey"),
            (Some(403), "providerAccessDenied"),
            (Some(429), "providerQuota"),
            (Some(500), "providerConnectionFailed"),
            (None, "providerConnectionFailed"),
        ] {
            assert_eq!(classify_connection(status).code, expected);
        }
    }

    #[test]
    fn untrusted_provider_errors_are_never_copied_into_diagnostics() {
        let patterns = ["API key not valid", "quota", "model not found", "unknown"];
        for index in 0..512 {
            let marker = format!("synthetic-private-marker-{index}");
            let reason = format!("{} {marker}", patterns[index % patterns.len()]);
            let diagnostic = classify(&reason, index % 2 == 0);
            assert!(!diagnostic.message.contains(&marker));
            assert!(!diagnostic.code.contains(&marker));
        }
    }
}
