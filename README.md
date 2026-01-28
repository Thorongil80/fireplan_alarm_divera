# Fireplan DIVERA Proxy (fireplan_alarm_divera)

This service receives HTTPS webhook-like JSON submissions (e.g., from DIVERA 24/7 or similar integrations), parses the payload in the style of “ILS Karlsruhe,” and forwards alarms to Fireplan. It also serves an HTTPS status UI with health, readiness, metrics, and small operational logs of received/submitted alarms.

Two full documentation sections are provided: first in English (American), then in German (Germany).

---

## Documentation (English)

### Overview
- Purpose: Accept alarms from external systems via a secure HTTPS endpoint, validate/authenticate, parse structured text into normalized fields, and submit alarms to Fireplan.
- Audience: Operators of FF Ubstadt-Weiher who need reliable bridging between DIVERA and Fireplan, including support for dummy RICs and always-present KdoW RIC to guarantee alarm coverage across departments.
- Transport: Actix Web server over HTTPS (rustls 0.23) using Let’s Encrypt certificates at `/etc/letsencrypt/live/<hostname>/`.
- Configuration: Loaded from `fireplan_alarm_divera.conf` in the home directory. Includes:
  - `http_host` and `http_port`: Bind host and port.
  - `auth_token`: Pre-shared token validated via `token` query param on `/submit`.
  - `fireplan_api_key`: API key used to obtain a temporary Fireplan API-Token.
  - Regexes: `regex_ort`, `regex_ortsteil`, `regex_objektname` used to extract fields from textual body.
  - `rics`: A list of configured target RICs, including dummy/KdoW behavior.
  - Optional `simple_trigger`: a script path to run on successful submission.

### Security
- HTTPS-encrypted endpoints (no plaintext HTTP).
- `/submit` requires the `token` query parameter to match `auth_token`. Mismatches return `401 Unauthorized`.
- No sensitive application versions are exposed in metrics or UI.

### Runtime behavior
- Web server starts before the receiver loop.
- Signals (SIGINT/SIGTERM/SIGHUP/SIGQUIT) are trapped; a Shutdown event is sent to the main loop to exit cleanly.
- Logging: Unified logger prints to stdout/stderr (suitable for systemd). Actix request logs are emitted via the same backend.

### Data formats
- JSON POST to `/submit?token=<auth>` with `Content-Type: application/json`.
- Payload fields (lat/lng are strings to avoid float inaccuracies):
  ```json
  {
    "id": 247,
    "foreign_id": "<external-message-id>",
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
  }
  ```

### Endpoint reference
All endpoints are served over HTTPS on the configured host/port.

- `GET /`
  - Returns a status HTML page with:
    - Welcome message.
    - Timestamped health snippet.
    - Link to `/metrics` when clicking “server”.
    - Two scrollable log boxes (newest first):
      - “Received” shows `/root/fireplan_alarm_divera_received` lines.
      - “Submitted” shows `/root/fireplan_alarm_divera_submitted` lines.
  - 200 OK on success.

- `GET /health`
  - JSON: `{ "status": "OK", "timestamp": "<RFC3339>" }`
  - 200 OK.

- `GET /ready`
  - JSON: `{ "status": "READY", "timestamp": "<RFC3339>" }`
  - 200 OK.

- `GET /version`
  - Returns package version string.
  - 200 OK.

- `GET /status`
  - JSON: `{ "status": "ok" }`
  - 200 OK.

- `GET /time`
  - JSON: `{ "utc": "<RFC3339>" }`
  - 200 OK.

- `GET /metrics`
  - HTML page showing:
    - CPU usage with a color gauge.
    - Memory and swap with GiB/MiB and gauges for used %.
    - File system overview (each mount) with total size, free space, and a gauge for usage %.
    - Process count (non-sensitive summary only).
  - 200 OK.

- `GET /echo/{msg}`
  - Returns `{msg}` as plain text.
  - 200 OK.

- `GET /help`
  - Returns a short help string of available endpoints.
  - 200 OK.

- `GET /ping`
  - Returns `pong`.
  - 200 OK.

- `POST /submit?token=<auth_token>`
  - Accepts `application/json` with the payload above.
  - Behavior:
    - If `token` mismatches: 401 Unauthorized with JSON `{ "error": "Unauthorized" }`.
    - If JSON parse fails: 400 Bad Request with JSON `{ "error": "JSON parse error: ...", "example": { ... } }` (example payload included).
    - On success:
      - Appends a line to `/root/fireplan_alarm_divera_received`: `<timestamp>\t<title>`.
      - Sends the event into the main loop for parsing and submission.
      - Returns 200 OK with JSON `{ "status": "submitted" }`.

