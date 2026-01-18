#![allow(non_snake_case)]

use miette::{Diagnostic, NamedSource, SourceSpan};
use thiserror::Error;

#[derive(Error, Diagnostic, Debug)]
pub enum StrqlError {
    #[error("Unexpected character")]
    #[diagnostic(code(lexer::unexpected_char), help("Remove or escape this character"))]
    LexerError {
        #[source_code]
        _src: NamedSource<String>,
        #[label("unexpected character here")]
        _span: SourceSpan,
    },
    #[error("Unexpected token `{_found}`)")]
    #[diagnostic(code(parser::unexpected_token), help("Was expecting: `{_expected}`"))]
    UnexpectedToken {
        _expected: String,
        _found: String,
        #[source_code]
        _src: NamedSource<String>,
        #[label("here")]
        _span: SourceSpan,
    },

    #[error("Unbound variable '{_name}'")]
    #[diagnostic(
        code(solver::unbound_variable),
        help("Create the variable first with: [...] -> ADD {_name}{{}} TO [...]")
    )]
    UnboundVariable {
        _name: String,
        #[source_code]
        _src: NamedSource<String>,
        #[label("node '{_name}' not yet created")]
        _span: SourceSpan,
    },

    #[error("VariableIsNotObject variable '{_name}'")]
    #[diagnostic(
        code(solver::variable_is_not_object),
        help("VariableIsNotObject the variable first with: [...] -> ADD {_name}{{}} TO [...]")
    )]
    VariableTypeMismatch {
        _name: String,
        _expected: String,
        #[source_code]
        _src: NamedSource<String>,
        #[label("node '{_name}' not yet created")]
        _span: SourceSpan,
    },
    #[error("Internal error: {_message}")]
    #[diagnostic(code(internal), help("Please open a github issue about this!"))]
    Internal { _message: &'static str },

    // ========== Solver Errors (input-level) ==========
    #[error("Input does not match the pattern")]
    #[diagnostic(code(solver::no_match), help("The statements do not match the input"))]
    PatternNoMatch {
        #[source_code]
        _src: NamedSource<String>,
    },

    #[error("Input text ambiguously matches the pattern")]
    #[diagnostic(
        code(solver::ambiguous),
        help("Add LAZY or GREEDY disambiguators to refine your statement set")
    )]
    AmbiguousParse {
        #[source_code]
        _src: NamedSource<String>,
    },

    #[error("Expected literal \"{_expected}\"")]
    #[diagnostic(code(solver::literal_mismatch), help("Found \"{_found}\" instead"))]
    LiteralMismatch {
        _expected: String,
        _found: String,
        #[source_code]
        _src: NamedSource<String>,
        #[label("mismatch here")]
        _span: SourceSpan,
    },

    #[error("Expected {_expected}")]
    #[diagnostic(
        code(solver::builtin_mismatch),
        help("Found '{_found}' which is not a valid {_expected}")
    )]
    BuiltinMismatch {
        _expected: &'static str,
        _found: String,
        #[source_code]
        _src: NamedSource<String>,
        #[label("here")]
        _span: SourceSpan,
    },

    #[error("Unexpected end of input")]
    #[diagnostic(
        code(solver::unexpected_eof),
        help("Expected {_expected} but input ended")
    )]
    UnexpectedEndOfInput {
        _expected: &'static str,
        #[source_code]
        _src: NamedSource<String>,
        #[label("input ends here")]
        _span: SourceSpan,
    },

    #[error("No alternative matched")]
    #[diagnostic(
        code(solver::no_alternative),
        help("None of the OR alternatives matched the input at this position")
    )]
    NoAlternativeMatched {
        #[source_code]
        _src: NamedSource<String>,
        #[label("no alternative matches here")]
        _span: SourceSpan,
    },

    #[error("Pattern matched only {_matched} of {_total} bytes")]
    #[diagnostic(
        code(solver::partial_match),
        help("Extend your statement set to match the missing portion of the text")
    )]
    PartialMatch {
        _matched: usize,
        _total: usize,
        #[source_code]
        _src: NamedSource<String>,
        #[label("unmatched portion starts here")]
        _span: SourceSpan,
    },

    #[error("Quantifier requires at least {_min} repetitions, found {_found}")]
    #[diagnostic(
        code(solver::quantifier_min),
        help("The pattern needs to repeat at least {_min} times")
    )]
    QuantifierMinNotMet {
        _min: usize,
        _found: usize,
        #[source_code]
        _src: NamedSource<String>,
        #[label("quantifier failed here")]
        _span: SourceSpan,
    },

    #[error("Constraint not satisfied")]
    #[diagnostic(code(solver::constraint_failed))]
    ConstraintFailed {
        #[source_code]
        _src: NamedSource<String>,
    },

    #[error("Variable '{_name}' is not numeric")]
    #[diagnostic(
        code(solver::not_numeric),
        help("Value \"{_value}\" cannot be used in numeric comparison")
    )]
    VariableNotNumeric {
        _name: String,
        _value: String,
        #[source_code]
        _src: NamedSource<String>,
    },
    #[error("No TEXT statement given")]
    #[diagnostic(
        code(solver::no_text_statement),
        help("Add a `TEXT = <expression>` statement to give the query an entry point")
    )]
    NoTextStatement {
        #[source_code]
        _src: NamedSource<String>,
    },
}

pub type StrqlResult<T> = Result<T, StrqlError>;

pub trait NamedSourceExt<'a> {
    fn src(&self) -> &'a str;
    fn source_name(&self) -> &str {
        "strql"
    }

    fn src_to_named(&self) -> NamedSource<String> {
        NamedSource::new(self.source_name(), self.src().to_string())
    }
}

/// Create a NamedSource for input text (used by solver for error reporting)
pub fn input_to_named(input: &str) -> NamedSource<String> {
    NamedSource::new("input", input.to_string())
}
