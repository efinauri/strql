use crate::ast::*;
use crate::error::{NamedSourceExt, StrqlError, StrqlResult};
use crate::lexer::{SpannedToken, Token};

pub struct Parser<'a> {
    source: &'a str,
    tokens: Vec<SpannedToken>,
    cursor: usize,
    inlined_statements: Vec<Statement>,
}

impl<'a> NamedSourceExt<'a> for Parser<'a> {
    fn src(&self) -> &'a str {
        self.source
    }
}

impl<'a> Parser<'a> {
    pub fn new(source: &'a str) -> StrqlResult<Self> {
        Ok(Self {
            source,
            tokens: Token::vec_from(source)?,
            cursor: 0,
            inlined_statements: Vec::new(),
        })
    }

    pub fn parse(mut self) -> StrqlResult<Program> {
        let mut statements = Vec::new();
        self.skip_newlines();

        while !self.is_at_end() {
            statements.push(self.parse_statement()?);
            self.skip_newlines();
        }

        statements.extend(self.inlined_statements);

        Ok(Program { statements })
    }

    fn parse_statement(&mut self) -> StrqlResult<Statement> {
        let start_cursor = self.cursor;
        let name = self.lvalue()?;
        self.expect(&Token::Equals)?;
        let pattern = self.parse_pattern()?;

        let capture = if self.check(&[&Token::Arrow]) {
            self.advance_cursor_and_get();
            let mut cap = self.parse_capture_clause()?;
            if cap.name.is_empty() {
                cap.name = name.clone();
            }
            Some(cap)
        } else {
            None
        };

        let span = self.span_from(start_cursor);
        Ok(Statement {
            name,
            pattern,
            capture,
            span,
        })
    }

    fn span_from(&self, start_cursor: usize) -> std::ops::Range<usize> {
        let start = self
            .tokens
            .get(start_cursor)
            .map(|t| t.span.start)
            .unwrap_or(0);
        let end = if self.cursor > start_cursor {
            self.tokens
                .get(self.cursor - 1)
                .map(|t| t.span.end)
                .unwrap_or(start)
        } else {
            self.tokens
                .get(self.cursor)
                .map(|t| t.span.start)
                .unwrap_or(self.source.len())
        };
        start..end
    }

    fn make_pattern(&self, start_cursor: usize, node: PatternKind) -> Pattern {
        Pattern {
            node,
            span: self.span_from(start_cursor),
        }
    }

    fn parse_pattern(&mut self) -> StrqlResult<Pattern> {
        self.parse_alternation()
    }

    fn parse_alternation(&mut self) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        let mut left = self.parse_sequence()?;

        while self.check(&[&Token::Or]) {
            self.advance_cursor_and_get();
            let right = self.parse_sequence()?;

            left = match left.node {
                PatternKind::OrChain(mut alts) => {
                    alts.push(right);
                    self.make_pattern(start_cursor, PatternKind::OrChain(alts))
                }
                _ => self.make_pattern(start_cursor, PatternKind::OrChain(vec![left, right])),
            };
        }

