use std::collections::HashSet;
use log::{error, info, LevelFilter, warn};
use serde_derive::Deserialize;
use serde_derive::Serialize;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode, SimpleLogger};
use std::fs;
use std::sync::{mpsc, Arc, Mutex};
use cmd_lib::run_cmd;
use once_cell::sync::OnceCell;
use threadpool::ThreadPool;

mod fireplan;
mod parser;
mod web_server;

// Global static channel endpoints
static SENDER: OnceCell<mpsc::Sender<Event>> = OnceCell::new();

// Public helper to allow any thread to send an Event to main loop
pub fn send_event(event: Event) -> Result<(), mpsc::SendError<Event>> {
    if let Some(tx) = SENDER.get() {
        tx.send(event)
    } else {
        Err(mpsc::SendError(event))
    }
}

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

    // Initialize global channel
    let (tx, rx) = mpsc::channel::<Event>();
    let _ = SENDER.set(tx.clone());

    // Spawn a thread to listen for OS signals and send Shutdown
    {
        std::thread::spawn(|| {
            use signal_hook::consts::signal::*;
            use signal_hook::iterator::Signals;

            let mut signals = match Signals::new([SIGINT, SIGTERM, SIGHUP, SIGQUIT]) {
                Ok(s) => s,
                Err(e) => {
                    error!("Failed to register signal handlers: {e}");
                    return;
                }
            };

            for sig in signals.forever() {
                match sig {
                    SIGINT | SIGTERM | SIGHUP | SIGQUIT => {
                        let _ = send_event(Event::Shutdown);
                        break;
                    }
                    _ => {}
                }
            }
        });
    }

    // Shared known RICs set protected by a mutex for concurrent worker access
    let known_rics: Arc<Mutex<HashSet<(String, String)>>> = Arc::new(Mutex::new(HashSet::new()));

    // Thread pool with maximum size 20 to process Event::Data without blocking main loop
    let pool = ThreadPool::new(20);

    // Use the local receiver in the main loop
    loop {
        match rx.recv() {
            Ok(Event::Data(mut data)) => {
                let configuration = configuration.clone();
                let known_rics = Arc::clone(&known_rics);
                pool.execute(move || {
                    // Deduplicate RICs based on (einsatznrlst, ric)
                    let mut alarmier_rics: Vec<Ric> = vec![];
                    if let Ok(mut set) = known_rics.lock() {
                        for ric in &data.rics {
                            let key = (data.einsatznrlst.clone(), ric.ric.clone());
                            if !set.contains(&key) {
                                set.insert(key);
                                alarmier_rics.push(ric.clone());
                            }
                        }
                    } else {
                        warn!("Could not lock known_rics, skipping deduplication");
                        alarmier_rics = data.rics.clone();
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
                });
            }
            Ok(Event::Submit(payload)) => {
                let configuration = configuration.clone();
                pool.execute(move || {
                    match parser::parse(payload, configuration.clone()) {
                        Ok(parsed_data) => {
                            match send_event(Event::Data(parsed_data)) {
                                Ok(_) => info!("Parsed data sent to main loop"),
                                Err(e2) => error!("Failed to send parsed data: {}", e2),
                            }
                        }
                        Err(e) => {
                            error!("Failed to parse payload text: {}", e);
                        }
                    }
                });
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
