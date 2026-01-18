use std::time::Instant;
use strql::evaluate_partition;

/// Simple seeded PRNG for reproducible tests
struct Rng {
    state: u64,
}

impl Rng {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.state
    }

    fn next_in_range(&mut self, min: usize, max: usize) -> usize {
        if min >= max {
            return min;
        }
        min + (self.next() as usize % (max - min))
    }

    fn next_char(&mut self, chars: &[char]) -> char {
        chars[self.next() as usize % chars.len()]
    }

    fn gen_string(&mut self, len: usize, chars: &[char]) -> String {
        (0..len).map(|_| self.next_char(chars)).collect()
    }
}

/// Measures execution time with statistical significance.
/// Returns (mean_ns, std_error_ns).
fn measure_time_stats<F: Fn()>(f: F, runs: usize) -> (f64, f64) {
    const WARMUP_RUNS: usize = 2;

    // Warmup
    for _ in 0..WARMUP_RUNS {
        f();
    }

    // Measure
    let mut times = Vec::with_capacity(runs);
    for _ in 0..runs {
        let start = Instant::now();
        f();
        times.push(start.elapsed().as_nanos() as f64);
    }

    let mean = times.iter().sum::<f64>() / times.len() as f64;
    if times.len() < 2 {
        return (mean, 0.0);
    }
    let variance = times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / (times.len() - 1) as f64;
    let std_dev = variance.sqrt();
    let std_error = std_dev / (times.len() as f64).sqrt();

    (mean, std_error)
}

mod fuzzing {
    use super::*;

