//! HTTP API for Meridian: a thin, read-only layer that maps requests onto `engine_api` calls.
//!
//! `handle` takes a request and returns a response without touching the network, so the whole
//! API can be tested directly. `main.rs` only connects it to a socket.
//!
//! Routes (responses are JSON unless noted). Every route that reads match data accepts the
//! filters `?region=na1&tier=DIAMOND&role=support`.
//!
//! | Path                              | Returns                                                    |
//! | --------------------------------- | ---------------------------------------------------------- |
//! | /api/patches                      | Stored patches with match and balance-change counts        |
//! | /api/patches/{patch}/report       | The patch compared with the one before it                  |
//! | /api/patches/{patch}/report.csv   | The same report as CSV (text/csv)                          |
//! | /api/patches/{patch}/rankings     | Champions by win rate, with pick and ban rates             |
//! | /api/champions/{name}/history     | One champion's results patch by patch                      |
//! | /api/meta/shifts                  | How far the meta moved at each patch                       |
//! | /api/meta/effectiveness           | How often buffs and nerfs moved win rates as intended      |
//!
//! `{patch}` is a game version such as `16.2`. Errors are returned as `{"error": "message"}`.
//!
//! The server has no authentication and is meant for local use or a private network. Requests
//! must name a local `Host` or one of the extra host names it was started with, which stops web
//! pages from reading the data through DNS rebinding.
//!
//! `GET /` serves the dashboard, a single static page (`ui/index.html`) that uses these routes.

use core_types::Filter;
use engine_api::{Connection, EngineError};
use serde::Serialize;
use serde_json::{Value, json};

/// The parts of an HTTP request the API looks at.
pub struct Request<'a> {
    pub method: &'a str,
    /// Path plus optional query string, as sent by the client.
    pub url: &'a str,
    pub host: Option<&'a str>,
}

/// An HTTP response.
pub struct Response {
    pub status: u16,
    pub content_type: &'static str,
    pub body: String,
}

impl Response {
    fn json(status: u16, value: &Value) -> Response {
        Response {
            status,
            content_type: "application/json",
            body: value.to_string(),
        }
    }

    fn error(status: u16, message: String) -> Response {
        Response::json(status, &json!({ "error": message }))
    }
}

/// An error on its way to becoming an HTTP error response.
struct ApiError {
    status: u16,
    message: String,
}

impl ApiError {
    fn bad_request(message: String) -> ApiError {
        ApiError {
            status: 400,
            message,
        }
    }
}

impl From<EngineError> for ApiError {
    fn from(e: EngineError) -> ApiError {
        let status = match e {
            EngineError::PatchNotFound(_)
            | EngineError::NoPreviousPatch(_)
            | EngineError::ChampionNotFound(_) => 404,
            EngineError::Domain(_) => 400,
            EngineError::UnsupportedSchemaVersion(_)
            | EngineError::Corrupt(_)
            | EngineError::Sqlite(_) => 500,
        };
        ApiError {
            status,
            message: e.to_string(),
        }
    }
}

impl From<core_types::DomainError> for ApiError {
    fn from(e: core_types::DomainError) -> ApiError {
        ApiError::bad_request(e.to_string())
    }
}

/// True for `localhost`, `127.0.0.1`, `[::1]` and any name in `extra_hosts`, with or without a
/// port. Names are compared without regard to case, as DNS does.
fn is_allowed_host(host: &str, extra_hosts: &[String]) -> bool {
    let name = match host.rsplit_once(':') {
        Some((name, port)) if !name.is_empty() && port.parse::<u16>().is_ok() => name,
        _ => host,
    };
    matches!(name, "localhost" | "127.0.0.1" | "[::1]")
        || extra_hosts
            .iter()
            .any(|extra| extra.eq_ignore_ascii_case(name))
}

/// The dashboard, compiled into the binary so the server is a single file.
const UI: &str = include_str!("../ui/index.html");

