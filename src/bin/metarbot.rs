#![forbid(unsafe_code)]
#![recursion_limit="256"]

extern crate clap;
extern crate futures;
extern crate irc;
extern crate pretty_env_logger;
extern crate regex;
extern crate tokio;
extern crate reqwest;

#[macro_use] extern crate lazy_static;
#[macro_use] extern crate log;

use irc::client::prelude::*;
use futures::{
    prelude::*,
    future::FutureExt,
    future::BoxFuture,
    stream::FuturesUnordered,
    select,
};
use regex::Regex;
use std::time::Duration;
use std::vec::Vec;
use std::collections::HashMap;
use std::error;
use std::fmt;

#[derive(Debug)]
enum BotError {
    NoResponseTarget,
    NoChannelToPart,
    ReqwestError(reqwest::Error),
}

impl fmt::Display for BotError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BotError::NoResponseTarget =>
                write!(f, "Ignoring message since no response target is set"),
            BotError::NoChannelToPart =>
                write!(f, "Requested to part the current channel outside of a channel"),
            BotError::ReqwestError(ref e) =>
                e.fmt(f),
        }
    }
}

impl error::Error for BotError {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match *self {
            BotError::NoResponseTarget => None,
            BotError::NoChannelToPart => None,
            BotError::ReqwestError(ref e) => Some(e),
        }
    }
}

impl From<reqwest::Error> for BotError {
    fn from(err: reqwest::Error) -> BotError {
        BotError::ReqwestError(err)
    }
}

#[derive(Debug)]
struct BotPartResponse {
    channel: String,
    comment: Option<String>,
}

#[derive(Debug)]
struct BotJoinResponse {
    channel: String,
}

#[derive(Debug)]
struct BotPrivmsgResponse {
    target: String,
    message: String,
}

#[derive(Debug)]
struct BotNoticeResponse {
    target: String,
    message: String,
}

#[derive(Debug)]
struct BotQuitResponse {
    message: Option<String>
}

#[derive(Debug)]
enum BotResponse {
    Ignore,
    Quit(BotQuitResponse),
    Part(BotPartResponse),
    Join(BotJoinResponse),
    Privmsg(BotPrivmsgResponse),
    Notice(BotNoticeResponse),
}

#[derive(Debug)]
struct BotParameters {
    message: irc::proto::message::Message,
    leader: String,
    owners: Vec<Prefix>,
    args: Vec<String>,
}

type BotCommandResult = Result<BotResponse, BotError>;
type BotCommandFutureResult<'a> = BoxFuture<'a, BotCommandResult>;
type BotCommand = Box<dyn Fn(BotParameters) -> BotCommandFutureResult<'static>>;

static METAR_API_URL: &str = "https://api.met.no/weatherapi/tafmetar/1.0/metar";
static TAF_API_URL: &str = "https://api.met.no/weatherapi/tafmetar/1.0/taf";

lazy_static! {
    static ref AIRPORT_RE: Regex = Regex::new(r"^(?i)[a-z0-9]{4}$").unwrap();
}
lazy_static! {
    static ref REQWEST: reqwest::Client = reqwest::Client::new();
}

#[derive(Debug)]
enum WeatherType {
    METAR,
    TAF
}

impl fmt::Display for WeatherType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            WeatherType::METAR => write!(f, "METAR"),
            WeatherType::TAF   => write!(f, "TAF"),
        }
    }
}

