use std::io::Error as IoError;
use std::error::Error;

pub enum GitError {
    Io(IoError),
    Git(ErrorKind),
}

pub enum ErrorKind {
    // Parsing
    ChecksumMismatch,
    MagicNumberMismatch,
}

impl Error for GitError {
}
