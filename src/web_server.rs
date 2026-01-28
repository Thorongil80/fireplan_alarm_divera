use log::{error, info};
use std::thread::JoinHandle;

// Actix Web imports
use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use actix_web::middleware::Logger as ActixLogger;

// rustls (0.23) imports to enable HTTPS
use rustls::pki_types::{CertificateDer, PrivateKeyDer};

// Shared app state for handlers
#[derive(Clone)]
pub struct AppState {
    pub auth_token: String,
}

// Query parameter for token
#[derive(serde::Deserialize)]
struct QueryToken {
    token: String,
}

// ----------------------
// Actix Web handlers (9 total)
// ----------------------
#[get("/")]
async fn root() -> impl Responder {
    let ts = chrono::Utc::now().to_rfc3339();
    let html = format!(r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Fireplan IMAP</title>
  <style>
    body {{ font-family: system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica, Arial, "Apple Color Emoji", "Segoe UI Emoji"; background: #0f172a; color: #e2e8f0; display: grid; place-items: center; min-height: 100vh; margin: 0; }}
    .card {{ background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 28px 32px; box-shadow: 0 10px 30px rgba(0,0,0,.4); max-width: 720px; }}
    h1 {{ margin: 0 0 12px; font-size: 36px; letter-spacing: .5px; }}
    p {{ margin: 8px 0 0; color: #cbd5e1; }}
    code {{ background: #0b1220; padding: 2px 6px; border-radius: 6px; }}
    small {{ color: #94a3b8; display: block; margin-top: 12px; }}
    a {{ color: #93c5fd; text-decoration-color: #1e293b; }}
    a:hover {{ color: #bfdbfe; }}
    .status {{ display: inline-flex; align-items: center; gap: 8px; }}
    .dot {{ width: 8px; height: 8px; border-radius: 50%; background: #22c55e; box-shadow: 0 0 0 3px rgba(34,197,94,.25); }}
  </style>
</head>
<body>
  <div class="card">
    <h1>Howdy partner 👋</h1>
    <p>Welcome to the Fireplan DIVERA proxy service. Your <a href="/metrics">server</a> is up and running over <code>HTTPS</code>.</p>
    <small class="status"><span class="dot"></span> Healthy · {ts}</small>
  </div>
</body>
</html>"#);

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
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

fn fmt_bytes_gib_mib(bytes: u64) -> String {
    const MIB: f64 = 1024.0 * 1024.0;
    const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
    if (bytes as f64) >= GIB {
        format!("{:.2} GiB", (bytes as f64) / GIB)
    } else {
        format!("{:.2} MiB", (bytes as f64) / MIB)
    }
}

#[get("/metrics")]
async fn metrics() -> impl Responder {
    use sysinfo::{System, CpuRefreshKind, RefreshKind, MemoryRefreshKind};

    let refresh = RefreshKind::everything()
        .with_memory(MemoryRefreshKind::everything().with_ram().with_swap())
        .with_cpu(CpuRefreshKind::everything())
        .with_processes(sysinfo::ProcessRefreshKind::everything());

    let mut sys = System::new_with_specifics(refresh);
    sys.refresh_specifics(refresh);

    let total_mem = sys.total_memory();
    let used_mem = sys.used_memory();
    let total_swap = sys.total_swap();
    let used_swap = sys.used_swap();
    let avg_cpu = sys.global_cpu_usage();
    let cpu_cores = sys.cpus().len() as u64;
    let processes_total = sys.processes().len() as u64;

    let ts = chrono::Utc::now().to_rfc3339();

    let total_mem_fmt = fmt_bytes_gib_mib(total_mem);
    let used_mem_fmt = fmt_bytes_gib_mib(used_mem);
    let total_swap_fmt = fmt_bytes_gib_mib(total_swap);
    let used_swap_fmt = fmt_bytes_gib_mib(used_swap);

    let html = format!(r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>Server Metrics</title>
  <style>
    body {{ font-family: system-ui, -apple-system, Segoe UI, Roboto, Ubuntu, Cantarell, Noto Sans, Helvetica, Arial, "Apple Color Emoji", "Segoe UI Emoji"; background: #0f172a; color: #e2e8f0; min-height: 100vh; margin: 0; }}
    .wrap {{ max-width: 860px; margin: 0 auto; padding: 24px; }}
    .card {{ background: #111827; border: 1px solid #1f2937; border-radius: 12px; padding: 24px 28px; box-shadow: 0 10px 30px rgba(0,0,0,.4); margin-top: 24px; }}
    h1 {{ margin: 0; font-size: 28px; }}
    h2 {{ margin: 18px 0 8px; font-size: 18px; color: #cbd5e1; }}
    p, li {{ color: #cbd5e1; }}
    small {{ color: #94a3b8; display: block; margin-top: 10px; }}
    .grid {{ display: grid; grid-template-columns: repeat(auto-fit, minmax(240px, 1fr)); gap: 16px; }}
    .item {{ background: #0b1220; border: 1px solid #1f2937; border-radius: 10px; padding: 14px; }}
    .muted {{ color: #94a3b8; }}
    a {{ color: #93c5fd; text-decoration-color: #1e293b; }}
    a:hover {{ color: #bfdbfe; }}
  </style>
</head>
<body>
  <div class="wrap">
    <div class="card">
      <h1>Server Metrics</h1>
      <small class="muted">Updated · {ts}</small>
      <div class="grid">
        <div class="item">
          <h2>CPU</h2>
          <ul>
            <li>Average usage: {avg_cpu:.1}%</li>
            <li>Cores: {cpu_cores}</li>
          </ul>
        </div>
        <div class="item">
          <h2>Memory</h2>
          <ul>
            <li>Total: {total_mem_fmt}</li>
            <li>Used: {used_mem_fmt}</li>
          </ul>
        </div>
        <div class="item">
          <h2>Swap</h2>
          <ul>
            <li>Total: {total_swap_fmt}</li>
            <li>Used: {used_swap_fmt}</li>
          </ul>
        </div>
        <div class="item">
          <h2>Processes</h2>
          <ul>
            <li>Total: {processes_total}</li>
          </ul>
        </div>
      </div>
      <small class="muted"><a href="/">Back to Home</a></small>
    </div>
  </div>
</body>
</html>"#);

    HttpResponse::Ok()
        .content_type("text/html; charset=utf-8")
        .body(html)
}

#[get("/echo/{msg}")]
async fn echo(path: web::Path<String>) -> impl Responder { HttpResponse::Ok().body(path.into_inner()) }

#[get("/help")]
async fn help_page() -> impl Responder {
    HttpResponse::Ok().body("Use /, /health, /ready, /version, /status, /time, /metrics, /echo/{msg}, /help, /ping")
}

#[get("/ping")]
async fn ping() -> impl Responder { HttpResponse::Ok().body("pong") }

#[post("/submit")]
async fn submit(
    query: web::Query<QueryToken>,
    body: web::Bytes,
    state: web::Data<AppState>,
) -> impl Responder {
    if query.token != state.auth_token {
        error!("Invalid auth token");
        return HttpResponse::Unauthorized().json(serde_json::json!({
            "error": "Unauthorized",
        }));
    }

    info!("Received /submit request with body length: {}", body.len());
    info!("Received: {}", String::from_utf8_lossy(&body));

    match serde_json::from_slice::<crate::SubmitPayload>(&body) {
        Ok(data) => {
            let _ = crate::send_event(crate::Event::Submit(data.clone()));
            info!("Received: {:?}", data);
            HttpResponse::Ok().json(serde_json::json!({
                "status": "submitted"
            }))
        },
        Err(e) => {
            error!("Invalid payload: {}", e);
            let example = serde_json::json!({
                "id": 247,
                "number": "E-123",
                "title": "FEUER3",
                "text": "Unklare Rauchentwicklung im Hafen",
                "address": "Hauptstraße 247, 12345 Musterstadt",
                "lat": "1.23456",
                "lng": "12.34567",
                "priority": 1,
                "cluster": ["Untereinheit 1"],
                "group": ["Gruppe 1", "Gruppe 2"],
                "vehicle": ["HLF-1", "LF-10"],
                "ts_create": 1769601252,
                "ts_update": 1769601252
            });
            HttpResponse::BadRequest().json(serde_json::json!({
                "error": format!("JSON parse error: {}", e),
                "example": example,
            }))
        }
    }
}

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

pub fn start_https_server(http_host: String, http_port: u16, auth_token: String) -> std::io::Result<JoinHandle<()>> {
    let addr = format!("0.0.0.0:{http_port}");

    // Build rustls config up-front to fail fast if missing certs
    let tls_config = match build_rustls_config(&http_host) {
        Ok(c) => c,
        Err(e) => {
            error!("TLS configuration failed: {e}");
            return Err(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()));
        }
    };

    let handle = std::thread::spawn(move || {
        info!("Starting HTTPS server on https://{}:{}", http_host, http_port);
        let sys = actix_web::rt::System::new();
        sys.block_on(async move {
            let app_state = web::Data::new(AppState { auth_token });
            let server = HttpServer::new(move || {
                App::new()
                    .wrap(ActixLogger::default())
                    .app_data(app_state.clone())
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
                    .service(submit)
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