### Parsing and conversion logic
- The incoming text (`data.text`) is normalized (`\r` removed) and then scanned line by line applying configured regular expressions:
  - `regex_ort`, `regex_ortsteil`, `regex_objektname` attempt to capture named groups and assign: `result.ort`, `result.ortsteil`, `result.objektname`.
- RIC detection:
  - Operates only on the substring of `data.text` after the marker `Einsatzmittel:`.
  - Splits by commas, and for each token detects configured `rics` by substring matching.
  - Substring de-duplication: If a longer RIC name contains a smaller one, only the longer survives within that token.
- Street/house number:
  - `result.strasse` = first word of the address’s left part (before first comma).
  - `result.hausnummer` = the second token (if any) in the left part.
- Einsatznummer & Stichwort:
  - `result.einsatznrlst` = `data.foreign_id`.
  - `result.einsatzstichwort` = `data.title`.
- Zusatzinfo extraction:
  - If `data.text` contains a segment between `Meldung:` and `Schlagwort`, it’s trimmed and stored in `result.zusatzinfo`. Otherwise empty.
- Coordinates:
  - `lat`/`lng` kept as strings.
  - `result.koordinaten` produced as `"lat,lng"` (Google Maps-friendly).
- Warnings are logged if essential fields remain empty after parsing.

### Dummy RICs and KdoW
- The configuration includes special RICs to ensure Fireplan opens a Report and enters alarm mode for every affected “Einsatzabteilung” of FF Ubstadt-Weiher:
  - Dummy RICs: One member of each “Einsatzabteilung” is assigned a dummy RIC to guarantee alarms for “Zug” or “Gesamt” propagate to every department.
  - KdoW RIC: Always present in “Verwaltung” Abteilung, ensuring the KdoW is included and Fireplan opens the report consistently.
- Operational effect:
  - Even if a specific department wouldn’t be reached by a minimal set, the dummy/KdoW RICs force inclusion, making sure Fireplan treats the alarm as affecting all necessary units.

### Submission to Fireplan
- A temporary API-Token is obtained by calling `https://data.fireplan.de/api/Register/<Standort>` with header `API-Key: <fireplan_api_key>`.
- For each detected RIC (post de-duplication and filtering), a `FireplanAlarm` is sent via POST to `https://data.fireplan.de/api/Alarmierung`:
  - Headers: `API-Token: <token>` and `accept: */*`.
  - JSON includes: RIC, subRIC, einsatznrlst, strasse, hausnummer, ort, ortsteil, objektname, koordinaten, einsatzstichwort, zusatzinfo.
- On success:
  - A line is appended to `/root/fireplan_alarm_divera_submitted`: `<timestamp>\t<einsatznrlst> - <einsatzstichwort>`.
  - The optional `simple_trigger` script (if configured) is executed.
- On failure:
  - Error logs capture HTTP status and any returned body.

### Files written
- `/root/fireplan_alarm_divera_received`: Appends `<timestamp>\t<title>` for each accepted submission.
- `/root/fireplan_alarm_divera_submitted`: Appends `<timestamp>\t<einsatznrlst> - <einsatzstichwort>` for each successful POST to Fireplan.
- Log output goes to stdout/stderr (journald under systemd).

### Signals and shutdown
- SIGINT/SIGTERM/SIGHUP/SIGQUIT send an internal Shutdown event; the main loop exits gracefully.

### Metrics and UI notes
- Metrics show CPU, memory, swap, filesystem sizes/free space (GiB/MiB where appropriate) with visual gauges.
- The root page includes two scrollable log boxes so the page remains compact; newest entries are displayed first.

### Operational considerations
- HTTPS certs must exist at `/etc/letsencrypt/live/<hostname>/` (fullchain.pem, privkey.pem).
- The process must have permissions to append to `/root/fireplan_alarm_divera_received` and `/root/fireplan_alarm_divera_submitted` (running as root is typical for writing in `/root/`).
- Configure `auth_token` and pass it as `token` query parameter for `/submit` requests.

---

## Dokumentation (Deutsch)

### Überblick
- Zweck: Entgegennahme von Alarmen über einen sicheren HTTPS-Endpunkt, Validierung/Authentifizierung, Parsen von strukturiertem Text und Weiterleitung an Fireplan.
- Zielgruppe: Betreiber der FF Ubstadt-Weiher, die eine robuste Brücke zwischen DIVERA und Fireplan benötigen – inklusive Unterstützung für Dummy-RICs und die stets vorhandene KdoW-RIC, um Alarmabdeckung über alle Abteilungen sicherzustellen.
- Transport: Actix-Web-Server über HTTPS (rustls 0.23) mit Let’s-Encrypt-Zertifikaten unter `/etc/letsencrypt/live/<hostname>/`.
- Konfiguration: Aus `fireplan_alarm_divera.conf` im Home-Verzeichnis geladen. Enthält:
  - `http_host` und `http_port` zum Binden.
  - `auth_token`: Vorab geteilter Token, der via `token` Query-Parameter in `/submit` geprüft wird.
  - `fireplan_api_key`: API-Schlüssel, um ein temporäres Fireplan-API-Token zu erhalten.
  - Regexe: `regex_ort`, `regex_ortsteil`, `regex_objektname` zum Extrahieren von Feldern aus dem Textkörper.
  - `rics`: Liste der konfigurierten Ziel-RICs, inkl. Dummy/KdoW-Verhalten.
  - Optional `simple_trigger`: Skriptpfad, der bei erfolgreicher Übermittlung ausgeführt wird.

