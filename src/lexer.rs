use crate::error::{StrqlError, StrqlResult};
use logos::Logos;
use miette::NamedSource;
use std::fmt::{Display, Formatter};

#[derive(Logos, Debug, PartialEq, Clone)]
#[logos(skip r"[ \t]+")] // whitespace
#[logos(skip r"//[^\n]*")] // line comments
#[logos(skip r"/\*([^*]|\*[^/])*\*/")] // block comments
pub enum Token {
    // Keywords
    #[token("TEXT", ignore(case))]
    Text,
    #[token("ROOT", ignore(case))]
    Root,
    #[token("OR", ignore(case))]
    Or,
    #[token("ADD", ignore(case))]
    Add,
    #[token("TO", ignore(case))]
    To,
    #[token("SPLITBY", ignore(case))]
    SplitBy,
    #[token("ANYCASE", ignore(case))]
    AnyCase,
    #[token("UPPER", ignore(case))]
    Upper,
    #[token("LOWER", ignore(case))]
    Lower,
    #[token("LAZY", ignore(case))]
    Lazy,
    #[token("GREEDY", ignore(case))]
    Greedy,

    // Built-in patterns
    #[token("WORD", ignore(case))]
    Word,
    #[token("LINE", ignore(case))]
    Line,
    #[token("NEWLINE", ignore(case))]
    Newline,
    #[token("SPACE", ignore(case))]
    Space,
    #[token("ANYCHAR", ignore(case))]
    AnyChar,
    #[token("ANY", ignore(case))]
    Any,
    #[token("DIGIT", ignore(case))]
    Digit,
    #[token("LETTER", ignore(case))]
    Letter,
    #[token("ALPHANUM", ignore(case))]
    Alphanum,

    // Operators and punctuation
    #[token("=")]
    Equals,
    #[token("->")]
    Arrow,
    #[token("..")]
    DotDot,
    #[token(".")]
    Dot,
    #[token("(")]
    LParen,
    #[token(")")]
    RParen,
    #[token("[")]
    LBracket,
    #[token("]")]
    RBracket,
    #[token("{")]
    LBrace,
    #[token("}")]
    RBrace,
    #[token(":")]
    Colon,
    #[token("\n")]
    NewlineChar,
    #[token("\r\n")]
    CrLf,

    #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", priority = 1, callback = |lex| lex.slice().to_string())]
    Identifier(String),
    #[regex(r"[0-9]+", |lex| lex.slice().parse::<usize>().ok())]
    Number(usize),
    #[regex(r#""([^"\\]|\\.)*""#, parse_string_literal)]
    StringLiteral(String),
}

impl Display for Token {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

/// nodes escape sequences.
fn parse_string_literal(lex: &mut logos::Lexer<Token>) -> Option<String> {
    let unquoted_slice = &lex.slice()[1..lex.slice().len() - 1];

    let mut result = String::new();
    let mut chars = unquoted_slice.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '\\' {
            result.push(c)
        } else {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some(other) => result.push(other),
                None => return None,
            }
        }
    }

    Some(result)
}

pub struct SpannedToken {
    pub token: Token,
    pub span: std::ops::Range<usize>,
}

impl Token {
    pub fn vec_from(source: &str) -> StrqlResult<Vec<SpannedToken>> {
        let lexer = Token::lexer(source);
        let mut result = vec![];
        for (tok, span) in lexer.spanned() {
            tok.map_err(|_| StrqlError::LexerError {
                _src: NamedSource::new("strql", source.to_string()),
                _span: span.clone().into(),
            })
            .map(|token| result.push(SpannedToken { token, span }))?;
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tokens() {
        let source = "TEXT = myvar SPLITBY NEWLINE";
        let tokens = Token::vec_from(source).unwrap();

        assert_eq!(tokens.len(), 5);
        assert_eq!(tokens[0].token, Token::Text);
        assert_eq!(tokens[1].token, Token::Equals);
        assert_eq!(tokens[2].token, Token::Identifier("myvar".to_string()));
        assert_eq!(tokens[3].token, Token::SplitBy);
        assert_eq!(tokens[4].token, Token::Newline);
    }

    #[test]
    fn test_string_literal() {
        let source = r#""hello world""#;
        let tokens = Token::vec_from(source).unwrap();

        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0].token,
            Token::StringLiteral("hello world".to_string())
        );
    }

    #[test]
    fn test_escape_sequences() {
        let source = r#""hello\nworld""#;
        let tokens = Token::vec_from(source).unwrap();

        assert_eq!(tokens.len(), 1);
        assert_eq!(
            tokens[0].token,
            Token::StringLiteral("hello\nworld".to_string())
        );
    }

    #[test]
    fn test_quantifier() {
        let source = "0..N WORD";
        let tokens = Token::vec_from(source).unwrap();

        assert_eq!(tokens.len(), 4);
        assert_eq!(tokens[0].token, Token::Number(0));
        assert_eq!(tokens[1].token, Token::DotDot);
        assert_eq!(tokens[2].token, Token::Identifier("N".to_string()));
        assert_eq!(tokens[3].token, Token::Word);
    }

    #[test]
    fn test_add_statement() {
        let source = "-> ADD item{} TO ROOT.items[]";
        let tokens = Token::vec_from(source).unwrap();

        // -> ADD item {} TO ROOT . items [ ]
        // 1   2   3    4  5  6   7  8    9  10 11
        assert_eq!(tokens.len(), 11);
        assert_eq!(tokens[0].token, Token::Arrow);
        assert_eq!(tokens[1].token, Token::Add);
        assert_eq!(tokens[2].token, Token::Identifier("item".to_string()));
        assert_eq!(tokens[3].token, Token::LBrace);
        assert_eq!(tokens[4].token, Token::RBrace);
        assert_eq!(tokens[5].token, Token::To);
        assert_eq!(tokens[6].token, Token::Root);
        assert_eq!(tokens[7].token, Token::Dot);
        assert_eq!(tokens[8].token, Token::Identifier("items".to_string()));
        assert_eq!(tokens[9].token, Token::LBracket);
        assert_eq!(tokens[10].token, Token::RBracket);
    }

    #[test]
    fn test_case_insensitive_keywords() {
        let source = "text = WORD splitby newline";
        let tokens = Token::vec_from(source).unwrap();

        assert_eq!(tokens[0].token, Token::Text);
        assert_eq!(tokens[2].token, Token::Word);
        assert_eq!(tokens[3].token, Token::SplitBy);
        assert_eq!(tokens[4].token, Token::Newline);
    }
}
