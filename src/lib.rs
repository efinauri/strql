pub mod ast;
pub mod error;
pub mod lexer;
pub mod parser;
mod solver;

use crate::error::StrqlResult;
pub use ast::{Builtin, Pattern, Program, Statement};

pub fn evaluate_partition(source: &str, input: &str) -> StrqlResult<serde_json::Value> {
    let program = parser::parse(source)?;
    let mut solver = solver::Solver::new(&program)?;
    solver.solve(input)
}

#[cfg(test)]
mod integration_tests {
    use super::*;

    #[test]
    fn test_hello_world() {
        let source = r#"TEXT = "Hello, World!""#;
        let result = evaluate_partition(source, "Hello, World!").unwrap();
        assert!(result.is_object());
    }

    #[test]
    fn test_simple_extraction() {
        let source = r#"
TEXT = "Name: " name
name = WORD -> ADD name TO ROOT
"#;
        let result = evaluate_partition(source, "Name: Alice").unwrap();
        assert_eq!(result["name"], "Alice");
    }

    #[test]
    fn test_alternation() {
        let source = r#"
TEXT = greeting
greeting = "hello" OR "hi" OR "hey"
"#;
        assert!(evaluate_partition(source, "hello").is_ok());
        assert!(evaluate_partition(source, "hi").is_ok());
        assert!(evaluate_partition(source, "hey").is_ok());
        assert!(evaluate_partition(source, "bye").is_err());
    }

    #[test]
    fn test_quantifier_range() {
        let source = "TEXT = 2..4 DIGIT";
        assert!(evaluate_partition(source, "12").is_ok());
        assert!(evaluate_partition(source, "123").is_ok());
        assert!(evaluate_partition(source, "1234").is_ok());
        assert!(evaluate_partition(source, "1").is_err());
        assert!(evaluate_partition(source, "12345").is_err());
    }

    #[test]
    fn test_optional() {
        let source = r#"
TEXT = name (0..1 " Jr.")
name = WORD
"#;
        assert!(evaluate_partition(source, "Bob").is_ok());
        assert!(evaluate_partition(source, "Bob Jr.").is_ok());
    }

    #[test]
    fn test_splitby() {
        let source = r#"
TEXT = item SPLITBY ", "
item = 1..N LETTER
"#;
        let result = evaluate_partition(source, "a, b, c").unwrap();
        assert!(result.is_object());
    }

    #[test]
    fn test_constraint_failure() {
        let source = r#"
TEXT = "<" tag ">" content "</" close ">"
tag = 1..N LETTER
close = 1..N LETTER
content = 1..N LETTER
TRUE = tag == close
"#;
        assert!(evaluate_partition(source, "<div>hello</span>").is_err());
    }

    #[test]
    fn test_anycase() {
        let source = r#"
TEXT = ANYCASE "hello"
"#;
        assert!(evaluate_partition(source, "hello").is_ok());
        assert!(evaluate_partition(source, "HELLO").is_ok());
        assert!(evaluate_partition(source, "HeLLo").is_ok());
    }

    #[test]
    fn test_upper() {
        let source = r#"
TEXT = UPPER WORD
"#;
        assert!(evaluate_partition(source, "HELLO").is_ok());
        assert!(evaluate_partition(source, "hello").is_err());
        assert!(evaluate_partition(source, "Hello").is_err());

        let source = r#"TEXT = UPPER "abc""#;
        assert!(evaluate_partition(source, "ABC").is_ok());
        assert!(evaluate_partition(source, "abc").is_err());

        let source = r#"TEXT = UPPER "123""#;
        assert!(evaluate_partition(source, "123").is_ok());
    }

    #[test]
    fn test_lower() {
        let source = r#"
TEXT = LOWER WORD
"#;
        assert!(evaluate_partition(source, "hello").is_ok());
        assert!(evaluate_partition(source, "HELLO").is_err());
        assert!(evaluate_partition(source, "Hello").is_err());

        let source = r#"TEXT = LOWER "ABC""#;
        assert!(evaluate_partition(source, "abc").is_ok());
        assert!(evaluate_partition(source, "ABC").is_err());
    }

    #[test]
    fn test_nested_groups() {
        let source = r#"
TEXT = (first OR second) " " (third OR fourth)
first = "A"
second = "B"
third = "C"
fourth = "D"
"#;
        assert!(evaluate_partition(source, "A C").is_ok());
        assert!(evaluate_partition(source, "B D").is_ok());
        assert!(evaluate_partition(source, "A D").is_ok());
        assert!(evaluate_partition(source, "B C").is_ok());
    }