async fn handle_metar_taf(type_: WeatherType, params: BotParameters) -> BotCommandResult {
    let response_target = params.message
        .response_target()
        .ok_or(BotError::NoResponseTarget)?
        .to_string();

    if let Some(airport) = params.args.get(0) {
        if AIRPORT_RE.is_match(airport) {
            let result = REQWEST.get(match type_ {
                    WeatherType::METAR => METAR_API_URL,
                    WeatherType::TAF => TAF_API_URL,
                })
                .header("Accept", "text/plain")
                .timeout(Duration::from_secs(5))
                .query(&[("icao", airport)])
                .send()
                .await;
            return match result {
                Ok(response) =>
                    if !response.status().is_success() {
                        Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                            target: response_target,
                            message: format!("{} {} {}", airport, type_, response.status()),
                        }))
                    } else {
                        match response.text().await {
                            Ok(string) =>
                                if let Some(metar) = string.lines().rfind(|item| !item.trim().is_empty()) {
                                    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                                        target: response_target,
                                        message: metar.to_string(),
                                    }))
                                } else {
                                    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                                        target: response_target,
                                        message: format!("No {} found for {}", type_, airport),
                                    }))
                                }
                            Err(err) =>
                                Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                                    target: response_target,
                                    message: format!("Error decoding response: {}", err),
                                }))
                        }
                    },
                Err(err) =>
                    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                        target: response_target,
                        message: format!("{} {} {}", airport, type_, err),
                    }))
            }
        }
    }
    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
        target: response_target,
        message: format!("Usage: {}{} <4-letter ICAO airport code>", params.leader, type_.to_string().to_lowercase()),
    }))
}

async fn handle_metar(params: BotParameters) -> BotCommandResult {
    handle_metar_taf(WeatherType::METAR, params).await
}

async fn handle_taf(params: BotParameters) -> BotCommandResult {
    handle_metar_taf(WeatherType::TAF, params).await
}

fn is_owner(prefix: &Prefix, owners: &Vec<Prefix>) -> bool {
    debug!("is_owner: {:?}, owners: {:?}", prefix, owners);
    match prefix {
        Prefix::ServerName(_) => false,
        Prefix::Nickname(nick, user, host) => {
            for match_prefix in owners {
                if let Prefix::Nickname(match_nick, match_user, match_host) = match_prefix {
                    if !match_nick.is_empty() {
                        let maybe_pattern = glob::Pattern::new(match_nick);
                        if !maybe_pattern.is_ok() {
                            warn!("Failed to compile pattern '{}': {}", match_nick, maybe_pattern.err().unwrap());
                            continue
                        }
                        if !maybe_pattern.unwrap().matches(&nick) {
                            continue
                        }
                    }
                    if !match_user.is_empty() {
                        let maybe_pattern = glob::Pattern::new(match_user);
                        if !maybe_pattern.is_ok() {
                            warn!("Failed to compile pattern '{}': {}", match_user, maybe_pattern.err().unwrap());
                            continue
                        }
                        if !maybe_pattern.unwrap().matches(&user) {
                            continue
                        }
                    }
                    if !match_host.is_empty() {
                        let maybe_pattern = glob::Pattern::new(match_host);
                        if !maybe_pattern.is_ok() {
                            warn!("Failed to compile pattern '{}': {}", match_host, maybe_pattern.err().unwrap());
                            continue
                        }
                        if !maybe_pattern.unwrap().matches(&host) {
                            continue
                        }
                    }
                    if match_nick.is_empty() && match_user.is_empty() && match_host.is_empty() {
                        continue
                    }
                    return true;
                }
            };
            false
        }
    }
}

async fn handle_quit(params: BotParameters) -> BotCommandResult {
    if !is_owner(&params.message.prefix.as_ref().unwrap_or(&Prefix::new_from_str("")), &params.owners) {
        if let Some(ref source_nickname) = params.message.source_nickname() {
            Ok(BotResponse::Notice(BotNoticeResponse {
                target: source_nickname.to_string(),
                message: "You are not authorized to use the quit command".to_string(),
            }))
        } else {
            Ok(BotResponse::Ignore)
        }
    } else {
        Ok(BotResponse::Quit(BotQuitResponse {
            message: if params.args.len() > 0 { Some(params.args.join(" ")) } else { None }
        }))
    }
}

