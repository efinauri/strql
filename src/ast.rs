use std::collections::HashMap;
use std::ops::Range;

pub struct Program {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Range<usize>,
}

/// `<name> = <pattern> -> <capture>`
///
/// example: `TEXT = ln SPLITBY NEWLINE -> ADD item{} TO ROOT.items[]`
pub struct Statement {
    pub name: String,
    pub pattern: Pattern,
    pub capture: Option<CaptureClause>,
    pub span: Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuantifierBias {
    #[default]
    Neutral,
    Greedy,
    Lazy,
}

pub type Pattern = Spanned<PatternKind>;

#[derive(PartialEq, Clone, Debug)]
pub enum PatternKind {
    Literal(String),
    Variable(String),
    Builtin(Builtin),
    Sequence(Vec<Pattern>),
    OrChain(Vec<Pattern>),
    Repetition {
        min: Bound,
        max: Bound,
        pattern: Box<Pattern>,
        bias: QuantifierBias,
    },
    AnyCase(Box<Pattern>),
    Upper(Box<Pattern>),
    Lower(Box<Pattern>),
    Group(Box<Pattern>),
}

#[derive(Debug, PartialEq, Clone)]
pub enum Bound {
    /// `0..<1>`
    Literal(usize),
    /// `0..<n>`
    Variable(String),
    /// implicit in higher order constructs like `WORD` (which is `0..<anon> LETTER`)
    Anonymous,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Builtin {
    Digit,
    Letter,
    AnyChar,
    Newline,
    Space,
    Line, // other multichar builtins like words are missing because they're easy to desugar
}

/// `ADD <name><{} if is_object> TO <path>`
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureClause {
    pub name: String,
    /// distinguishes between `ADD item{} TO ROOT.items[]` and `ADD item TO ROOT.items[]`
    pub is_object: bool,
    pub path: CapturePath,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CapturePath {
    pub segments: Vec<PathSegment>,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PathSegment {
    Root,
    /// `ROOT.field`
    Field(String),
    /// `ROOT.field[key]`
    DynamicField(String),
    /// `ROOT.arr[]`
    ArrayAppend,
}

impl Program {
    pub fn new() -> Self {
        Self {
            statements: Vec::new(),
        }
    }

    pub fn variable_map(&self) -> HashMap<&str, &Pattern> {
        self.statements
            .iter()
            .map(|s| (s.name.as_str(), &s.pattern))
            .collect()
    }
}

impl Default for Program {
    fn default() -> Self {
        Self::new()
    }
}

impl Pattern {
    pub fn is_literal(&self) -> bool {
        matches!(self.node, PatternKind::Literal(_))
    }
    pub fn is_variable(&self) -> bool {
        matches!(self.node, PatternKind::Variable(_))
    }
    pub fn variables(&self) -> Vec<&str> {
        let mut vars = Vec::new();
        self.node.collect_variables(&mut vars);
        vars
    }
}

impl PatternKind {
    fn collect_variables<'a>(&'a self, vars: &mut Vec<&'a str>) {
        match self {
            PatternKind::Variable(name) => vars.push(name),
            PatternKind::Sequence(patterns) | PatternKind::OrChain(patterns) => {
                for p in patterns {
                    p.node.collect_variables(vars);
                }
            }
            PatternKind::Repetition {
                pattern,
                min,
                max,
                bias: _,
            } => {
                pattern.node.collect_variables(vars);
                if let Bound::Variable(v) = min {
                    vars.push(v);
                }
                if let Bound::Variable(v) = max {
                    vars.push(v);
                }
            }
            PatternKind::AnyCase(p) | PatternKind::Upper(p) | PatternKind::Lower(p) | PatternKind::Group(p) => {
                p.node.collect_variables(vars);
            }
            PatternKind::Literal(_) | PatternKind::Builtin(_) => {}
        }
    }
}

impl CapturePath {
    pub fn root() -> Self {
        Self {
            segments: vec![PathSegment::Root],
        }
    }
    pub fn add_field(mut self, name: impl Into<String>) -> Self {
        self.segments.push(PathSegment::Field(name.into()));
        self
    }
    pub fn add_dynamic_field(mut self, var: impl Into<String>) -> Self {
        self.segments.push(PathSegment::DynamicField(var.into()));
        self
    }
    pub fn add_array_append(mut self) -> Self {
        self.segments.push(PathSegment::ArrayAppend);
        self
    }
    pub fn ends_with_array(&self) -> bool {
        matches!(self.segments.last(), Some(PathSegment::ArrayAppend))
    }
}
