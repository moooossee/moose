use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

pub fn new_id() -> String {
    Uuid::now_v7().to_string()
}

pub fn utc_now() -> String {
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .expect("UTC timestamps always format as RFC 3339")
}
