use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Debug)]
pub struct Request {
    pub version: String,
    pub mode: String,
    pub cwd: String,
    pub prebuffer: String,
    pub buffer: String,
}

#[derive(Serialize, Debug)]
pub struct Response {
    /// Highlight regions. Newline separated.
    pub regions: String,
}
