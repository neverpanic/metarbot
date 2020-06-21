//! Module that provides METAR and TAF weather reports, downloaded from api.met.no

#![deny(unsafe_code)]
#![deny(missing_docs)]

extern crate async_trait;
extern crate irc;
extern crate regex;

use std::fmt;
use std::time;

use crate::{
    BotCommand,
    BotCommandResult,
    BotError,
    BotParameters,
    BotResponse,
};

static METAR_API_URL: &str = "https://api.met.no/weatherapi/tafmetar/1.0/metar";
static TAF_API_URL: &str = "https://api.met.no/weatherapi/tafmetar/1.0/taf";
lazy_static! {
    static ref AIRPORT_RE: regex::Regex = regex::Regex::new(r"^(?i)[a-z0-9]{4}$").unwrap();
    static ref REQWEST: reqwest::Client = reqwest::Client::new();
}

struct MetarCommand {}
struct TafCommand {}

/**
 * Factory function that will create instances of all implemented commands in this module.
 */
pub fn mk() -> Vec<Box<dyn BotCommand>> {
    vec![
        Box::new(MetarCommand{}),
        Box::new(TafCommand{}),
    ]
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

#[derive(Debug)]
enum MetarError {
    EmptyResponse,
    NonSuccessResponse(reqwest::StatusCode),
    ReqwestError(reqwest::Error),
}

impl fmt::Display for MetarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MetarError::EmptyResponse =>
                write!(f, "Received empty response"),
            MetarError::NonSuccessResponse(statuscode) =>
                write!(f, "{}", statuscode),
            MetarError::ReqwestError(err) =>
                write!(f, "ReqwestError: {}", err),
        }
    }
}

async fn download(type_: WeatherType, airport: &str) -> Result<String, MetarError> {
    let url = match type_ {
        WeatherType::METAR => METAR_API_URL,
        WeatherType::TAF => TAF_API_URL,
    };

    let result = REQWEST.get(url)
        .header("Accept", "text/plain")
        .timeout(time::Duration::from_secs(5))
        .query(&[("icao", airport)])
        .send()
        .await;

    match result {
        Err(err) =>
            Err(MetarError::ReqwestError(err)),
        Ok(response) =>
            if !response.status().is_success() {
                Err(MetarError::NonSuccessResponse(response.status()))
            } else {
                match response.text().await {
                    Err(err) =>
                        Err(MetarError::ReqwestError(err)),
                    Ok(string) =>
                        match string.lines().rfind(|item| !item.trim().is_empty()) {
                            None =>
                                Err(MetarError::EmptyResponse),
                            Some(metar) =>
                                Ok(metar.to_string()),
                        },
                }
            },
    }
}

async fn handle(type_: WeatherType, params: BotParameters) -> BotCommandResult {
    let response_target = params.message
        .response_target()
        .ok_or(BotError::NoResponseTarget)?
        .to_string();

    if let Some(airport) = params.args.get(0) {
        if AIRPORT_RE.is_match(airport) {
            Ok(BotResponse::Privmsg(
                response_target,
                match download(type_, airport).await {
                    Ok(metar) =>
                        metar,
                    Err(err) =>
                        format!("Error: {}", err),
                }))
        } else {
            Ok(BotResponse::Privmsg(
                response_target,
                format!("{} does not seem to be a valid ICAO airport code", airport)))
        }
    } else {
        Ok(BotResponse::Privmsg(
            response_target,
            format!("Usage: {}{} <4-letter ICAO airport code>",
                params.leader,
                type_.to_string().to_lowercase()),
        ))
    }
}

#[async_trait::async_trait]
impl BotCommand for MetarCommand {
    fn trigger(&self) -> &'static str {
        "metar"
    }

    async fn handle(&self, params: BotParameters) -> BotCommandResult {
        handle(WeatherType::METAR, params).await
    }
}

#[async_trait::async_trait]
impl BotCommand for TafCommand {
    fn trigger(&self) -> &'static str {
        "taf"
    }

    async fn handle(&self, params: BotParameters) -> BotCommandResult {
        handle(WeatherType::TAF, params).await
    }
}
