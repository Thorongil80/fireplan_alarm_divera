use std::collections::HashSet;
use log::{error, info, LevelFilter, warn};
use serde_derive::Deserialize;
use serde_derive::Serialize;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, SimpleLogger};
use std::fs;
use std::sync::mpsc;
use cmd_lib::run_cmd;

mod fireplan;
mod parser;
mod web_server;

#[derive(Clone, Serialize, Deserialize, Eq, Hash, PartialEq, Debug)]
pub struct Standort {
    standort: String,
    imap_server: String,
    imap_port: u16,
    imap_user: String,
    imap_password: String,
    additional_rics: Option<Vec<Ric>>
}

#[derive(Clone, Serialize, Deserialize, Eq, Hash, PartialEq, Debug)]
pub struct Ric {
    text: String,
    ric: String,
    subric: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Configuration {
    fireplan_api_key: String,
    regex_ort: String,
    regex_ortsteil: String,
    regex_objektname: String,
    simple_trigger: Option<String>,
    rics: Vec<Ric>,
    http_port: u16,
    http_host: String,
    auth_token: String,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ParsedData {
    rics: Vec<Ric>,
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

// Incoming JSON payload structure for submit
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SubmitPayload {
    id: u64,
    foreign_id: String,
    title: String,
    text: String,
    address: String,
    lat: String,
    lng: String,
    priority: u8,
    cluster: Vec<String>,
    group: Vec<String>,
    vehicle: Vec<String>,
    ts_create: i64,
    ts_update: i64,
}

// New event enum to transport richer context
#[derive(Clone, Debug)]
pub enum Event {
    Data(ParsedData),
    Submit(SubmitPayload),
    Shutdown,
}

fn main() {
    let file = if cfg!(windows) {
        format!(
            "{}\\fireplan_alarm_divera.conf",
            std::env::var("USERPROFILE").unwrap()
        )
    } else {
        format!(
            "{}/fireplan_alarm_divera.conf",
            homedir::my_home().unwrap().unwrap().to_string_lossy()
        )
    };
    let content = fs::read_to_string(file).expect("Config file missing!");
    let configuration: Configuration = toml::from_str(content.as_str()).unwrap();

    // Robust logger init: use TermLogger when TTY is available, otherwise fallback to SimpleLogger
    let term = TermLogger::new(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    );
    CombinedLogger::init(vec![term]).unwrap_or_else(|_| {
        CombinedLogger::init(vec![SimpleLogger::new(LevelFilter::Info, Config::default())]).unwrap();
    });

    info!("Configuration: {:?}", configuration);

    // Start HTTPS web server (actix) before receiving from channel
    if let Err(e) = web_server::start_https_server(configuration.http_host.clone(), configuration.http_port, configuration.auth_token.clone()) {
        error!("Failed to start HTTPS server: {e}");
    }

    // Channel now carries Event
    let (tx, rx) = mpsc::channel::<Event>();

    // Spawn a thread to listen for OS signals and send Shutdown
    {
        let tx_sig = tx.clone();
        std::thread::spawn(move || {
            use signal_hook::consts::signal::*;
            use signal_hook::iterator::Signals;

            let mut signals = match Signals::new(&[SIGINT, SIGTERM, SIGHUP, SIGQUIT]) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to register signal handlers: {e}");
                    return;
                }
            };

            for sig in signals.forever() {
                match sig {
                    SIGINT | SIGTERM | SIGHUP | SIGQUIT => {
                        let _ = tx_sig.send(Event::Shutdown);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }

    let mut known_rics : HashSet<(String,String)> = HashSet::new();

    loop {
        match rx.recv() {
            Ok(Event::Data(mut data)) => {
                let mut alarmier_rics: Vec<Ric> = vec![];
                for ric in &data.rics {
                    if ! known_rics.contains(&(data.einsatznrlst.clone(), ric.ric.clone())) {
                        known_rics.insert((data.einsatznrlst.clone(), ric.ric.clone()));
                        alarmier_rics.push(ric.clone());
                    }
                }
                if alarmier_rics.is_empty() {
                    warn!("All contained RICs already submitted for this EinsatzNrLeitstelle, do not submit this alarm")
                } else {
                    data.rics = alarmier_rics;
                    info!("Submitting to Fireplan Standort Verwaltung");
                    fireplan::submit("Verwaltung".to_string(), configuration.fireplan_api_key.clone(), data);
                    if let Some(script_path) = configuration.simple_trigger.clone() {
                        info!("Executing simple trigger");
                        match run_cmd!($script_path) {
                            Ok(()) => info!("Execute ok"),
                            Err(e) => error!("Failure: {e}")
                        }
                    }
                }
            }
            Ok(Event::Submit(payload)) => {
                match parser::parse(payload, configuration.clone()) {
                    Ok(parsed_data) => {
                        match tx.send(Event::Data(parsed_data)) {
                            Ok(_) => info!("Parsed data sent to main loop"),
                            Err(e2) => error!("Failed to send parsed data: {}", e2),
                        }
                    }
                    Err(e) => {
                        error!("Failed to parse payload text: {}", e);
                    }
                }
            }
            Ok(Event::Shutdown) => {
                info!("Shutdown event received, exiting main loop");
                break;
            }
            Err(e) => {
                error!("Receive error: {}", e);
                break;
            }
        }
    }
}
