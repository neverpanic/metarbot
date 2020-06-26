//! IRC bot to fetch METARs and TAFs from api.met.no

#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![recursion_limit="512"]

extern crate clap;
extern crate futures;
extern crate irc;
extern crate pretty_env_logger;
extern crate regex;
extern crate tokio;
extern crate reqwest;

#[macro_use] extern crate log;

use irc::client::prelude::*;
use futures::{
    prelude::*,
    future::FutureExt,
    stream::FuturesUnordered,
    select,
};

use std::vec::Vec;
use std::collections::HashMap;

use metarbot::{
    BotCommand,
    BotParameters,
    BotResponse,
    modules,
    util,
};

static EMPTY_LEADERS: Vec<char> = vec![];

fn handle_response(client: &Client, response: BotResponse) -> irc::error::Result<()> {
    match response {
        BotResponse::Ignore =>
            Ok(()),
        BotResponse::Quit(quit_message) =>
            client.send(Command::QUIT(quit_message)),
        BotResponse::Part(channel, part_message) =>
            client.send(Command::PART(channel, part_message)),
        BotResponse::Join(channel) =>
            client.send_join(channel),
        BotResponse::Privmsg(target, message) =>
            client.send_privmsg(target, message),
        BotResponse::Notice(target, message) =>
            client.send_notice(target, message),
    }
}

#[tokio::main]
async fn main() -> Result<(), failure::Error> {
    let args = clap::App::new("metarbot")
        .arg(
            clap::Arg::with_name("config-file")
                .long("config-file")
                .default_value("config.toml"),
        )
        .get_matches();

    pretty_env_logger::init();

    let config = Config::load(args.value_of("config-file").expect("default missing?")).unwrap();
    let leaders: Vec<char> = config.get_option("leaders").unwrap_or("&").chars().collect();
    let owners: Vec<Prefix> = config.get_option("owners").unwrap_or(&"".to_string()).split(";").map(Prefix::new_from_str).collect();

    let mut commands : HashMap<&'static str, Box<dyn BotCommand>> = HashMap::new();
    for module in modules::ALL {
        for command in module() {
            commands.insert(command.trigger(), command);
        }
    }

    let mut client = Client::from_config(config.clone()).await?;
    client.identify()?;

    let mut stream = client.stream()?;
    let mut futures = FuturesUnordered::new();

    loop {
        select! {
            maybe_message = stream.next() => {
                if let Some(message) = maybe_message.transpose()? {
                    if let Command::PRIVMSG(ref target, ref text) = message.command {
                        let mut leader: Option<char> = None;
                        let leader_required = util::is_public(target);
                        if leader_required {
                            let first_char = text.chars().next();
                            if first_char.is_none() || !leaders.contains(&first_char.unwrap()) {
                                continue
                            }
                            leader = first_char;
                        }
                        let tokens : Vec<String> = match leader {
                            None => text,
                            Some(first_char) => text.trim_start_matches(first_char),
                        }.split_whitespace().map(String::from).collect();

                        if let Some((ref cmd, ref args)) = tokens.split_first() {
                            if let Some(command) = commands.get(cmd.to_lowercase().as_str()) {
                                futures.push(command.handle(BotParameters {
                                    message: message,
                                    leaders: if leader_required { &leaders } else { &EMPTY_LEADERS },
                                    owners: &owners,
                                    args: args.to_vec(),
                                    options: &config.options,
                                }).fuse());
                            }
                        }
                    }
                } else {
                    break;
                }
            },
            result = futures.select_next_some() => {
                match result {
                    Err(e) => warn!("error running command: {:?}", e),
                    Ok(response) =>
                        match handle_response(&client, response) {
                            Ok(()) => (),
                            Err(e) => warn!("error handling response: {:?}", e),
                        },
                };
            },
            complete => break,
        }
    }

    Ok(())
}
