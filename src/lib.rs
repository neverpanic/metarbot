//! A fully async library to write IRC bots using the irc crate (>= 0.14)

#![deny(unsafe_code)]
#![deny(missing_docs)]
#![recursion_limit="256"]

extern crate async_trait;
extern crate futures;
extern crate irc;
extern crate pretty_env_logger;

#[macro_use] extern crate log;
#[macro_use] extern crate lazy_static;

use std::error;
use std::fmt;
use std::result::Result;

use futures::future;

use irc::client;

/// Various modules that provide commands for the IRC bot.
pub mod modules;

/// Utility functions to help write IRC bot commands.
pub mod util;

/// An error type for the IRC bot; wraps other errors where needed.
#[derive(Debug)]
pub enum BotError {
    /** This message could have been processed, but there is no response target to send the
        response to, so it has been ignored. */
    NoResponseTarget,

    /** The bot has been asked to leave the current channel outside of a channel. */
    NoChannelToPart,
}

/// Implementation of the Display trait for BotError, so it can be converted to a string.
impl fmt::Display for BotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BotError::NoResponseTarget =>
                write!(f, "Ignoring message since no response target is set"),
            BotError::NoChannelToPart =>
                write!(f, "Requested to part the current channel outside of a channel"),
        }
    }
}

/**
 * Implementation of the Error trait for BotError. This can be used to get details about wrapped
 * errors, e.g. the underlying reqwest::Error for BotError::ReqwestError.
 */
impl error::Error for BotError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            BotError::NoResponseTarget => None,
            BotError::NoChannelToPart => None,
        }
    }
}

/**
 * Various actions the bot can trigger in response to a command. Each enum specifies one response.
 */
#[derive(Debug)]
pub enum BotResponse {
    /**
     * Do nothing and just ignore the command.
     */
    Ignore,

    /**
     * Quit the connection to the current IRC server. The optional string argument is used as quit
     * message.
     */
    Quit(Option<String>),

    /**
     * Part the given channel. The second argument is an optional part message.
     */
    Part(String, Option<String>),

    /**
     * Join a new channel using the given channel name.
     */
    Join(String),

    /**
     * Send a privmsg. This is probably what you will use most of the time. The first parameter is
     * the target of the privmsg (e.g. a channel name or a nickname), the second argument is the
     * text to send.
     */
    Privmsg(String, String),

    /**
     * Send a notice. The first parameter is the target of the notice (e.g. a nickname), the second
     * argument is the notice text.
     */
    Notice(String, String),
}

/**
 * Parameters passed to a module that implements a command whenever it is being invoked.
 */
#[derive(Debug)]
pub struct BotParameters {
    /**
     * The received IRC message that triggered the module
     */
    pub message: irc::proto::message::Message,

    /**
     * The leader character that was used in this context to trigger the bot. Usually a single
     * character when the message was written in a channel, and an empty string when it was written
     * in a query.
     */
    pub leader: String,

    /**
     * A list of IRC prefixes (i.e. nick, username, hostname tuples) that are considered owners of
     * this bot. Each of the components will be evaluated as glob expressions against the prefix of
     * the user invoking a privileged command. Note that empty strings will implicitly match
     * everything, unless all three parts are empty, in which case the entry is ignored.
     */
    pub owners: Vec<client::prelude::Prefix>,

    /**
     * A list of arguments given to the command, split at whitespaces.
     */
    pub args: Vec<String>,
}

/**
 * The result of a bot command; either a BotResponse, or a BotError.
 */
pub type BotCommandResult = Result<BotResponse, BotError>;

/**
 * A bot result as it would be returned by an async function, only boxed so it can be used without
 * knowing the size of the result.
 */
pub type BotCommandFutureResult<'a> = future::BoxFuture<'a, BotCommandResult>;

/**
 * A box containing an async function that will be invoked to handle a command.
 */
//type BotCommandHandler = Box<dyn Fn(BotParameters) -> BotCommandFutureResult<'static>>;

/**
 * A trait implementing a command.
 */
#[async_trait::async_trait]
pub trait BotCommand {
    /**
     * The trigger string for this bot command, must be a single-word string.
     */
    fn trigger(&self) -> &'static str;

    /**
     * Handler for this bot command, will be invoked when the trigger word has been seen.
     */
    async fn handle(&self, params: BotParameters) -> BotCommandResult;
}