### Sicherheit
- HTTPS-verschlüsselte Endpunkte (kein Klartext-HTTP).
- `/submit` erfordert den Query-Parameter `token`, der mit `auth_token` übereinstimmen muss. Bei Abweichung: `401 Unauthorized`.
- Keine sensiblen Versionsinformationen in Metriken oder UI exponiert.

### Laufzeitverhalten
- Der Webserver startet vor der Empfangsschleife.
- Signale (SIGINT/SIGTERM/SIGHUP/SIGQUIT) werden abgefangen; ein Shutdown-Event wird gesendet und die Hauptschleife beendet sich sauber.
- Logging: Einheitlicher Logger auf stdout/stderr (geeignet für systemd). Actix-Request-Logs werden über dasselbe Backend ausgegeben.

### Datenformate
- JSON-POST nach `/submit?token=<auth>` mit `Content-Type: application/json`.
- Felder der Nutzlast (lat/lng als Strings, um Fließkomma-Ungenauigkeiten zu vermeiden):
  ```json
  {
    "id": 247,
    "foreign_id": "<externe-nachrichten-id>",
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
  }
  ```

### Endpunkt-Referenz
Alle Endpunkte werden über HTTPS auf dem konfigurierten Host/Port bereitgestellt.

- `GET /`
  - Liefert eine Status-HTML-Seite mit:
    - Willkommensnachricht.
    - Kleiner Status mit Zeitstempel.
    - Link auf `/metrics` über das Wort „server“.
    - Zwei scrollbare Log-Felder (neueste Einträge oben):
      - „Received“ zeigt Zeilen aus `/root/fireplan_alarm_divera_received`.
      - „Submitted“ zeigt Zeilen aus `/root/fireplan_alarm_divera_submitted`.
  - 200 OK.

- `GET /health`
  - JSON: `{ "status": "OK", "timestamp": "<RFC3339>" }`
  - 200 OK.

- `GET /ready`
  - JSON: `{ "status": "READY", "timestamp": "<RFC3339>" }`
  - 200 OK.

- `GET /version`
  - Gibt die Paketversion als String zurück.
  - 200 OK.

- `GET /status`
  - JSON: `{ "status": "ok" }`
  - 200 OK.

- `GET /time`
  - JSON: `{ "utc": "<RFC3339>" }`
  - 200 OK.

- `GET /metrics`
  - HTML-Seite mit:
    - CPU-Auslastung und Farb-Gauge.
    - Speicher und Swap in GiB/MiB sowie Gauges für genutzte %.
    - Dateisystem-Übersicht (pro Mount) mit Gesamtgröße, freiem Speicher und Gauge für Nutzungs%.
    - Prozessanzahl (nur nicht-sensitive Zusammenfassung).
  - 200 OK.

- `GET /echo/{msg}`
  - Gibt `{msg}` als Text zurück.
  - 200 OK.

- `GET /help`
  - Kurze Hilfe mit verfügbaren Endpunkten.
  - 200 OK.

- `GET /ping`
  - Gibt `pong` zurück.
  - 200 OK.

- `POST /submit?token=<auth_token>`
  - Erwartet `application/json` und das oben beschriebenen Schema.
  - Verhalten:
    - Token abweichend: 401 Unauthorized mit JSON `{ "error": "Unauthorized" }`.
    - JSON-Parsing-Fehler: 400 Bad Request mit JSON `{ "error": "JSON parse error: ...", "example": { ... } }`.
    - Bei Erfolg:
      - Eine Zeile wird in `/root/fireplan_alarm_divera_received` angehängt: `<timestamp>\t<title>`.
      - Event geht in die Hauptschleife zum Parsen und Weiterleiten.
      - 200 OK mit JSON `{ "status": "submitted" }`.

### Parsen und Konvertierung
- Eingehender Text (`data.text`) wird normalisiert (Entfernen von `\r`) und zeilenweise mit konfigurierten Regexen gescannt:
  - `regex_ort`, `regex_ortsteil`, `regex_objektname` setzen entsprechend `result.ort`, `result.ortsteil`, `result.objektname`.
