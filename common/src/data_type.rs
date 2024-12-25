#![allow(dead_code)]

use core::fmt;
use std::fmt::{Display, Formatter};

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone)]
pub enum DataType {
    Null,
    Boolean,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Binary,
}

impl Display for DataType {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl DataType {
    pub fn is_number(&self) -> bool {
        use DataType::*;
        matches!(
            self,
            UInt8 | UInt16 | UInt32 | UInt64 | Int8 | Int16 | Int32 | Int64
        )
    }

    pub fn is_null(&self) -> bool {
        use DataType::Null;

        matches!(self, Null)
    }
}
