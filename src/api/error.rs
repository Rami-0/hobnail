//! The `/api/v1` error shape (docs/plans/react-port.md 4.2):
//! `{"error":{"code":"…","message":"…","field":"…"}}`, `field` omitted when
//! the error is not about one field; always `Cache-Control: no-store`.
//!
//! Gate failures of writes are not this shape: they stay plain text
//! (`auth::WriteDenied::response`).

use hyper::header::{HeaderValue, ALLOW};
use hyper::{Response, StatusCode};
use serde::Serialize;

use crate::http::server::Body;

/// One API error. `status` is the HTTP status and is not serialised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiError {
    #[serde(skip)]
    #[cfg_attr(test, ts(skip))]
    pub status: StatusCode,
    /// Stable, machine-readable: `not_found`, `method_not_allowed`,
    /// `sign_in_required`, `logs_sign_in_required`, `bad_request`,
    /// `internal` (500: a handler task failed).
    pub code: &'static str,
    /// Plain words for a person.
    pub message: String,
    /// The request field the error is about, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub field: Option<String>,
}

/// The body of every `/api/v1` error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ApiErrorBody {
    pub error: ApiError,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            field: None,
        }
    }

    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "not found")
    }

    pub fn method_not_allowed() -> Self {
        Self::new(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "method not allowed",
        )
    }

    pub fn sign_in_required() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "sign_in_required",
            "sign in required",
        )
    }

    pub fn logs_sign_in_required() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "logs_sign_in_required",
            "sign in to read logs",
        )
    }

    /// A malformed request; `field` names the offending field, if any.
    pub fn bad_request(message: impl Into<String>, field: Option<&str>) -> Self {
        Self {
            field: field.map(str::to_string),
            ..Self::new(StatusCode::BAD_REQUEST, "bad_request", message)
        }
    }

    pub fn response(self) -> Response<Body> {
        let status = self.status;
        super::json(status, &ApiErrorBody { error: self })
    }

    /// A 405 with `Allow` listing the methods the path does answer.
    pub fn method_not_allowed_response(allow: &[&str]) -> Response<Body> {
        let mut r = Self::method_not_allowed().response();
        if let Ok(v) = HeaderValue::from_str(&allow.join(", ")) {
            r.headers_mut().insert(ALLOW, v);
        }
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use http_body_util::BodyExt;

    async fn parts(r: Response<Body>) -> (StatusCode, hyper::HeaderMap, serde_json::Value) {
        let (p, b) = r.into_parts();
        let bytes = b.collect().await.unwrap().to_bytes();
        (p.status, p.headers, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn shape_headers_and_codes() {
        let cases = [
            (ApiError::not_found(), 404, "not_found"),
            (ApiError::method_not_allowed(), 405, "method_not_allowed"),
            (ApiError::sign_in_required(), 401, "sign_in_required"),
            (
                ApiError::logs_sign_in_required(),
                401,
                "logs_sign_in_required",
            ),
            (ApiError::bad_request("bad", None), 400, "bad_request"),
        ];
        for (e, status, code) in cases {
            let (s, h, v) = parts(e.response()).await;
            assert_eq!(s.as_u16(), status, "{code}");
            assert_eq!(h["content-type"], "application/json", "{code}");
            assert_eq!(h["cache-control"], "no-store", "{code}");
            assert_eq!(v["error"]["code"], code);
            assert!(v["error"]["message"]
                .as_str()
                .is_some_and(|m| !m.is_empty()));
            // No field key unless the error is about one; no status echoed.
            let keys: Vec<_> = v["error"].as_object().unwrap().keys().cloned().collect();
            assert_eq!(keys, ["code", "message"], "{code}");
            assert_eq!(v.as_object().unwrap().len(), 1);
        }
    }

    #[tokio::test]
    async fn field_is_sent_when_set() {
        let (s, _, v) =
            parts(ApiError::bad_request("name is empty", Some("name")).response()).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
        assert_eq!(
            v,
            serde_json::json!({"error":{"code":"bad_request","message":"name is empty","field":"name"}})
        );
    }

    #[tokio::test]
    async fn method_not_allowed_lists_allow() {
        let r = ApiError::method_not_allowed_response(&["GET", "HEAD"]);
        assert_eq!(r.headers()["allow"], "GET, HEAD");
        let (s, _, v) = parts(r).await;
        assert_eq!(s, StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(v["error"]["code"], "method_not_allowed");
    }
}