/// Handles one request. `extra_hosts` are host names served besides localhost, such as the
/// server's address on a private network.
pub fn handle(conn: &Connection, extra_hosts: &[String], request: &Request) -> Response {
    if !request
        .host
        .is_some_and(|host| is_allowed_host(host, extra_hosts))
    {
        return Response::error(
            403,
            "meridian-server only accepts requests to localhost or a host listed in MERIDIAN_HOSTS"
                .to_string(),
        );
    }
    if request.method != "GET" {
        return Response::error(405, "this API is read-only; use GET".to_string());
    }
    if request.url == "/" || request.url.starts_with("/?") {
        return Response {
            status: 200,
            content_type: "text/html; charset=utf-8",
            body: UI.to_string(),
        };
    }
    match route(conn, request.url) {
        Ok(Reply::Json(value)) => Response::json(200, &value),
        Ok(Reply::Csv(text)) => Response {
            status: 200,
            content_type: "text/csv; charset=utf-8",
            body: text,
        },
        Err(e) => Response::error(e.status, e.message),
    }
}

enum Reply {
    Json(Value),
    Csv(String),
}

fn route(conn: &Connection, url: &str) -> Result<Reply, ApiError> {
    let (path, query) = url.split_once('?').unwrap_or((url, ""));
    let query = Query::parse(query);
    let segments: Vec<String> = path
        .trim_matches('/')
        .split('/')
        .map(percent_decode)
        .collect();
    let segments: Vec<&str> = segments.iter().map(String::as_str).collect();

    match segments.as_slice() {
        ["api", "patches"] => to_json(engine_api::list_patches(conn)?),
        ["api", "patches", patch, "report"] => to_json(engine_api::patch_report(
            conn,
            patch.parse()?,
            &query.filter()?,
        )?),
        ["api", "patches", patch, "report.csv"] => {
            let report = engine_api::patch_report(conn, patch.parse()?, &query.filter()?)?;
            Ok(Reply::Csv(engine_api::render_csv(&report)))
        }
        ["api", "patches", patch, "rankings"] => to_json(engine_api::rankings(
            conn,
            patch.parse()?,
            &query.filter()?,
        )?),
        ["api", "champions", name, "history"] => {
            to_json(engine_api::champion_history(conn, name, &query.filter()?)?)
        }
        ["api", "meta", "shifts"] => to_json(engine_api::meta_shifts(conn, &query.filter()?)?),
        ["api", "meta", "effectiveness"] => {
            to_json(engine_api::balance_effectiveness(conn, &query.filter()?)?)
        }
        _ => Err(ApiError {
            status: 404,
            message: format!("no route for GET {path}"),
        }),
    }
}

fn to_json(value: impl Serialize) -> Result<Reply, ApiError> {
    serde_json::to_value(value)
        .map(Reply::Json)
        .map_err(|e| ApiError {
            status: 500,
            message: format!("could not encode response: {e}"),
        })
}

/// Decoded query string parameters, in order.
struct Query(Vec<(String, String)>);

impl Query {
    fn parse(query: &str) -> Query {
        Query(
            query
                .split('&')
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                    (percent_decode(key), percent_decode(value))
                })
                .collect(),
        )
    }

    /// The value of a parameter; empty values count as absent, as a form leaves them.
    fn first(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, value)| k == key && !value.is_empty())
            .map(|(_, value)| value.as_str())
    }

    fn filter(&self) -> Result<Filter, ApiError> {
        Ok(Filter {
            region: self.first("region").map(str::to_string),
            tier: self.first("tier").map(str::parse).transpose()?,
            role: self.first("role").map(str::parse).transpose()?,
        })
    }
}

/// Decodes `%XX` escapes and `+` (a space in query strings). Malformed escapes are kept as is.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let escaped = bytes
            .get(i + 1..i + 3)
            .and_then(|hex| std::str::from_utf8(hex).ok())
            .and_then(|hex| u8::from_str_radix(hex, 16).ok());
        match (bytes[i], escaped) {
            (b'%', Some(byte)) => {
                decoded.push(byte);
                i += 3;
            }
            (b'+', _) => {
                decoded.push(b' ');
                i += 1;
            }
            (byte, _) => {
                decoded.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}