        Ok(left)
    }

    fn parse_sequence(&mut self) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        let mut items = Vec::new();

        while !self.is_at_end()
            && !self.check(&[
                &Token::Or,
                &Token::Arrow,
                &Token::RParen,
                &Token::NewlineChar,
                &Token::CrLf,
            ])
        {
            items.push(self.parse_quantified()?);
        }

        match items.len() {
            0 => Err(self.unexpected_token("pattern")),
            1 => Ok(items.remove(0)),
            _ => Ok(self.make_pattern(start_cursor, PatternKind::Sequence(items))),
        }
    }

    fn parse_quantified(&mut self) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        let bias = self.parse_bias();

        if let Ok(min) = self.parse_bound(true) {
            self.expect(&Token::DotDot)?;

            let max = self.parse_bound(false)?;
            let pattern = self.parse_primary(QuantifierBias::Neutral)?;
            return Ok(self.make_pattern(
                start_cursor,
                PatternKind::Repetition {
                    min,
                    max,
                    pattern: Box::new(pattern),
                    bias,
                },
            ));
        }

        self.parse_splitby(bias)
    }

    fn parse_bias(&mut self) -> QuantifierBias {
        if self.check(&[&Token::Lazy]) {
            self.advance_cursor_and_get();
            QuantifierBias::Lazy
        } else if self.check(&[&Token::Greedy]) {
            self.advance_cursor_and_get();
            QuantifierBias::Greedy
        } else {
            QuantifierBias::Neutral
        }
    }

    fn parse_bound(&mut self, testing_for_min_bound: bool) -> StrqlResult<Bound> {
        let start_pos = self.cursor;

        let bound = if testing_for_min_bound {
            match self.get_and_advance_cursor() {
                Some(Token::Number(n)) => Ok(Some(*n)),
                _ => Err(self.unexpected_token("number")),
            }
        } else {
            match self.get_and_advance_cursor() {
                Some(Token::Number(n)) => Ok(Some(*n)),
                Some(Token::N) => Ok(None),
                _ => Err(self.unexpected_token("number or N")),
            }
        };

        if testing_for_min_bound && !self.check(&[&Token::DotDot]) {
            self.cursor = start_pos;
            return Err(StrqlError::Internal {
                _message: "min bound result percolated",
            });
        }

        bound
    }

    /// desugar `<expr> splitby <sep>` into `<expr> 0..n (<sep> <expr>)`
    fn parse_splitby(&mut self, bias: QuantifierBias) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        let pattern = self.parse_modified(bias)?;

        let bias = self.parse_bias();

        if self.check(&[&Token::SplitBy]) {
            self.advance_cursor_and_get();
            let separator = self.parse_primary(QuantifierBias::Neutral)?;

            let tail = self.make_pattern(
                start_cursor,
                PatternKind::Sequence(vec![separator, pattern.clone()]),
            );
            let tail_quantifier = self.make_pattern(
                start_cursor,
                PatternKind::Repetition {
                    min: Some(0),
                    max: None,
                    pattern: Box::new(tail),
                    bias,
                },
            );

            return Ok(self.make_pattern(
                start_cursor,
                PatternKind::Sequence(vec![pattern, tail_quantifier]),
            ));
        }

        if bias != QuantifierBias::Neutral {
            return Err(
                self.unexpected_token("a quantifier (`SPLITBY`, `n..m`, `WORD`, `ANY`, etc.)")
            );
        }

        Ok(pattern)
    }

    fn parse_modified(&mut self, bias: QuantifierBias) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        if self.check(&[&Token::AnyCase]) {
            self.advance_cursor_and_get();
            let inner = self.parse_primary(bias)?;
            return Ok(self.make_pattern(start_cursor, PatternKind::AnyCase(Box::new(inner))));
        }
        if self.check(&[&Token::Upper]) {
            self.advance_cursor_and_get();
            let inner = self.parse_primary(bias)?;
            return Ok(self.make_pattern(start_cursor, PatternKind::Upper(Box::new(inner))));
        }
        if self.check(&[&Token::Lower]) {
            self.advance_cursor_and_get();
            let inner = self.parse_primary(bias)?;
            return Ok(self.make_pattern(start_cursor, PatternKind::Lower(Box::new(inner))));
        }
        self.parse_primary(bias)
    }

    fn parse_primary(&mut self, bias: QuantifierBias) -> StrqlResult<Pattern> {
        let start_cursor = self.cursor;
        let token = self.get_and_advance_cursor().cloned();
        match token {
            Some(Token::StringLiteral(s)) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Literal(s)))
            }
            Some(Token::Identifier(idf)) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Variable(idf)))
            }
            Some(Token::Digit) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Digit)))
            }
            Some(Token::Letter) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Letter)))
            }
            Some(Token::AnyChar) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::AnyChar)))
            }
            Some(Token::Newline) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Newline)))
            }
            Some(Token::Space) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Space)))
            }
            Some(Token::Any) => Ok(self.make_pattern(
                start_cursor,
                PatternKind::Repetition {
                    // desugar into 0..n ANYCHAR
                    min: Some(0),
                    max: None,
                    pattern: Box::new(
                        self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::AnyChar)),
                    ),
                    bias,
                },
            )),
            Some(Token::Word) => Ok(self.make_pattern(
                start_cursor,
                PatternKind::Repetition {
                    // desugar into 0..n LETTER
                    min: Some(0),
                    max: None,
                    pattern: Box::new(
                        self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Letter)),
                    ),
                    bias,
                },
            )),
            Some(Token::Alphanum) => Ok(self.make_pattern(
                start_cursor,
                PatternKind::Repetition {
                    // desugar into 0..n (LETTER OR DIGIT)
                    min: Some(0),
                    max: None,
                    pattern: Box::new(self.make_pattern(
                        start_cursor,
                        PatternKind::OrChain(vec![
                            self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Letter)),
                            self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Digit)),
                        ]),
                    )),
                    bias,
                },
            )),
            Some(Token::Line) => {
                Ok(self.make_pattern(start_cursor, PatternKind::Builtin(Builtin::Line)))
            }
            Some(Token::LParen) => {
                if self.is_next_inlined_statement() {
                    let stmt = self.parse_statement()?;
                    let name = stmt.name.clone();
                    self.inlined_statements.push(stmt);
                    self.expect(&Token::RParen)?;
                    Ok(self.make_pattern(start_cursor, PatternKind::Variable(name)))
                } else {
                    let inner = self.parse_pattern()?;
                    self.expect(&Token::RParen)?;
                    Ok(self.make_pattern(start_cursor, PatternKind::Group(Box::new(inner))))
                }
            }

            _ => Err(self.unexpected_token("pattern")),
        }
    }
    fn is_next_inlined_statement(&self) -> bool {
        if self.cursor + 1 >= self.tokens.len() {
            return false;
        }
        let should_be_assign = &self.tokens[self.cursor + 1].token;
        self.try_get_lvalue().is_some() && matches!(should_be_assign, Token::Equals)
    }

    fn parse_capture_clause(&mut self) -> StrqlResult<CaptureClause> {
        self.expect(&Token::Add)?;

        let (name, is_object) = if self.check(&[&Token::To]) {
            (String::new(), false)
        } else {
            let n = self.expect_identifier()?;
            let obj = if self.check(&[&Token::LBrace]) {
                self.advance_cursor_and_get();
                self.expect(&Token::RBrace)?;
                true
            } else {
                false
            };
            (n, obj)
        };

        self.expect(&Token::To)?;

        let path = self.parse_capture_path()?;

        Ok(CaptureClause {
            name,
            is_object,
            path,
        })
    }

    fn parse_capture_path(&mut self) -> StrqlResult<CapturePath> {
        let mut segments = Vec::new();

        // Must start with ROOT or a node name
        if self.check(&[&Token::Root]) {
            self.advance_cursor_and_get();
            segments.push(PathSegment::Root);
        } else {
            segments.push(PathSegment::Field(self.expect_identifier()?));
        }

        loop {
            if self.check(&[&Token::Dot]) {
                self.advance_cursor_and_get();
                segments.push(PathSegment::Field(self.expect_identifier()?));
            } else if self.check(&[&Token::LBracket]) {
                self.advance_cursor_and_get();
                if self.check(&[&Token::RBracket]) {
                    self.advance_cursor_and_get();
                    segments.push(PathSegment::ArrayAppend);
                } else {
                    let var = self.expect_identifier()?;
                    self.expect(&Token::RBracket)?;
                    segments.push(PathSegment::DynamicField(var));
                }
            } else {
                break;
            }
        }

        Ok(CapturePath { segments })
    }

    fn skip_newlines(&mut self) {
        while self.check(&[&Token::NewlineChar, &Token::CrLf]) {
            self.advance_cursor_and_get();
        }
    }

    fn is_at_end(&self) -> bool {
        self.cursor >= self.tokens.len()
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor).map(|t| &t.token)
    }

    fn get_and_advance_cursor(&mut self) -> Option<&Token> {
        self.tokens.get(self.cursor).map(|t| {
            self.cursor += 1;
            &t.token
        })
    }

    fn check(&self, expected: &[&Token]) -> bool {
        self.peek().map(|t| expected.contains(&t)).unwrap_or(false)
    }

    fn advance_cursor_and_get(&mut self) -> Option<&Token> {
        if !self.is_at_end() {
            self.cursor += 1;
        }
        self.tokens.get(self.cursor - 1).map(|t| &t.token)
    }

    fn expect(&mut self, expected: &Token) -> StrqlResult<()> {
        if self.check(&[expected]) {
            self.advance_cursor_and_get();
            Ok(())
        } else {
            Err(self.unexpected_token(expected.to_string().as_str()))
        }
    }

    fn expect_identifier(&mut self) -> StrqlResult<String> {
        self.lvalue()
    }

    fn try_get_lvalue(&self) -> Option<String> {
        self.peek().and_then(|tok| match tok {
            Token::Identifier(idf) => Some(idf.clone()),
            Token::Text
            | Token::Root
            | Token::Or
            | Token::Add
            | Token::To
            | Token::SplitBy
            | Token::AnyCase
            | Token::Upper
            | Token::Lower
            | Token::Lazy
            | Token::Greedy
            | Token::N
            | Token::Word
            | Token::Line
            | Token::Newline
            | Token::Space
            | Token::AnyChar
            | Token::Any
            | Token::Digit
            | Token::Letter
            | Token::Alphanum => Some(tok.to_string().to_ascii_uppercase()),
            _ => None,
        })
    }

    fn lvalue(&mut self) -> StrqlResult<String> {
        if let Some(name) = self.try_get_lvalue() {
            self.advance_cursor_and_get();
            Ok(name)
        } else {
            Err(self.unexpected_token("identifier or keyword"))
        }
    }

    fn unexpected_token(&self, expected: &str) -> StrqlError {
        let (found, span) = match self.tokens.get(self.cursor) {
            Some(t) => (t.token.to_string(), t.span.start),
            None => (
                "end of input".to_string(),
                self.tokens.last().map(|t| t.span.end).unwrap_or(0),
            ),
        };
        StrqlError::UnexpectedToken {
            _expected: expected.to_string(),
            _found: found,
            _src: self.src_to_named(),
            _span: span.into(),
        }
    }
}

