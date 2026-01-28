use log::{error, info};
use std::thread::JoinHandle;

// Actix Web imports
use actix_web::{get, web, App, HttpResponse, HttpServer, Responder};

// rustls (0.23) imports to enable HTTPS
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

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
    a { color: #93c5fd; text-decoration-color: #1e293b; }
    a:hover { color: #bfdbfe; }
  </style>
</head>
<body>
  <div class=\"card\">
    <h1>Howdy partner 👋</h1>
    <p>Welcome to the Fireplan DIVERA proxy service. Your <a href=\"/metrics\">server</a> is up and running over <code>HTTPS</code>.</p>
  </div>
</body>
</html>"#,
        )
}

#[get("/health")]
async fn health() -> impl Responder {
    let ts = chrono::Utc::now().to_rfc3339();
    HttpResponse::Ok().json(serde_json::json!({"status":"OK","timestamp": ts}))
}

#[get("/ready")]
async fn ready() -> impl Responder {
    let ts = chrono::Utc::now().to_rfc3339();
    HttpResponse::Ok().json(serde_json::json!({"status":"READY","timestamp": ts}))
}

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
async fn metrics() -> impl Responder {
    use sysinfo::{System, CpuRefreshKind, RefreshKind, MemoryRefreshKind};

    let refresh = RefreshKind::new()
        .with_memory(MemoryRefreshKind::new().with_ram().with_swap())
        .with_cpu(CpuRefreshKind::everything())
        .with_processes(sysinfo::ProcessRefreshKind::new());

    let mut sys = System::new_with_specifics(refresh);
    sys.refresh_specifics(refresh);

    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory();
    let total_swap = sys.total_swap();
    let used_swap = sys.used_swap();
    let avg_cpu = sys.global_cpu_usage();
    let cpu_cores = sys.cpus().len() as u64;
    let processes_total = sys.processes().len() as u64;

    HttpResponse::Ok().json(serde_json::json!({
        "memory": {
            "total_bytes": total_mem,
            "used_bytes": used_mem,
        },
        "swap": {
            "total_bytes": total_swap,
            "used_bytes": used_swap,
        },
        "cpu": {
            "avg_usage_percent": avg_cpu,
            "cores": cpu_cores,
        },
        "processes_total": processes_total,
    }))
}

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

pub fn start_https_server(http_host: String, http_port: u16) -> std::io::Result<JoinHandle<()>> {
    let addr = format!("0.0.0.0:{http_port}");

    // Build rustls config up-front to fail fast if missing certs
    let tls_config = match build_rustls_config(&http_host) {
        Ok(c) => c,
        Err(e) => {
            // Map to io::Error to satisfy return type; also log error
            error!("TLS configuration failed: {e}");
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };

    let handle = std::thread::spawn(move || {
        info!("Starting HTTPS server on https://{}:{}", http_host, http_port);
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
