#![allow(dead_code)]

use std::{iter::Peekable, str::Chars};

use crate::data_type::DataType;

pub struct Parser<'a> {
    val: &'a str,
    tokenizer: Tokenizer<'a>,
}

impl<'a> Parser<'a> {
    pub fn new(val: &'a str) -> Self {
        Self {
            val,
            tokenizer: Tokenizer::new(val),
        }
    }

    pub fn parse(self: &mut Self) -> crate::Result<DataType> {
        let next_data_type = self.parse_next_type()?;
        if self.tokenizer.next().is_some() {
            return Err(crate::BustubError::ParseError(
                format!("checking trailing content after parsing {next_data_type}").into(),
            ));
        }
        Ok(next_data_type)
    }

    pub fn parse_next_type(&mut self) -> crate::Result<DataType> {
        match self.next_token()? {
            Token::SimpleType(data_type) => Ok(data_type),
            Token::LParen => todo!(),
            Token::RParen => todo!(),
            Token::Comma => todo!(),
        }
    }

    fn next_token(&mut self) -> crate::Result<Token> {
        match self.tokenizer.next() {
            Some(token) => token,
            None => Err(crate::BustubError::ParseError(
                "fail to find next toke".into(),
            )),
        }
    }
}

impl<'a> Iterator for Tokenizer<'a> {
    type Item = crate::Result<Token>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match self.chars.peek()? {
                ' ' => {
                    self.chars.next();
                    continue;
                }
                '(' => {
                    self.chars.next();
                    return Some(Ok(Token::LParen));
                }
                ')' => {
                    self.chars.next();
                    return Some(Ok(Token::RParen));
                }
                _ => return Some(self.parse_word()),
            }
        }
    }
}
enum Token {
    SimpleType(DataType),
    LParen,
    RParen,
    Comma,
}

struct Tokenizer<'a> {
    val: &'a str,
    chars: Peekable<Chars<'a>>,
    //temporary buffer for parsing words
    word: String,
}

fn is_separator(c: char) -> bool {
    c == '(' || c == ')' || c == ' ' || c == ','
}

impl<'a> Tokenizer<'a> {
    fn new(val: &'a str) -> Self {
        Self {
            val,
            chars: val.chars().peekable(),
            word: String::new(),
        }
    }

    fn peek_next_char(&mut self) -> Option<char> {
        self.chars.peek().copied()
    }

    fn parse_word(&mut self) -> crate::Result<Token> {
        self.word.clear();
        loop {
            match self.peek_next_char() {
                None => break,
                Some(c) if is_separator(c) => break,
                Some(c) => {
                    self.chars.next();
                    self.word.push(c);
                }
            }
        }
        match self.word.as_str() {
            "Null" => Ok(Token::SimpleType(DataType::Null)),
            "Int8" => Ok(Token::SimpleType(DataType::Int8)),
            "Int16" => Ok(Token::SimpleType(DataType::Int16)),
            "Int32" => Ok(Token::SimpleType(DataType::Int32)),
            "Int64" => Ok(Token::SimpleType(DataType::Int64)),
            "UInt8" => Ok(Token::SimpleType(DataType::UInt8)),
            "UInt16" => Ok(Token::SimpleType(DataType::UInt16)),
            "UInt32" => Ok(Token::SimpleType(DataType::UInt32)),
            "UInt64" => Ok(Token::SimpleType(DataType::UInt64)),
            "Boolean" => Ok(Token::SimpleType(DataType::Boolean)),
            "Binary" => Ok(Token::SimpleType(DataType::Binary)),

            _ => Err(crate::BustubError::ParseError("Token Parsing Error".into())),
        }
    }
}
