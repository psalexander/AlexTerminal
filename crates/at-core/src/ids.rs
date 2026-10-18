//! Stable identifiers. ULIDs survive restarts and are written into the snapshot.

use serde::{Deserialize, Serialize};
use std::fmt;

macro_rules! id_type {
    ($name:ident, $prefix:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub ulid::Ulid);

        impl $name {
            pub fn new() -> Self {
                Self(ulid::Ulid::new())
            }
            pub fn parse(s: &str) -> Option<Self> {
                ulid::Ulid::from_string(s).ok().map(Self)
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let s = self.0.to_string();
                write!(f, "{}:{}", $prefix, &s[s.len() - 6..])
            }
        }
    };
}

id_type!(WindowId, "W");
id_type!(TabId, "T");
id_type!(PaneId, "P");
