//! Module that provides standard IRC actions, such as join, part, and quit

#![deny(unsafe_code)]
#![deny(missing_docs)]

extern crate async_trait;
extern crate irc;

use crate::{
    BotCommand,
    BotCommandResult,
    BotError,
    BotParameters,
    BotResponse,
    util::is_owner,
    util::is_public,
};

use irc::client;

struct IrcJoinCommand {}
struct IrcPartCommand {}
struct IrcQuitCommand {}

/**
 * Factory function that will create instances of all implemented commands in this module.
 */
pub fn mk() -> Vec<Box<dyn BotCommand>> {
    vec![
        Box::new(IrcJoinCommand{}),
        Box::new(IrcPartCommand{}),
        Box::new(IrcQuitCommand{}),
    ]
}

/**
 * Function to ensure that the person sending the message is the owner of the bot. If that is the
 * case, None will be returned, and execution of the command should continue. Otherwise, a suitable
 * error message is returned as a BotCommandResult, which should be bubbled up to the caller.
 */
fn ensure_owner(command: &str, params: &BotParameters<'_>) -> Option<BotCommandResult> {
    if !is_owner(&params.message.prefix.as_ref().unwrap_or(&client::prelude::Prefix::new_from_str("")), &params.owners) {
        if let Some(source_nickname) = params.message.source_nickname() {
            Some(Ok(BotResponse::Notice(
                source_nickname.to_string(),
                format!("You are not authorized to use the {} command", command))))
        } else {
            Some(Ok(BotResponse::Ignore))
        }
    } else {
        None
    }
}


#[async_trait::async_trait]
impl BotCommand for IrcJoinCommand {
    fn trigger(&self) -> &'static str {
        "join"
    }

    async fn handle(&self, params: BotParameters<'_>) -> BotCommandResult {
        if let Some(botcommand) = ensure_owner(self.trigger(), &params) {
            return botcommand;
        }

        match params.args.get(0) {
            Some(channel) =>
                Ok(BotResponse::Join(channel.to_string())),
            None =>
                Ok(BotResponse::Ignore)
        }
    }
}

#[async_trait::async_trait]
impl BotCommand for IrcPartCommand {
    fn trigger(&self) -> &'static str {
        "part"
    }

    async fn handle(&self, params: BotParameters<'_>) -> BotCommandResult {
        if let Some(botcommand) = ensure_owner(self.trigger(), &params) {
            return botcommand;
        }

        let channel = match params.args.get(0) {
            Some(channel) => Ok(channel.as_str()),
            None =>
                match params.message.response_target() {
                    Some(response_target) =>
                        if is_public(response_target) {
                            Ok(response_target)
                        } else {
                            Err(BotError::NoChannelToPart)
                        },
                    None =>
                        Err(BotError::NoChannelToPart),
                },
        }?.to_string();

        let comment = if params.args.len() > 1 {
            Some(params.args[1..].join(" "))
        } else {
            None
        };

        Ok(BotResponse::Part(channel, comment))
    }
}

#[async_trait::async_trait]
impl BotCommand for IrcQuitCommand {
    fn trigger(&self) -> &'static str {
        "quit"
    }

    async fn handle(&self, params: BotParameters<'_>) -> BotCommandResult {
        if let Some(botcommand) = ensure_owner(self.trigger(), &params) {
            return botcommand;
        }

        Ok(BotResponse::Quit(
            if params.args.len() > 0 {
                Some(params.args.join(" "))
            } else {
                None
            }))
    }
}
