//! Utility functions to help write IRC bot commands.

#![deny(unsafe_code)]
#![deny(missing_docs)]

extern crate irc;

use std::vec::Vec;

use irc::client::prelude::Prefix;
use irc::client::prelude::ChannelExt;

/**
 * Determine whether the given IRC prefix (i.e. tuple of (nickname, username, hostname)) matches
 * one of the patterns given in owners. For each one of the entries in owners, each of the
 * components will be evaluated as glob expressions against the given prefix. Note that empty
 * strings will implicitly match everything, unless all three parts are empty, in which case the
 * entry is ignored.
 */
pub fn is_owner(prefix: &Prefix, owners: &Vec<Prefix>) -> bool {
    let compile_and_test = |pattern, haystack| {
        match glob::Pattern::new(pattern) {
            Err(err) => {
                warn!("Failed to compile pattern '{}': {}", pattern, err);
                false
            },
            Ok(matcher) =>
                matcher.matches(haystack),

        }
    };

    match prefix {
        Prefix::ServerName(_) =>
            false,
        Prefix::Nickname(nick, user, host) => {
            for owner in owners {
                if let Prefix::Nickname(owner_nick, owner_user, owner_host) = owner {
                    if owner_nick.is_empty() && owner_user.is_empty() && owner_host.is_empty() {
                        continue
                    }
                    if !owner_nick.is_empty() {
                        if !compile_and_test(&owner_nick, &nick) {
                            continue
                        }
                    }
                    if !owner_user.is_empty() {
                        if !compile_and_test(&owner_user, &user) {
                            continue
                        }
                    }
                    if !owner_host.is_empty() {
                        if !compile_and_test(&owner_host, &host) {
                            continue
                        }
                    }
                    return true;
                }
            };
            false
        },
    }
}

/**
 * Return true iff the given target string represents an IRC channel. Returns false otherwise, e.g.
 * then the given target is a nickname.
 */
pub fn is_public(target: &str) -> bool {
    target.is_channel_name()
}
