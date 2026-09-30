//! The rating scale, best to worst. `NotRated` stands outside it: nodes with nothing to rate
//! (KDFs, padding, components without crypto) are neutral in a roll-up.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Status {
    /// Approved, and not vulnerable to a quantum computer.
    Safe,
    /// Approved now.
    Acceptable,
    /// Could not be identified, or a parameter the rating needs is missing. Never green.
    Unknown,
    /// Allowed, but on the way out.
    Deprecated,
    /// Not permitted by the profile, such as an expired certificate.
    Disallowed,
    /// Practically broken: MD5, SHA-1 signatures, DES, RC4, RSA-1024.
    Broken,
    NotRated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Color {
    Green,
    Grey,
    Yellow,
    Red,
    Muted,
}

impl Status {
    pub const ALL: [Status; 7] = [
        Status::Safe,
        Status::Acceptable,
        Status::Unknown,
        Status::Deprecated,
        Status::Disallowed,
        Status::Broken,
        Status::NotRated,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Status::Safe => "safe",
            Status::Acceptable => "acceptable",
            Status::Unknown => "unknown",
            Status::Deprecated => "deprecated",
            Status::Disallowed => "disallowed",
            Status::Broken => "broken",
            Status::NotRated => "not-rated",
        }
    }

    pub fn color(self) -> Color {
        match self {
            Status::Safe | Status::Acceptable => Color::Green,
            Status::Unknown => Color::Grey,
            Status::Deprecated => Color::Yellow,
            Status::Disallowed | Status::Broken => Color::Red,
            Status::NotRated => Color::Muted,
        }
    }

    /// The worse of two, for roll-ups. Unknown ranks above green on purpose: an asset that could
    /// not be identified must never let its component read as healthy.
    pub fn worst(self, other: Status) -> Status {
        match (self, other) {
            (Status::NotRated, s) | (s, Status::NotRated) => s,
            (a, b) => a.max(b),
        }
    }
}
