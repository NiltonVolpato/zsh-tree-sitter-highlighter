use serde_derive::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, Debug)]
pub struct Request {
    pub version: String,
    pub mode: String,
    pub cwd: String,
    pub prebuffer: String,
    pub buffer: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Response {
    /// Highlight regions. Newline separated.
    pub regions: String,
}
