use std::collections::HashSet;
use crate::imap::monitor_postbox;
use log::{error, info, LevelFilter, warn};
use serde_derive::Deserialize;
use serde_derive::Serialize;
use simplelog::{ColorChoice, CombinedLogger, Config, TermLogger, TerminalMode};
use std::fs;
use std::sync::mpsc;
use std::thread::JoinHandle;
use cmd_lib::run_cmd;

// Actix Web imports
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};

// rustls (0.23) imports to enable HTTPS
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

mod fireplan;
mod imap;
mod parser;

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
    regex_einsatzstichwort: String,
    regex_strasse: String,
    regex_ort: String,
    regex_hausnummer: String,
    regex_ortsteil: String,
    regex_einsatznrleitstelle: String,
    regex_koordinaten: String,
    regex_zusatzinfo: String,
    regex_objektname: String,
    simple_trigger: Option<String>,
    rics: Vec<Ric>,
    http_port: u16,
    http_host: String,
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

// ----------------------
// Actix Web handlers (10 total)
// ----------------------
#[get("/")]
async fn root() -> impl Responder {
    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(
            r#"<!doctype html>
<html lang=\"en\">
<head>
  <meta charset=\"utf-8\" />
  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
  <title>Fireplan IMAP</title>
  <style>
    body { font-family: system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica, Arial, \"Apple Color Emoji\", \"Segoe UI Emoji\"; background: #0f172a; color: #e2e8f0; display: grid; place-items: center; min-height: 100vh; margin: 0; }
    .card { background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 28px 32px; box-shadow: 0 10px 30px rgba(0,0,0,.4); max-width: 680px; }
    h1 { margin: 0 0 12px; font-size: 36px; letter-spacing: .5px; }
    p { margin: 8px 0 0; color: #cbd5e1; }
    code { background: #0b1220; padding: 2px 6px; border-radius: 6px; }
  </style>
</head>
<body>
  <div class=\"card\">
    <h1>Howdy partner 👋</h1>
    <p>Welcome to the Fireplan DIVERA proxy service. Your server is up and running over <code>HTTPS</code>.</p>
  </div>
</body>
</html>"#,
        )
}

#[get("/health")]
async fn health() -> impl Responder { HttpResponse::Ok().body("OK") }

#[get("/ready")]
async fn ready() -> impl Responder { HttpResponse::Ok().body("READY") }

#[get("/version")]
async fn version() -> impl Responder {
    HttpResponse::Ok().body(env!("CARGO_PKG_VERSION"))
}

#[get("/status")]
async fn status() -> impl Responder { HttpResponse::Ok().json(serde_json::json!({"status":"ok"})) }

#[get("/time")]
async fn time() -> impl Responder {
    let now = chrono::Utc::now().to_rfc3339();
    HttpResponse::Ok().json(serde_json::json!({"utc": now}))
}

#[get("/metrics")]
async fn metrics() -> impl Responder { HttpResponse::Ok().body("# no metrics yet\n") }

#[get("/echo/{msg}")]
async fn echo(path: web::Path<String>) -> impl Responder { HttpResponse::Ok().body(path.into_inner()) }

#[get("/help")]
async fn help_page() -> impl Responder {
    HttpResponse::Ok().body("Use /, /health, /ready, /version, /status, /time, /metrics, /echo/{msg}, /help, /ping")
}

#[get("/ping")]
async fn ping() -> impl Responder { HttpResponse::Ok().body("pong") }

// Build rustls ServerConfig from Let's Encrypt files for the configured hostname
fn build_rustls_config(hostname: &str) -> anyhow::Result<rustls::ServerConfig> {
    let base = format!("/etc/letsencrypt/live/{hostname}");
    let cert_path = format!("{base}/fullchain.pem");
    let key_path = format!("{base}/privkey.pem");

    let mut cert_file = std::io::BufReader::new(
        std::fs::File::open(&cert_path)
            .map_err(|e| anyhow::anyhow!("failed to open cert file {cert_path}: {e}"))?,
    );
    let mut key_file = std::io::BufReader::new(
        std::fs::File::open(&key_path)
            .map_err(|e| anyhow::anyhow!("failed to open key file {key_path}: {e}"))?,
    );

    let cert_chain: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut cert_file)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("failed to parse certs: {e}"))?;

    // Try parsing any private key format supported
    let key: PrivateKeyDer<'static> = {
        // Try PKCS#8 first
        let pkcs8_candidate = {
            let mut key_file = std::io::BufReader::new(
                std::fs::File::open(&key_path)
                    .map_err(|e| anyhow::anyhow!("failed to open key file {key_path}: {e}"))?,
            );
            let res = rustls_pemfile::pkcs8_private_keys(&mut key_file).next();
            res
        };
        if let Some(Ok(k)) = pkcs8_candidate {
            PrivateKeyDer::from(k)
        } else {
            // Try EC (SEC1)
            let ec_candidate = {
                let mut key_file = std::io::BufReader::new(
                    std::fs::File::open(&key_path)
                        .map_err(|e| anyhow::anyhow!("failed to open key file {key_path}: {e}"))?,
                );
                let res = rustls_pemfile::ec_private_keys(&mut key_file).next();
                res
            };
            if let Some(Ok(k)) = ec_candidate {
                PrivateKeyDer::from(k)
            } else {
                // Try legacy RSA (PKCS#1)
                let rsa_candidate = {
                    let mut key_file = std::io::BufReader::new(
                        std::fs::File::open(&key_path)
                            .map_err(|e| anyhow::anyhow!("failed to open key file {key_path}: {e}"))?,
                    );
                    let res = rustls_pemfile::rsa_private_keys(&mut key_file).next();
                    res
                };
                if let Some(Ok(k)) = rsa_candidate {
                    PrivateKeyDer::from(k)
                } else {
                    return Err(anyhow::anyhow!("no valid private key found in {key_path}"));
                }
            }
        }
    };

    let cfg = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| anyhow::anyhow!("rustls config error: {e}"))?;

    Ok(cfg)
}