- RIC-Erkennung:
  - Nur im Substring von `data.text` nach dem Marker `Einsatzmittel:`.
  - Split per Komma; in jedem Token werden konfigurierte `rics` per Substring-Match erkannt.
  - Substring-Entdoppelung: Wenn ein längerer RIC einen kürzeren als Substring enthält, bleibt nur der längere für dieses Token bestehen.
- Straße/Hausnummer:
  - `result.strasse` = erstes Wort der linken Adress-Seite (vor erstem Komma).
  - `result.hausnummer` = zweiter Token (falls vorhanden) in dieser linken Seite.
- Einsatznummer & Stichwort:
  - `result.einsatznrlst` = `data.foreign_id`.
  - `result.einsatzstichwort` = `data.title`.
- Zusatzinfo:
  - Falls ein Textsegment zwischen `Meldung:` und `Schlagwort` vorhanden ist, wird dieses getrimmt und als `result.zusatzinfo` abgelegt; sonst leer.
- Koordinaten:
  - `lat`/`lng` bleiben Strings.
  - `result.koordinaten` wird als `"lat,lng"` erzeugt (Google-Maps-kompatibel).
- Warnungen werden geloggt, wenn wesentliche Felder nach dem Parsen leer bleiben.

### Dummy-RICs und KdoW
- In der Konfiguration sind besondere RICs enthalten, um sicherzustellen, dass Fireplan für jede betroffene Einsatzabteilung der FF Ubstadt-Weiher einen Report öffnet und in den Alarmmodus wechselt:
  - Dummy-RICs: Ein Mitglied jeder Einsatzabteilung ist mit einer Dummy-RIC versehen, damit Alarme für „Zug“ oder „Gesamt“ in jeder Abteilung eintreffen.
  - KdoW-RIC: In der Abteilung „Verwaltung“ stets vorhanden, um den KdoW einzubeziehen und ein durchgängiges Öffnen des Reports in Fireplan zu gewährleisten.
- Operativer Effekt:
  - Selbst wenn eine Abteilung durch eine minimale Zuteilung nicht erreicht würde, erzwingen Dummy-/KdoW-RICs die Einbeziehung, sodass Fireplan den Alarm als alle notwendigen Einheiten betreffend behandelt.

### Übermittlung an Fireplan
- Ein temporäres API-Token wird über `https://data.fireplan.de/api/Register/<Standort>` mit Header `API-Key: <fireplan_api_key>` bezogen.
- Für jede erkannte RIC (nach Entdoppelung und Filterung) wird ein `FireplanAlarm` per POST nach `https://data.fireplan.de/api/Alarmierung` gesendet:
  - Header: `API-Token: <token>` und `accept: */*`.
  - JSON enthält: RIC, subRIC, einsatznrlst, strasse, hausnummer, ort, ortsteil, objektname, koordinaten, einsatzstichwort, zusatzinfo.
- Bei Erfolg:
  - Eine Zeile wird in `/root/fireplan_alarm_divera_submitted` angehängt: `<timestamp>\t<einsatznrlst> - <einsatzstichwort>`.
  - Optionales `simple_trigger`-Skript wird ausgeführt (falls konfiguriert).
- Bei Fehler:
  - Fehlerlogs enthalten HTTP-Status und ggf. Antworttext.

### Geschriebene Dateien
- `/root/fireplan_alarm_divera_received`: Hängt `<timestamp>\t<title>` für jeden akzeptierten Eingang an.
- `/root/fireplan_alarm_divera_submitted`: Hängt `<timestamp>\t<einsatznrlst> - <einsatzstichwort>` für jede erfolgreiche Übermittlung an Fireplan an.
- Logs gehen nach stdout/stderr (journald unter systemd).

### Signale und Beenden
- SIGINT/SIGTERM/SIGHUP/SIGQUIT erzeugen intern ein Shutdown-Event; die Hauptschleife beendet sich geordnet.

### Metriken und UI
- Metriken zeigen CPU, Speicher, Swap, Dateisystemgrößen/freien Speicher (GiB/MiB) mit visuellen Gauges.
- Die Startseite beinhaltet zwei scrollbare Log-Felder, sodass die Seite kompakt bleibt; neueste Einträge zuerst.

### Betriebshinweise
- HTTPS-Zertifikate müssen unter `/etc/letsencrypt/live/<hostname>/` vorhanden sein (fullchain.pem, privkey.pem).
- Der Prozess benötigt Schreibrechte auf `/root/fireplan_alarm_divera_received` und `/root/fireplan_alarm_divera_submitted` (typischerweise als root).
- `auth_token` konfigurieren und als `token` Query-Parameter bei `/submit` übergeben.
