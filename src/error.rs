use alloc::boxed::Box;
use alloc::string::String;

#[repr(transparent)]
pub(crate) struct StringError(Box<str>);

impl core::fmt::Display for StringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.0, f)
    }
}

impl core::fmt::Debug for StringError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(&self.0, f)
    }
}

impl core::error::Error for StringError {}

#[allow(missing_docs)]
#[derive(Debug)]
pub struct Error {
    inner: Box<dyn core::error::Error + Send>,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Display::fmt(&self.inner, f)
    }
}

impl core::error::Error for Error {}

impl From<Box<dyn core::error::Error + Send>> for Error {
    fn from(value: Box<dyn core::error::Error + Send>) -> Self {
        Self { inner: value }
    }
}

impl From<Box<StringError>> for Error {
    fn from(value: Box<StringError>) -> Self {
        Self { inner: value }
    }
}

impl From<Error> for Box<dyn core::error::Error + Send> {
    fn from(value: Error) -> Self {
        value.inner
    }
}

#[allow(missing_docs)]
pub type Result<T> = core::result::Result<T, Error>;

pub(crate) fn display_error(err: impl Into<String>) -> Error {
    let string: String = err.into();
    Error {
        inner: Box::new(StringError(string.into_boxed_str())),
    }
}
