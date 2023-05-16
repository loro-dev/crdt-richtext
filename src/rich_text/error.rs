#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("Decode error")]
    DecodeError,
    #[error("Invalid expand")]
    InvalidExpand,
}
