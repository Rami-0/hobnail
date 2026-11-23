use super::*;
use crate::{util::unix_now, widgets::StructuredBody};

pub(super) const SSL_EXPIRY: Provider = Provider {
    id: "ssl_expiry",
    name: "SSL certificate expiry",
    category: "Network",
    icon: "ssl",
    color: "#16a34a",
    detect: None,
    description: "Inspect the certificate presented by a TLS server, including expired and untrusted certificates.",
    docs: "https://www.rfc-editor.org/rfc/rfc9525.html",
    public_internet: false,
    contacts: "The configured TLS hostname and port. Public addresses require internet access.",
    fields: &[
        Field {
            label: "TLS endpoint",
            help: "Enter one hostname per integration as https://host or https://host:port. Default port: 443.",
            ..SERVICE_URL
        },
        ENABLED,
    ],
    test,
    widgets: &[Widget {
        id: "expiry",
        name: "Certificate expiry",
        description: "Days until expiry, issuer, subject, hostname match and trust verdict.",
        recommended: true,
        default_size: size(3, 2),
        min_size: size(2, 2),
        sample: r#"{"kind":"stat_grid","rows":[["Expires in","54 days"],["Issuer","Example CA"],["Verified","Yes"]]}"#,
        fields: &[
            Field {
                default: Some("900"),
                ..REFRESH
            },
            Field {
                default: Some("14"),
                min: Some(1),
                max: Some(365),
                ..Field::new("warn_days", "Warn within (days)", Kind::Number, "")
            },
            Field {
                default: Some("7"),
                min: Some(1),
                max: Some(365),
                ..Field::new("urgent_days", "Urgent within (days)", Kind::Number, "")
            },
        ],
        fetch,
        validate,
    }],
};

fn days_option(options: &Values, key: &str, fallback: u64) -> i64 {
    options.get(key).and_then(Value::as_u64).unwrap_or(fallback) as i64
}

fn validate(options: &Values) -> Result<(), &'static str> {
    if days_option(options, "urgent_days", 7) > days_option(options, "warn_days", 14) {
        return Err("urgent_days");
    }
    Ok(())
}

fn test<'a>(conn: &'a Conn<'_>) -> Job<'a, ()> {
    Box::pin(async move { conn.tls_leaf().await.map(|_| ()) })
}

fn expiry(seconds: i64, options: &Values) -> String {
    let days = seconds.div_euclid(86400);
    if seconds < 0 {
        return format!("Expired · {} day(s) ago", -days);
    }
    let status = if seconds <= days_option(options, "urgent_days", 7) * 86400 {
        " · Urgent"
    } else if seconds <= days_option(options, "warn_days", 14) * 86400 {
        " · Warning"
    } else {
        ""
    };
    format!("{days} day(s) remaining{status}")
}

fn fetch<'a>(conn: &'a Conn<'_>, options: &'a Values) -> Job<'a, StructuredBody> {
    Box::pin(async move {
        let leaf = conn.tls_leaf().await?;
        let trust = match leaf.verification {
            TlsVerification::Trusted => "Trusted by public roots",
            TlsVerification::Pinned => "Pinned leaf matched",
            TlsVerification::PinMismatch => "Pin mismatch",
            TlsVerification::Rejected(_) => "Untrusted",
        };
        let names: Vec<_> = leaf
            .sans
            .into_iter()
            .filter_map(|name| match name {
                SubjectAltName::Dns(name) => Some(name),
                SubjectAltName::Ip(ip) => Some(ip.to_string()),
                _ => None,
            })
            .take(8)
            .collect();
        Ok(StructuredBody::StatGrid {
            rows: vec![
                (
                    "Expiry".into(),
                    expiry(leaf.not_after - unix_now() as i64, options),
                ),
                ("Trust".into(), trust.into()),
                ("Issuer".into(), display_text(conn, leaf.issuer)),
                ("Subject".into(), display_text(conn, leaf.subject)),
                (
                    "Hostname".into(),
                    if leaf.name_matches {
                        "Matches SAN"
                    } else {
                        "No matching SAN"
                    }
                    .into(),
                ),
                (
                    "DNS/IP SANs".into(),
                    if names.is_empty() {
                        "None".into()
                    } else {
                        display_text(conn, names.join(", "))
                    },
                ),
            ],
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expiry_uses_exact_thresholds_and_reports_already_expired_leaves() {
        let options = Values::new();
        assert!(expiry(-1, &options).starts_with("Expired"));
        assert!(expiry(7 * 86400, &options).ends_with("Urgent"));
        assert!(expiry(7 * 86400 + 1, &options).ends_with("Warning"));
        assert_eq!(expiry(14 * 86400 + 1, &options), "14 day(s) remaining");
    }
}