pub fn parse(source: &str) -> StrqlResult<Program> {
    Parser::new(source)?.parse()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_rule() {
        let source = "name = WORD";
        let program = parse(source).unwrap();

        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement {
                name,
                pattern: _,
                capture,
                ..
            } => {
                assert_eq!(name, "name");
                assert!(capture.is_none());
            }
        }
    }

    #[test]
    fn test_rule_with_capture() {
        let source = "name = WORD -> ADD name TO ROOT";
        let program = parse(source).unwrap();

        assert_eq!(program.statements.len(), 1);
        match &program.statements[0] {
            Statement { capture, .. } => {
                let capture = capture.as_ref().unwrap();
                assert_eq!(capture.name, "name");
                assert!(!capture.is_object);
                assert!(matches!(
                    capture.path.segments.as_slice(),
                    [PathSegment::Root]
                ));
            }
        }
    }

    #[test]
    fn test_rule_with_object_capture() {
        let source = "line = memberlist -> ADD item{} TO ROOT.items[]";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { capture, .. } => {
                let capture = capture.as_ref().unwrap();
                assert_eq!(capture.name, "item");
                assert!(capture.is_object);
                assert!(capture.path.ends_with_array());
            }
        }
    }

    #[test]
    fn test_alternation() {
        let source = r#"sep = ", " OR " and ""#;
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => {
                assert!(matches!(pattern.node, PatternKind::OrChain(_)));
            }
        }
    }

    #[test]
    fn test_quantifier() {
        let source = "digits = 1..N DIGIT";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition {
                    min,
                    max,
                    pattern,
                    bias,
                } => {
                    assert!(matches!(min, Some(1)));
                    assert!(matches!(max, None));
                    assert!(matches!(pattern.node, PatternKind::Builtin(Builtin::Digit)));
                    assert_eq!(*bias, QuantifierBias::Neutral);
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_splitby() {
        let source = "list = item SPLITBY sep";
        let program = parse(source).unwrap();

        // SPLITBY is now expanded to a Sequence: item (sep item)*
        match &program.statements[0] {
            Statement { pattern, .. } => {
                assert!(matches!(pattern.node, PatternKind::Sequence(_)));
            }
        }
    }

    #[test]
    fn test_sequence() {
        let source = r#"line = name " is " value"#;
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Sequence(items) => {
                    assert_eq!(items.len(), 3);
                }
                _ => panic!("Expected sequence"),
            },
        }
    }

    #[test]
    fn test_dynamic_field() {
        let source = "value = WORD -> ADD value TO item[key]";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { capture, .. } => {
                let capture = capture.as_ref().unwrap();
                assert!(matches!(
                    capture.path.segments.as_slice(),
                    [PathSegment::Field(_), PathSegment::DynamicField(_)]
                ));
            }
        }
    }

    #[test]
    fn test_full_example() {
        let source = r#"
TEXT = line SPLITBY NEWLINE
line = memberlist " are " kind -> ADD item{} TO ROOT.items[]
memberlist = member SPLITBY sep
member = WORD -> ADD member TO item.members[]
sep = ", " OR " and "
kind = WORD -> ADD kind TO item
"#;
        let program = parse(source).unwrap();
        assert_eq!(program.statements.len(), 6);
    }

    #[test]
    fn test_grouped_ors() {
        let source = r#"
TEXT = (first OR second) " " (third OR fourth)
first = "A"
second = "B"
third = "C"
fourth = "D"
"#;
        let program = parse(source).unwrap();
        assert_eq!(program.statements.len(), 5);
    }

    #[test]
    fn test_lazy_quantifier() {
        let source = "digits = LAZY 1..N DIGIT";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition { bias, .. } => {
                    assert_eq!(*bias, QuantifierBias::Lazy);
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_greedy_quantifier() {
        let source = "digits = GREEDY 1..N DIGIT";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition { bias, .. } => {
                    assert_eq!(*bias, QuantifierBias::Greedy);
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_lazy_splitby() {
        let source = "list = item LAZY SPLITBY sep";
        let program = parse(source).unwrap();

        // SPLITBY is now expanded to: item (sep item)*
        // The quantifier should have Lazy bias
        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Sequence(parts) if parts.len() == 2 => match &parts[1].node {
                    PatternKind::Repetition { bias, .. } => {
                        assert_eq!(*bias, QuantifierBias::Lazy);
                    }
                    _ => panic!("Expected quantifier as second part"),
                },
                _ => panic!("Expected sequence"),
            },
        }
    }

    #[test]
    fn test_greedy_splitby() {
        let source = "list = item GREEDY SPLITBY sep";
        let program = parse(source).unwrap();

        // SPLITBY is now expanded to: item (sep item)*
        // The quantifier should have Greedy bias
        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Sequence(parts) if parts.len() == 2 => match &parts[1].node {
                    PatternKind::Repetition { bias, .. } => {
                        assert_eq!(*bias, QuantifierBias::Greedy);
                    }
                    _ => panic!("Expected quantifier as second part"),
                },
                _ => panic!("Expected sequence"),
            },
        }
    }

    #[test]
    fn test_lazy_any() {
        // LAZY ANY should create a quantifier with Lazy bias
        let source = "w = LAZY ANY";
        let program = parse(source).unwrap();
        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition {
                    bias,
                    min,
                    max: _,
                    pattern: inner,
                } => {
                    assert_eq!(*bias, QuantifierBias::Lazy);
                    assert!(matches!(min, Some(0)));
                    assert!(matches!(inner.node, PatternKind::Builtin(Builtin::AnyChar)));
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_greedy_any() {
        // GREEDY ANY should create a quantifier with Greedy bias
        let source = "w = GREEDY ANY";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition { bias, .. } => {
                    assert_eq!(*bias, QuantifierBias::Greedy);
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_inlined_statement() {
        let source = "TEXT = (ln = ANY) SPLITBY NEWLINE";
        let program = parse(source).unwrap();

        // Should have 2 statements: TEXT and ln
        assert_eq!(program.statements.len(), 2);

        // Find TEXT and ln
        let text_stmt = program
            .statements
            .iter()
            .find(|s| s.name == "TEXT")
            .unwrap();
        let ln_stmt = program.statements.iter().find(|s| s.name == "ln").unwrap();

        // TEXT = ln SPLITBY NEWLINE
        match &text_stmt.pattern.node {
            PatternKind::Sequence(parts) => {
                assert!(matches!(&parts[0].node, PatternKind::Variable(v) if v == "ln"));
            }
            _ => panic!("Expected sequence for TEXT"),
        }

        // ln = ANY
        assert!(matches!(
            ln_stmt.pattern.node,
            PatternKind::Repetition { .. }
        ));
    }

    #[test]
    fn test_inlined_with_capture() {
        let source = "TEXT = (w = WORD -> ADD TO ROOT.words[])";
        let program = parse(source).unwrap();

        assert_eq!(program.statements.len(), 2);
        let w_stmt = program.statements.iter().find(|s| s.name == "w").unwrap();
        assert!(w_stmt.capture.is_some());
        // ROOT . words []
        // path.segments = [Root, Field("words"), ArrayAppend]
        assert_eq!(w_stmt.capture.as_ref().unwrap().path.segments.len(), 3);
    }

    #[test]
    fn test_quantifier_numeric_bounds() {
        // Both bounds are numbers
        let source = "digits = 0..5 DIGIT";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition { min, max, .. } => {
                    assert!(matches!(min, Some(0)));
                    assert!(matches!(max, Some(5)));
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_quantifier_lowercase_n() {
        // Lowercase n should work for unbounded
        let source = "digits = 1..n DIGIT";
        let program = parse(source).unwrap();

        match &program.statements[0] {
            Statement { pattern, .. } => match &pattern.node {
                PatternKind::Repetition { min, max, .. } => {
                    assert!(matches!(min, Some(1)));
                    assert!(matches!(max, None));
                }
                _ => panic!("Expected quantifier"),
            },
        }
    }

    #[test]
    fn test_quantifier_identifier_min_rejected() {
        // Identifier as min bound should be rejected
        let source = "x = myvar..5 DIGIT";
        let result = parse(source);
        assert!(result.is_err());
    }

    #[test]
    fn test_quantifier_identifier_max_rejected() {
        // Identifier as max bound should be rejected
        let source = "x = 0..myvar DIGIT";
        let result = parse(source);
        assert!(result.is_err());
    }
}
