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

static STATION_API_URL: &str = "https://avwx.rest/api/station/";
static METAR_API_URL: &str = "https://avwx.rest/api/metar/";
static TAF_API_URL: &str = "https://avwx.rest/api/taf/";

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
    NonSuccessResponse(reqwest::StatusCode),
    NoData(String, String),
    ReqwestError(reqwest::Error),
}

impl fmt::Display for MetarError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            MetarError::NonSuccessResponse(statuscode) =>
                write!(f, "{}", statuscode),
            MetarError::NoData(icao, name) =>
                write!(f, "{} ({}) is not reporting weather", icao, name),
            MetarError::ReqwestError(err) =>
                write!(f, "ReqwestError: {}", err),
        }
    }
}

#[derive(Deserialize)]
struct TafMetarJson {
    raw: String,
}

#[derive(Deserialize)]
struct Station {
    name: String,
    icao: String,
    reporting: bool,
}

async fn info(airport: &str, apikey: &str) -> Result<Station, MetarError> {
    let result = REQWEST.get(&[STATION_API_URL, airport].concat())
        .header("Accept", "application/json")
        .header("Authorization", ["Bearer", apikey].join(" "))
        .timeout(time::Duration::from_secs(5))
        .send()
        .await;

    match result {
        Err(err) =>
            Err(MetarError::ReqwestError(err)),
        Ok(response) =>
            if !response.status().is_success() {
                Err(MetarError::NonSuccessResponse(response.status()))
            } else {
                match response.json::<Station>().await {
                    Err(err) =>
                        Err(MetarError::ReqwestError(err)),
                    Ok(station) =>
                        Ok(station),
                }
            },
    }
}

async fn weather(type_: WeatherType, airport: &str, apikey: &str) -> Result<String, MetarError> {
    let info = info(airport, apikey).await?;
    if !info.reporting {
        return Err(MetarError::NoData(info.icao, info.name));
    }

    let url = match type_ {
        WeatherType::METAR => METAR_API_URL,
        WeatherType::TAF => TAF_API_URL,
    };

    let result = REQWEST.get(&[url, airport].concat())
        .header("Accept", "application/json")
        .header("Authorization", ["Bearer", apikey].join(" "))
        .timeout(time::Duration::from_secs(5))
        .send()
        .await;

    match result {
        Err(err) =>
            Err(MetarError::ReqwestError(err)),
        Ok(response) =>
            if !response.status().is_success() {
                Err(MetarError::NonSuccessResponse(response.status()))
            } else if response.status() == reqwest::StatusCode::NO_CONTENT {
                Err(MetarError::NoData(info.icao, info.name))
            } else {
                match response.json::<TafMetarJson>().await {
                    Err(err) =>
                        Err(MetarError::ReqwestError(err)),
                    Ok(data) =>
                        Ok(data.raw),
                }
            },
    }
}

async fn handle(type_: WeatherType, params: BotParameters<'_>) -> BotCommandResult {
    let response_target = params.message
        .response_target()
        .ok_or(BotError::NoResponseTarget)?
        .to_string();

    let apikey = params.options.get("avwx_apikey")
        .ok_or(BotError::Unconfigured("avwx_apikey not set"))?;

    if let Some(airport) = params.args.get(0) {
        if AIRPORT_RE.is_match(airport) {
            Ok(BotResponse::Privmsg(
                response_target,
                match weather(type_, airport, apikey).await {
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
                params.leaders.get(0).map_or("".to_string(), char::to_string),
                type_.to_string().to_lowercase()),
        ))
    }
}

#[async_trait::async_trait]
impl BotCommand for MetarCommand {
    fn trigger(&self) -> &'static str {
        "metar"
    }

    async fn handle(&self, params: BotParameters<'_>) -> BotCommandResult {
        handle(WeatherType::METAR, params).await
    }
}

#[async_trait::async_trait]
impl BotCommand for TafCommand {
    fn trigger(&self) -> &'static str {
        "taf"
    }

    async fn handle(&self, params: BotParameters<'_>) -> BotCommandResult {
        handle(WeatherType::TAF, params).await
    }
}
