use std::error::Error;
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Input = 3,
    Safety = 4,
    Strict = 5,
    Runtime = 6,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

#[derive(Debug)]
pub struct AppError {
    code: ExitCode,
    message: String,
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn input(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::Input,
            message: message.into(),
        }
    }

    pub fn safety(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::Safety,
            message: message.into(),
        }
    }

    pub fn strict(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::Strict,
            message: message.into(),
        }
    }

    pub fn runtime(message: impl Into<String>) -> Self {
        Self {
            code: ExitCode::Runtime,
            message: message.into(),
        }
    }

    pub fn runtime_err<E: Display>(err: E) -> Self {
        Self::runtime(err.to_string())
    }

    pub fn input_err<E: Display>(err: E) -> Self {
        Self::input(err.to_string())
    }

    pub fn exit_code(&self) -> i32 {
        self.code.as_i32()
    }
}

impl Display for AppError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl Error for AppError {}