async fn handle_join(params: BotParameters) -> BotCommandResult {
    if !is_owner(&params.message.prefix.as_ref().unwrap_or(&Prefix::new_from_str("")), &params.owners) {
        if let Some(source_nickname) = params.message.source_nickname() {
            Ok(BotResponse::Notice(BotNoticeResponse {
                target: source_nickname.to_string(),
                message: "You are not authorized to use the join command".to_string(),
            }))
        } else {
            Ok(BotResponse::Ignore)
        }
    } else {
        if let Some(channel) = params.args.get(0) {
            Ok(BotResponse::Join(BotJoinResponse {
                channel: channel.to_string(),
            }))
        } else {
            Ok(BotResponse::Ignore)
        }
    }
}

async fn handle_part(params: BotParameters) -> BotCommandResult {
    if !is_owner(&params.message.prefix.as_ref().unwrap_or(&Prefix::new_from_str("")), &params.owners) {
        if let Some(source_nickname) = params.message.source_nickname() {
            Ok(BotResponse::Notice(BotNoticeResponse {
                target: source_nickname.to_string(),
                message: "You are not authorized to use the join command".to_string(),
            }))
        } else {
            Ok(BotResponse::Ignore)
        }
    } else {
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
                        Err(BotError::NoResponseTarget),
                },
        }?.to_string();

        let comment = if params.args.len() > 1 {
            Some(params.args[1..].join(" "))
        } else {
            None
        };

        Ok(BotResponse::Part(BotPartResponse {
            channel,
            comment,
        }))
    }
}


fn is_public(target: &str) -> bool {
    target.is_channel_name()
}

fn handle_response(client: &Client, response: BotResponse) -> irc::error::Result<()> {
    match response {
        BotResponse::Ignore =>
            Ok(()),
        BotResponse::Quit(quit_response) =>
            client.send(Command::QUIT(quit_response.message)),
        BotResponse::Part(part_response) =>
            client.send(Command::PART(part_response.channel, part_response.comment)),
        BotResponse::Join(join_response) =>
            client.send_join(join_response.channel),
        BotResponse::Privmsg(privmsg_response) =>
            client.send_privmsg(privmsg_response.target, privmsg_response.message),
        BotResponse::Notice(notice_response) =>
            client.send_notice(notice_response.target, notice_response.message),
    }
}

fn make_boxed<Fut>(func: &'static dyn Fn(BotParameters) -> Fut) -> BotCommand
    where
        Fut: Future<Output = BotCommandResult> + FutureExt + Send
{
    Box::new(move |params| func(params).boxed())
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
    let leader = config.options.get("leader").map_or("&", String::as_str).to_owned();
    let owners: Vec<Prefix> = config.options.get("owners").unwrap_or(&"".to_string()).split(";").map(Prefix::new_from_str).collect();

    let mut commands : HashMap<&'static str, BotCommand> = HashMap::new();
    commands.insert("metar", make_boxed(&handle_metar));
    commands.insert("taf", make_boxed(&handle_taf));
    commands.insert("quit", make_boxed(&handle_quit));
    commands.insert("join", make_boxed(&handle_join));
    commands.insert("part", make_boxed(&handle_part));

    let mut client = Client::from_config(config).await?;
    client.identify()?;

    let mut stream = client.stream()?;

    let mut futures = FuturesUnordered::new();

    loop {
        select! {
            maybe_message = stream.next() => {
                if let Some(message) = maybe_message.transpose()? {
                    if let Command::PRIVMSG(ref target, ref text) = message.command {
                        let leader_required = is_public(target);
                        if leader_required && !text.starts_with(&leader) {
                            continue
                        }
                        let tokens : Vec<String> = match leader_required {
                            true => text.trim_start_matches(&leader),
                            false => text
                        }.split_whitespace().map(String::from).collect();

                        if let Some((ref cmd, ref args)) = tokens.split_first() {
                            if let Some(handler) = commands.get(cmd.to_lowercase().as_str()) {
                                futures.push(handler(BotParameters {
                                    message: message,
                                    leader: if leader_required { leader.to_string() } else { "".to_string() },
                                    owners: owners.to_vec(),
                                    args: args.to_vec(),
                                }).fuse());
                            }
                        }
                    }
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
