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
use std::error::Error;


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
}

type BotCommandResult = Result<BotResponse, Box<dyn Error>>;
type BotCommandFutureResult<'a> = BoxFuture<'a, BotCommandResult>;
type BotCommand = Box<dyn Fn(String, Vec<String>) -> BotCommandFutureResult<'static>>;

static METAR_API_URL: &str = "https://api.met.no/weatherapi/tafmetar/1.0/metar";

lazy_static! {
    static ref AIRPORT_RE: Regex = Regex::new(r"^(?i)[a-z0-9]{4}$").unwrap();
}
lazy_static! {
    static ref REQWEST: reqwest::Client = reqwest::Client::new();
}

async fn handle_metar(response_target: String, args: Vec<String>) -> BotCommandResult {
    if let Some(airport) = args.get(0) {
        if AIRPORT_RE.is_match(airport) {
            let resp = REQWEST.get(METAR_API_URL)
                .header("Accept", "text/plain")
                .timeout(Duration::from_secs(5))
                .query(&[("icao", airport)])
                .send()
                .await?
                .text()
                .await?;
            let maybe_metar = resp
                .lines()
                .rfind(|item| !item.trim().is_empty())
                .to_owned();
            info!("{} METAR: {:?}", airport, maybe_metar);
            return match maybe_metar {
                Some(metar) =>
                    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                        target: response_target,
                        message: metar.to_string(),
                    })),
                None =>
                    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                        target: response_target,
                        message: "empty response".to_string()
                    })),
            };
        }
    }
    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
        target: response_target,
        message: "Usage: <leader>metar <4-letter ICAO airport code>".to_owned(),
    }))
}

async fn handle_taf(response_target: String, args: Vec<String>) -> Result<BotResponse, Box<dyn Error>> {
    if let Some(airport) = args.get(0) {
        if AIRPORT_RE.is_match(airport) {
            return Ok(BotResponse::Privmsg(BotPrivmsgResponse {
                target: response_target,
                message: format!("If I knew how to look up metars already, I would have given you one for {}", airport),
            }))
        }
    }
    Ok(BotResponse::Privmsg(BotPrivmsgResponse {
        target: response_target,
        message: "Usage: <leader>taf <4-letter ICAO airport code>".to_owned(),
    }))
}

async fn handle_quit(_: String, args: Vec<String>) -> Result<BotResponse, Box<dyn Error>> {
    info!("quit {:?}", args);
    Ok(BotResponse::Quit(BotQuitResponse {
        message: Some(args.join(" "))
    }))
}

async fn handle_join(_: String, args: Vec<String>) -> Result<BotResponse, Box<dyn Error>> {
    info!("join {:?}", args);
    if let Some(channel) = args.get(0) {
        return Ok(BotResponse::Join(BotJoinResponse {
            channel: channel.to_string(),
        }));
    }
    Ok(BotResponse::Ignore)
}

async fn handle_part(response_target: String, args: Vec<String>) -> Result<BotResponse, Box<dyn Error>> {
    info!("part {:?}", args);
    let channel = match args.get(0) {
        Some(channel) => channel.to_string(),
        None => response_target,
    };

    let comment = if args.len() > 1 {
        Some(args[1..].join(" "))
    } else {
        None
    };

    return Ok(BotResponse::Part(BotPartResponse {
        channel,
        comment,
    }));
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
    }
}

fn make_boxed<Fut>(func: &'static dyn Fn(String, Vec<String>) -> Fut) -> BotCommand
    where
        Fut: Future<Output = BotCommandResult> + FutureExt + Send
{
    Box::new(move |response_target, args| func(response_target, args).boxed())
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

    let mut commands : HashMap<&'static str, BotCommand> = HashMap::new();
    commands.insert("metar", make_boxed(&handle_metar));
    commands.insert("taf", make_boxed(&handle_taf));
    commands.insert("quit", make_boxed(&handle_quit));
    commands.insert("join", make_boxed(&handle_join));
    commands.insert("part", make_boxed(&handle_part));

    let config = Config::load(args.value_of("config-file").expect("default missing?")).unwrap();
    let leader = config.options.get("leader").map_or("&", String::as_str).to_owned();
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
                                if let Some(response_target) = message.response_target() {
                                    // Ignore messages without a response target
                                    futures.push(handler(response_target.to_owned(), args.to_vec().to_owned()).fuse());
                                }
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
