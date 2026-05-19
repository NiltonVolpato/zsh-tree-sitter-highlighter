use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
struct Request {
    version: String,
    mode: String,
    cwd: String,
    prebuffer: String,
    buffer: String,
}

#[derive(Serialize, Debug)]
struct Response {
    // Highlight regions. Newline separated.
    regions: Option<String>,
}