    const ALPHA: &[char] = &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z',
    ];
    const ALPHANUMERIC: &[char] = &[
        'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', 'p', 'q', 'r',
        's', 't', 'u', 'v', 'w', 'x', 'y', 'z', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    ];
    const DIGITS: &[char] = &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9'];

    fn generate_matching_input_for_word_splitby(
        rng: &mut Rng,
        word_count: usize,
        sep: &str,
    ) -> String {
        let mut result = String::new();
        for i in 0..word_count {
            if i > 0 {
                result.push_str(sep);
            }
            let word_len = rng.next_in_range(1, 10);
            result.push_str(&rng.gen_string(word_len, ALPHA));
        }
        result
    }

    fn generate_matching_input_for_lines(rng: &mut Rng, line_count: usize) -> String {
        let mut result = String::new();
        let line_chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz ".chars().collect();
        for i in 0..line_count {
            if i > 0 {
                result.push('\n');
            }
            let line_len = rng.next_in_range(10, 100);
            result.push_str(&rng.gen_string(line_len, &line_chars));
        }
        result
    }

    fn generate_nomatch_input(rng: &mut Rng, size: usize, query_expects: &str) -> String {
        // Generate input that doesn't match the query expectations
        match query_expects {
            "digits" => rng.gen_string(size, ALPHA), // Query expects digits, give letters
            "letters" => rng.gen_string(size, DIGITS), // Query expects letters, give digits
            _ => rng.gen_string(size, ALPHA),
        }
    }

    fn generate_ambiguous_input(rng: &mut Rng, size: usize) -> String {
        // Generate input that could be parsed in multiple ways
        // e.g., for a splitby query, include the separator in variable parts
        let chars: Vec<char> = "ab.".chars().collect();
        rng.gen_string(size, &chars)
    }

    #[test]
    fn fuzz_word_splitby_matching() {
        let query = r#"
            TEXT = word GREEDY SPLITBY ", "
            word = WORD -> ADD TO ROOT.words[]
        "#;

        println!("\n=== Fuzzing: word splitby (matching inputs) ===");
        const ITERATIONS: usize = 20;
        const MEASUREMENT_RUNS: usize = 5;

        let word_counts = [10, 20, 50, 100];
        for &word_count in &word_counts {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut success_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = generate_matching_input_for_word_splitby(&mut rng, word_count, ", ");

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_ok() {
                    success_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Words: {}, Avg time: {:.0} ± {:.0} ns, Success rate: {}/{}",
                word_count, avg_time, std_error, success_count, ITERATIONS
            );

            assert_eq!(success_count, ITERATIONS, "Some matching inputs failed to parse");
        }
    }

    #[test]
    fn fuzz_lines_matching() {
        let query = r#"
            TEXT = line GREEDY SPLITBY NEWLINE
            line = LINE -> ADD TO ROOT.lines[]
        "#;

        println!("\n=== Fuzzing: lines (matching inputs) ===");
        const ITERATIONS: usize = 20;
        const MEASUREMENT_RUNS: usize = 5;

        let line_counts = [10, 20, 50, 100];
        for &line_count in &line_counts {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut success_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = generate_matching_input_for_lines(&mut rng, line_count);

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_ok() {
                    success_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Lines: {}, Avg time: {:.0} ± {:.0} ns, Success rate: {}/{}",
                line_count, avg_time, std_error, success_count, ITERATIONS
            );

            assert_eq!(success_count, ITERATIONS, "Some matching inputs failed to parse");
        }
    }

    #[test]
    fn fuzz_digit_sequence_nomatch() {
        let query = "TEXT = 1..N DIGIT";

        println!("\n=== Fuzzing: digit sequence (no-match inputs) ===");
        const ITERATIONS: usize = 20;
        const MEASUREMENT_RUNS: usize = 5;

        let sizes = [100, 200, 500, 1000];
        for &size in &sizes {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut nomatch_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = generate_nomatch_input(&mut rng, size, "digits");

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_err() {
                    nomatch_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Size: {}, Avg time: {:.0} ± {:.0} ns, NoMatch rate: {}/{}",
                size, avg_time, std_error, nomatch_count, ITERATIONS
            );

            assert_eq!(nomatch_count, ITERATIONS, "Some no-match inputs unexpectedly matched");
        }
    }

    #[test]
    fn fuzz_ambiguous_splitby() {
        // This query without modifiers should detect ambiguity
        let query = r#"
            TEXT = w SPLITBY "."
            w = ANY
        "#;

        println!("\n=== Fuzzing: ambiguous splitby ===");
        const ITERATIONS: usize = 20;
        const MEASUREMENT_RUNS: usize = 5;

        let sizes = [20, 40, 80, 160];
        for &size in &sizes {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut ambiguous_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = generate_ambiguous_input(&mut rng, size);

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_err() {
                    ambiguous_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Size: {}, Avg time: {:.0} ± {:.0} ns, Ambiguous/Error rate: {}/{}",
                size, avg_time, std_error, ambiguous_count, ITERATIONS
            );
        }
    }

    #[test]
    fn fuzz_alternation_random() {
        let query = r#"
            TEXT = 1..N choice
            choice = "a" OR "b" OR "c" OR "d"
        "#;

        println!("\n=== Fuzzing: alternation chain (random inputs) ===");
        const ITERATIONS: usize = 20;
        const MEASUREMENT_RUNS: usize = 5;

        let sizes = [100, 200, 400, 800];
        let valid_chars: Vec<char> = "abcd".chars().collect();

        for &size in &sizes {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut success_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = rng.gen_string(size, &valid_chars);

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_ok() {
                    success_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Size: {}, Avg time: {:.0} ± {:.0} ns, Success rate: {}/{}",
                size, avg_time, std_error, success_count, ITERATIONS
            );

            assert_eq!(success_count, ITERATIONS, "Some valid alternation inputs failed");
        }
    }

    #[test]
    fn fuzz_nested_quantifiers_random() {
        let query = "TEXT = 1..N (1..N LETTER)";

        println!("\n=== Fuzzing: nested quantifiers (random inputs) ===");
        const ITERATIONS: usize = 15;
        const MEASUREMENT_RUNS: usize = 5;

        let sizes = [50, 100, 200];

        for &size in &sizes {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = rng.gen_string(size, ALPHA);

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Size: {}, Avg time: {:.0} ± {:.0} ns",
                size, avg_time, std_error
            );
        }
    }

    #[test]
    fn fuzz_complex_capture_random() {
        let query = r#"
            TEXT = item GREEDY SPLITBY NEWLINE
            item = key ": " value -> ADD item{} TO ROOT.items[]
            key = WORD -> ADD TO item
            value = LINE -> ADD TO item
        "#;

        println!("\n=== Fuzzing: complex capture (random key-value inputs) ===");
        const ITERATIONS: usize = 15;
        const MEASUREMENT_RUNS: usize = 5;

        let line_counts = [20, 40, 80];

        for &line_count in &line_counts {
            let mut total_time = 0.0;
            let mut total_time_sq = 0.0;
            let mut success_count = 0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let mut input = String::new();
                for i in 0..line_count {
                    if i > 0 {
                        input.push('\n');
                    }
                    let key_len = rng.next_in_range(3, 10);
                    let key = rng.gen_string(key_len, ALPHA);
                    let value_len = rng.next_in_range(5, 50);
                    let value_chars: Vec<char> =
                        "abcdefghijklmnopqrstuvwxyz0123456789 ".chars().collect();
                    let value = rng.gen_string(value_len, &value_chars);
                    input.push_str(&format!("{}: {}", key, value));
                }

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                let result = evaluate_partition(query, &input);
                if result.is_ok() {
                    success_count += 1;
                }

                total_time += mean;
                total_time_sq += mean * mean;
            }

            let avg_time = total_time / ITERATIONS as f64;
            let variance = (total_time_sq / ITERATIONS as f64) - (avg_time * avg_time);
            let std_error = (variance / ITERATIONS as f64).sqrt();

            println!(
                "Lines: {}, Avg time: {:.0} ± {:.0} ns, Success rate: {}/{}",
                line_count, avg_time, std_error, success_count, ITERATIONS
            );

            assert_eq!(success_count, ITERATIONS, "Some valid complex capture inputs failed");
        }
    }

    #[test]
    fn fuzz_scaling_behavior() {
        // Test that time grows polynomially (not exponentially) with input size
        let query = "TEXT = 0..N ANYCHAR";

        println!("\n=== Fuzzing: scaling behavior check ===");
        const ITERATIONS: usize = 10;
        const MEASUREMENT_RUNS: usize = 5;

        let sizes = [500, 1000, 2000, 4000];
        let mut prev_avg_time = 0.0;

        for &size in &sizes {
            let mut total_time = 0.0;

            for iter in 0..ITERATIONS {
                let mut rng = Rng::new(42 + iter as u64);
                let input = rng.gen_string(size, ALPHANUMERIC);

                let (mean, _) = measure_time_stats(
                    || {
                        let _ = evaluate_partition(query, &input);
                    },
                    MEASUREMENT_RUNS,
                );

                total_time += mean;
            }

            let avg_time = total_time / ITERATIONS as f64;

            if prev_avg_time > 10000.0 {
                let ratio = avg_time / prev_avg_time;
                println!(
                    "Size: {}, Avg time: {:.0} ns, Ratio from previous: {:.2}",
                    size, avg_time, ratio
                );

                // For polynomial time, ratio should be bounded (2^k for O(n^k))
                // Allow up to 16x for O(n^4) or noise
                assert!(
                    ratio < 16.0,
                    "Scaling appears exponential: ratio {:.2} at size {}",
                    ratio,
                    size
                );
            } else {
                println!("Size: {}, Avg time: {:.0} ns", size, avg_time);
            }

            prev_avg_time = avg_time;
        }
    }
}