    #[test]
    fn test_builtin_patterns() {
        // DIGIT
        let source = "TEXT = DIGIT";
        assert!(evaluate_partition(source, "5").is_ok());
        assert!(evaluate_partition(source, "a").is_err());

        // LETTER
        let source = "TEXT = LETTER";
        assert!(evaluate_partition(source, "a").is_ok());
        assert!(evaluate_partition(source, "5").is_err());

        // WORD
        let source = "TEXT = WORD";
        assert!(evaluate_partition(source, "hello").is_ok());
        assert!(evaluate_partition(source, "hello world").is_err());

        // LINE
        let source = "TEXT = LINE";
        assert!(evaluate_partition(source, "hello world").is_ok());

        // NEWLINE
        let source = r#"TEXT = "a" NEWLINE "b""#;
        assert!(evaluate_partition(source, "a\nb").is_ok());

        // SPACE
        let source = r#"TEXT = "a" SPACE "b""#;
        assert!(evaluate_partition(source, "a b").is_ok());

        // ANYCHAR (single character)
        let source = "TEXT = ANYCHAR ANYCHAR ANYCHAR";
        assert!(evaluate_partition(source, "abc").is_ok());
        assert!(evaluate_partition(source, "ab").is_err());

        // ANY (0..N ANYCHAR - matches any amount >= 0)
        let source = "TEXT = ANY";
        assert!(evaluate_partition(source, "abc").is_ok());
        assert!(evaluate_partition(source, "").is_ok());
    }

    #[test]
    fn test_capture_to_root() {
        let source = r#"
TEXT = name " " age
name = WORD -> ADD name TO ROOT
age = 1..N DIGIT -> ADD age TO ROOT
"#;
        let result = evaluate_partition(source, "Alice 25").unwrap();
        assert_eq!(result["name"], "Alice");
        assert_eq!(result["age"], "25");
    }

    #[test]
    fn test_capture_array() {
        let source = r#"
TEXT = color SPLITBY ", "
color = WORD -> ADD color TO ROOT.colors[]
"#;
        let result = evaluate_partition(source, "red, green, blue").unwrap();
        let colors = result["colors"].as_array().unwrap();
        assert_eq!(colors.len(), 3);
        assert_eq!(colors[0], "red");
        assert_eq!(colors[1], "green");
        assert_eq!(colors[2], "blue");
    }

    #[test]
    fn test_ambiguity_detection_without_modifiers() {
        // Without LAZY/GREEDY, this should detect ambiguity because both
        // SPLITBY and ANY can consume arbitrary text in multiple ways.
        // Input "a. b. c." has multiple valid partitions.
        let source = r#"
TEXT = w SPLITBY "."
w = ANY -> ADD TO ROOT.results[]
"#;
        let result = evaluate_partition(source, "a. b. c.");
        assert!(
            result.is_err(),
            "Should detect ambiguity without LAZY/GREEDY modifiers"
        );
    }

    #[test]
    fn test_greedy_splitby_with_lazy_any() {
        // GREEDY SPLITBY should prefer more splits.
        // LAZY ANY should prefer matching fewer characters.
        // For "a. b. c.", with GREEDY SPLITBY and LAZY ANY:
        // - w="a", sep=".", w=" b", sep=".", w=" c" (can't split more, nothing after last ".")
        // Expected: ["a", " b", " c"]
        let source = r#"
TEXT = w GREEDY SPLITBY "."
w = ANY -> ADD TO ROOT.results[]
"#;
        let result = evaluate_partition(source, "a. b. c.").unwrap();
        let results = result["results"].as_array().unwrap();
        println!("{:?}", results);
        assert_eq!(results.len(), 3, "GREEDY SPLITBY should produce 3 elements");
        assert_eq!(results[0], "a");
        assert_eq!(results[1], " b");
        assert_eq!(results[2], " c");
    }

    #[test]
    fn test_lazy_splitby_with_greedy_any() {
        // LAZY SPLITBY should prefer fewer splits.
        // GREEDY ANY should prefer matching more characters.
        // For "a. b. c.", this should match w="a. b. c." with no splits.
        // Expected: ["a. b. c."]
        let source = r#"
TEXT = w LAZY SPLITBY "."
w = GREEDY ANY -> ADD TO ROOT.results[]
"#;
        let result = evaluate_partition(source, "a. b. c.").unwrap();
        let results = result["results"].as_array().unwrap();
        assert_eq!(results.len(), 1, "LAZY SPLITBY should produce 1 element");
        assert_eq!(results[0], "a. b. c.");
    }
}
