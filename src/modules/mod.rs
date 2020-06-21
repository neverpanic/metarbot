//! Modules for the fully async IRC bot

#![deny(unsafe_code)]
#![deny(missing_docs)]

pub use self::ircactions::mk as ircactions;
pub use self::metar::mk as metar;

use crate::BotCommand;

/// A module that fetches METARs and TAFs from api.met.no
mod metar;

/// A module that provides standard IRC actions, such as join, part, and quit
mod ircactions;

/// A slice of functions that will create vectors of all implemented modules
pub const ALL: &[fn() -> Vec<Box<dyn BotCommand>>] = &[ircactions, metar];
