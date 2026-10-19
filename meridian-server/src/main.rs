//! Serves the Meridian HTTP API and dashboard. Configure with MERIDIAN_DB (database path,
//! default `meridian.db`) and MERIDIAN_ADDR (listen address, default `127.0.0.1:7979`).

use std::process::ExitCode;

fn main() -> ExitCode {
    let db = std::env::var("MERIDIAN_DB").unwrap_or_else(|_| "meridian.db".to_string());
    let addr = std::env::var("MERIDIAN_ADDR").unwrap_or_else(|_| "127.0.0.1:7979".to_string());

    let conn = match engine_api::open_db(&db) {
        Ok(conn) => conn,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };
    let server = match tiny_http::Server::http(&addr) {
        Ok(server) => server,
        Err(e) => {
            eprintln!("error: cannot listen on {addr}: {e}");
            return ExitCode::FAILURE;
        }
    };
    eprintln!("meridian-server listening on http://{addr} (database: {db})");

    // Requests are handled one at a time with one database connection, which is plenty for a
    // local dashboard.
    for request in server.incoming_requests() {
        let response = meridian_server::handle(
            &conn,
            &meridian_server::Request {
                method: request.method().as_str(),
                url: request.url(),
                host: header(&request, "Host"),
            },
        );
        // Only fails for header bytes that aren't valid, and both parts here are constants.
        let content_type =
            tiny_http::Header::from_bytes("Content-Type", response.content_type).unwrap();
        let reply = tiny_http::Response::from_string(response.body)
            .with_status_code(response.status)
            .with_header(content_type);
        if let Err(e) = request.respond(reply) {
            eprintln!("warning: could not send response: {e}");
        }
    }
    ExitCode::SUCCESS
}

fn header<'r>(request: &'r tiny_http::Request, name: &'static str) -> Option<&'r str> {
    request
        .headers()
        .iter()
        .find(|header| header.field.equiv(name))
        .map(|header| header.value.as_str())
}
