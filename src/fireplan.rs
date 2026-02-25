use crate::ParsedData;
use log::{error, info};
use reqwest::blocking::Client;
use serde_derive::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::Write;
use std::time::{Duration, Instant};
use std::collections::HashMap;
use once_cell::sync::Lazy;
use std::sync::Mutex;

#[derive(Clone, Serialize, Deserialize, Eq, Hash, PartialEq, Debug)]
struct FireplanAlarm {
    ric: String,
    subRIC: String,
    einsatznrlst: String,
    strasse: String,
    hausnummer: String,
    ort: String,
    ortsteil: String,
    objektname: String,
    koordinaten: String,
    einsatzstichwort: String,
    zusatzinfo: String,
}



#[derive(Clone, Serialize, Deserialize, Eq, Hash, PartialEq, Debug)]
struct ApiKey {
    utoken: String,
}

// Token cache: standort -> (token, stored_at)
static TOKEN_CACHE: Lazy<Mutex<HashMap<String, (String, Instant)>>> = Lazy::new(|| Mutex::new(HashMap::new()));
const TOKEN_TTL: Duration = Duration::from_secs(30 * 60);

fn get_api_token(client: &Client, standort: &str, api_key: &str) -> Option<String> {
    // Try cached value
    if let Ok(cache) = TOKEN_CACHE.lock() {
        if let Some((tok, ts)) = cache.get(standort) {
            if ts.elapsed() < TOKEN_TTL {
                info!("Returning token from cache, stored {:?}", ts);
                return Some(tok.clone());
            }
        }
    }

    // Fetch fresh token
    let token_string = match client
        .get(format!(
            "https://data.fireplan.de/api/Register/{}",
            standort
        ))
        .header("API-Key", api_key.to_string())
        .header("accept", "*/*")
        .send()
    {
        Ok(r) => {
            if r.status().is_success() {
                match r.text() {
                    Ok(t) => t,
                    Err(e) => {
                        error!("[{}] - Could not get API Key body: {}", standort, e);
                        return None;
                    }
                }
            } else {
                error!(
                    "[{}] - Could not get API Key: {:?}",
                    standort,
                    r.status()
                );
                return None;
            }
        }
        Err(e) => {
            error!("[{}] - Could not get API Key: {}", standort, e);
            return None;
        }
    };


    info!("Retrieved token from fireplan API");
    let token: ApiKey = match serde_json::from_str(&token_string) {
        Ok(apikey) => apikey,
        Err(e) => {
            error!("could not deserialize token key: {}", e);
            return None;
        }
    };

    // Store in cache
    if let Ok(mut cache) = TOKEN_CACHE.lock() {
        cache.insert(standort.to_string(), (token.utoken.clone(), Instant::now()));
        info!("Stored token in cache for standort {}", standort);
    }

    Some(token.utoken)
}

pub fn submit(standort: String, api_key: String, data: ParsedData) {
    info!("[{}] - Fireplan submit triggered", standort);

    let client = Client::new();

    // Use cached or freshly fetched token
    let api_token = match get_api_token(&client, &standort, &api_key) {
        Some(t) => t,
        None => return,
    };

    info!("[{}] - using cached/fetched API Token", standort);

    for ric in data.rics {
        let alarm = FireplanAlarm {
            ric: ric.ric,
            subRIC: ric.subric,
            einsatznrlst: data.einsatznrlst.clone(),
            strasse: data.strasse.clone(),
            hausnummer: data.hausnummer.clone(),
            ort: data.ort.clone(),
            ortsteil: data.ortsteil.clone(),
            objektname: data.objektname.clone(),
            koordinaten: data.koordinaten.clone(),
            einsatzstichwort: data.einsatzstichwort.clone(),
            zusatzinfo: data.zusatzinfo.clone(),
        };

        info!("[{}] - submitting Alarm: {:?}", standort, alarm);

        match client
            .post("https://data.fireplan.de/api/Alarmierung")
            .header("API-Token", api_token.clone())
            .header("accept", "*/*")
            .json(&alarm)
            .send()
        {
            Ok(r) => {
                if r.status().is_success() {
                    // On success, append timestamp and "einsatznrlst - einsatzstichwort" to the submitted log file
                    let ts = chrono::Utc::now().to_rfc3339();
                    let line = format!(
                        "{}\t{} - {}\n",
                        ts,
                        data.einsatznrlst.as_str(),
                        data.einsatzstichwort.as_str()
                    );
                    if let Err(e) = OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open("/root/fireplan_alarm_divera_submitted")
                        .and_then(|mut f| f.write_all(line.as_bytes()))
                    {
                        error!("[{}] - Failed to write submission log: {}", standort, e);
                    }

                    match r.text() {
                        Ok(t) => {
                            info!("[{}] - Posted alarm, server says: {}", standort, t)
                        }
                        Err(e) => {
                            error!("[{}] - Could get result text: {}", standort, e);
                            continue;
                        }
                    }
                } else {
                    error!(
                        "[{}] - Could not post alarm: {:?}",
                        standort,
                        r.status()
                    );
                    match r.text() {
                        Ok(t) => info!("[{}] - server says: {}", standort, t),
                        Err(e) => {
                            error!("[{}] - Could not get result text: {}", standort, e);
                            continue;
                        }
                    }
                    continue;
                }
            }
            Err(e) => {
                error!("[{}] - Could not post alarm: {}", standort, e);
                continue;
            }
        }
    }
}
