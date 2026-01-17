mod examples {
    use pretty_assertions::assert_eq;
    use std::fs;
    use std::path::Path;
    use strql::error::StrqlError;
    use strql::evaluate_partition;

    #[test]
    fn test_examples() {
        let examples_dir = Path::new("examples");
        let mut entries: Vec<_> = fs::read_dir(examples_dir)
            .unwrap()
            .map(|res| res.unwrap())
            .collect();

        entries.sort_by_key(|e| e.path());

        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                println!("Running example test: {:?}", path.file_name().unwrap());
                run_example_test(&path);
            }
        }
    }

    fn run_example_test(path: &Path) {
        let test_name = path.file_name().unwrap().to_str().unwrap();

        let query_path = path.join("query.strql");
        let input_path = path.join("test.txt");
        let expected_path = path.join("expected.json");

        if !query_path.exists() || !input_path.exists() {
            println!(
                "Skipping {:?} (missing query.strql or test.txt)",
                path.file_name().unwrap()
            );
            return;
        }

        let source = fs::read_to_string(query_path).expect("Failed to read query.strql");
        let input = fs::read_to_string(input_path).expect("Failed to read test.txt");

        let result = evaluate_partition(&source, &input);

        if expected_path.exists() {
            let expected_json_str =
                fs::read_to_string(expected_path).expect("Failed to read expected.json");
            let expected_json: serde_json::Value =
                serde_json::from_str(&expected_json_str).expect("Failed to parse expected.json");

            match result {
                Ok(actual_json) => {
                    assert_eq!(
                        actual_json, expected_json,
                        "Output mismatch in example: {}",
                        test_name
                    );
                }
                Err(e) => {
                    panic!("Example {} failed unexpectedly: {:?}", test_name, e);
                }
            }
        } else {
            match result {
                Ok(val) => panic!(
                    "Example {} should have failed but succeeded. Result: {:?}",
                    test_name, val
                ),
                Err(StrqlError::AmbiguousParse { .. }) => println!("  (confirmed ambiguity error)"),
                Err(StrqlError::PatternNoMatch { .. }) => println!("  (confirmed no match error)"),
                Err(StrqlError::PartialMatch { .. }) => {
                    println!("  (confirmed partial match error)")
                }
                Err(e) => panic!(
                    "Example {} failed with unexpected error type: {:?}",
                    test_name, e
                ),
            }
        }
    }
}
