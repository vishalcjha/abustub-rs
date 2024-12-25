use std::borrow::Cow;

use thiserror::Error;

#[allow(dead_code)]
#[derive(Error, Debug)]
pub enum BustubError {
    #[error("I/O Error : {0}")]
    IoError(#[from] std::io::Error),
    #[error("{0}")]
    PlainError(Cow<'static, str>),
    #[error("{0}")]
    ParseError(Cow<'static, str>),
}

#[cfg(test)]
mod test {
    use super::BustubError;

    #[test]
    fn test_error_debug() {
        let error = BustubError::PlainError("This just happened".into());
        println!("{:?}", error);
        println!("{}", error);

        let io_error = BustubError::IoError(std::io::Error::new(
            std::io::ErrorKind::Deadlock,
            "Deadlock happened",
        ));

        println!("{:?}", io_error);
        println!("{}", io_error);
    }
}
