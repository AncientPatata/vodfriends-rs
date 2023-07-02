use std::fmt;

#[derive(Debug)]
pub struct TwitchClientError {
    details: String,
}

impl TwitchClientError {
    pub fn new(msg: &str) -> TwitchClientError {
        TwitchClientError {
            details: msg.to_string(),
        }
    }
}

impl fmt::Display for TwitchClientError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.details)
    }
}