fn start_https_server(cfg: Configuration) -> std::io::Result<JoinHandle<()>> {
    let host = cfg.http_host.clone();
    let port = cfg.http_port;
    let addr = format!("0.0.0.0:{port}");

    // Build rustls config up-front to fail fast if missing certs
    let tls_config = match build_rustls_config(&host) {
        Ok(c) => c,
        Err(e) => {
            // Map to io::Error to satisfy return type; also log error
            error!("TLS configuration failed: {e}");
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };

    let handle = std::thread::spawn(move || {
        info!("Starting HTTPS server on https://{host}:{port}");
        let sys = actix_web::rt::System::new();
        sys.block_on(async move {
            let server = HttpServer::new(|| {
                App::new()
                    .service(root)
                    .service(health)
                    .service(ready)
                    .service(version)
                    .service(status)
                    .service(time)
                    .service(metrics)
                    .service(echo)
                    .service(help_page)
                    .service(ping)
            })
            .bind_rustls_0_23(addr, tls_config)
            .expect("failed to bind HTTPS socket")
            .run();

            if let Err(e) = server.await {
                error!("HTTPS server error: {e}");
            }
        });
    });

    Ok(handle)
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

    CombinedLogger::init(vec![TermLogger::new(
        LevelFilter::Info,
        Config::default(),
        TerminalMode::Mixed,
        ColorChoice::Auto,
    )])
    .unwrap();

    let mut configuration_output = format!("Configuration: {:?}", configuration);


    info!("Configuration: {}", configuration_output);

    // Start HTTPS web server (actix) before receiving from channel
    if let Err(e) = start_https_server(configuration.clone()) {
        error!("Failed to start HTTPS server: {e}");
    }

    // let mut threads: Vec<JoinHandle<()>> = vec![];
    // let my_standorte = configuration.standorte.clone();

     let (tx, rx) = mpsc::channel::<ParsedData>();

    // for standort in my_standorte {
    //     let my_standort = standort.clone();
    //     let my_configuration = configuration.clone();
    //     let my_tx = tx.clone();
    //     let handle = std::thread::spawn(move || {
    //         match monitor_postbox(my_tx, my_standort, my_configuration.clone()) {
    //             Ok(_) => {
    //                 info!("monitor done: {}", standort.standort)
    //             }
    //             Err(e) => {
    //                 error!("monitor failed: {}, {}", standort.standort, e)
    //             }
    //         }
    //     });
    //     threads.push(handle);
    // }

    let mut known_rics : HashSet<(String,String)> = HashSet::new();

    loop {
        match rx.recv() {
            Ok(mut data) => {
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
            Err(e) => {
                error!("Receive error: {}", e);
            }
        }
    }
}
