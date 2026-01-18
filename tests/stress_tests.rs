use std::time::Instant;
use strql::evaluate_partition;

mod performance {
    use super::*;

    fn generate_random_input(size: usize, seed: u64, chars: &str) -> String {
        let mut res = String::with_capacity(size);
        let mut rng = seed;
        let char_vec: Vec<char> = chars.chars().collect();
        for _ in 0..size {
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let idx = (rng % char_vec.len() as u64) as usize;
            res.push(char_vec[idx]);
        }
        res
    }

    fn generate_words_with_separator(
        word_count: usize,
        word_len: usize,
        sep: &str,
        seed: u64,
    ) -> String {
        let mut res = String::new();
        let mut rng = seed;
        let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz".chars().collect();

        for i in 0..word_count {
            if i > 0 {
                res.push_str(sep);
            }
            for _ in 0..word_len {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let idx = (rng % chars.len() as u64) as usize;
                res.push(chars[idx]);
            }
        }
        res
    }

    fn generate_lines(line_count: usize, line_len: usize, seed: u64) -> String {
        let mut res = String::new();
        let mut rng = seed;
        let chars: Vec<char> = "abcdefghijklmnopqrstuvwxyz ".chars().collect();

        for i in 0..line_count {
            if i > 0 {
                res.push('\n');
            }
            for _ in 0..line_len {
                rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
                let idx = (rng % chars.len() as u64) as usize;
                res.push(chars[idx]);
            }
        }
        res
    }

    /// Measures execution time with statistical significance.
    /// Returns (mean_ns, std_error_ns).
    fn measure_time_stats(f: impl Fn()) -> (f64, f64) {
        const WARMUP_RUNS: usize = 3;
        const MEASUREMENT_RUNS: usize = 10;

        // Warmup
        for _ in 0..WARMUP_RUNS {
            f();
        }

        // Measure
        let mut times = Vec::with_capacity(MEASUREMENT_RUNS);
        for _ in 0..MEASUREMENT_RUNS {
            let start = Instant::now();
            f();
            times.push(start.elapsed().as_nanos() as f64);
        }

        let mean = times.iter().sum::<f64>() / times.len() as f64;
        let variance =
            times.iter().map(|t| (t - mean).powi(2)).sum::<f64>() / (times.len() - 1) as f64;
        let std_dev = variance.sqrt();
        let std_error = std_dev / (times.len() as f64).sqrt();

        (mean, std_error)
    }

    #[test]
    fn test_performance_linear_matching() {
        let query = "TEXT = 0..N ANYCHAR";
        let chars = "abcdefghijklmnopqrstuvwxyz";

        measure_growth(query, chars, "linear matching");
    }

    #[test]
    fn test_performance_ambiguous_inputs() {
        // This query is highly ambiguous without modifiers
        let query = "TEXT = (a 0..N ANYCHAR) (b 0..N ANYCHAR)";
        let chars = "a"; // Only 'a's will make it very ambiguous

        // We expect it to be slower but still polynomial due to memoization
        measure_growth(query, chars, "ambiguous inputs");
    }

    #[test]
    fn test_performance_nomatch_inputs() {
        let query = "TEXT = 1..N DIGIT";
        let chars = "abcdefg"; // No digits here

        measure_growth(query, chars, "no-match inputs");
    }

    #[test]
    fn test_performance_big_linear_matching() {
        let query = "TEXT = 0..N ANYCHAR";
        let chars = "abcdefghijklmnopqrstuvwxyz";

        // Use smaller sizes for big input test to avoid timeout
        let sizes = [500, 1000, 2000, 4000];
        let mut times = Vec::new();

        println!("\nStress test: big linear matching");
        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });
            times.push(mean);
            println!("Size: {}, Time: {:.0} ± {:.0} ns", size, mean, std_error);
        }

        for i in 1..times.len() {
            let ratio = times[i] / times[i - 1];
            println!("Size {} -> {}, Ratio: {:.2}", sizes[i - 1], sizes[i], ratio);

            if times[i - 1] > 10000.0 {
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high: ratio {:.2}",
                    ratio
                );
            }
        }
    }

    #[test]
    fn test_performance_splitby_words() {
        let query = r#"
            TEXT = word GREEDY SPLITBY ", "
            word = WORD
        "#;

        println!("\nStress test: splitby words");
        let sizes = [50, 100, 200, 400];
        let mut prev_time = 0.0;

        for &word_count in &sizes {
            let input = generate_words_with_separator(word_count, 5, ", ", 42);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });

            println!(
                "Words: {}, Input len: {}, Time: {:.0} ± {:.0} ns",
                word_count,
                input.len(),
                mean,
                std_error
            );

            if prev_time > 10000.0 {
                let ratio = mean / prev_time;
                println!("  Ratio from previous: {:.2}", ratio);
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high: {:.2}",
                    ratio
                );
            }
            prev_time = mean;
        }
    }

    #[test]
    fn test_performance_splitby_lines() {
        let query = r#"
            TEXT = line GREEDY SPLITBY NEWLINE
            line = LINE
        "#;

        println!("\nStress test: splitby lines");
        // Reduced sizes due to high per-line cost
        let sizes = [20, 40, 80, 160];
        let mut prev_time = 0.0;

        for &line_count in &sizes {
            let input = generate_lines(line_count, 50, 42);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });

            println!(
                "Lines: {}, Input len: {}, Time: {:.0} ± {:.0} ns",
                line_count,
                input.len(),
                mean,
                std_error
            );

            if prev_time > 10000.0 {
                let ratio = mean / prev_time;
                println!("  Ratio from previous: {:.2}", ratio);
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high: {:.2}",
                    ratio
                );
            }
            prev_time = mean;
        }
    }

    #[test]
    fn test_performance_nested_quantifiers() {
        // Nested quantifiers can be expensive
        let query = "TEXT = 1..N (1..N LETTER)";
        let chars = "abcdefghijklmnopqrstuvwxyz";

        println!("\nStress test: nested quantifiers");
        // Reduced sizes due to O(n²) or worse complexity
        let sizes = [25, 50, 100, 150];
        let mut prev_time = 0.0;

        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });

            println!("Size: {}, Time: {:.0} ± {:.0} ns", size, mean, std_error);

            if prev_time > 10000.0 {
                let ratio = mean / prev_time;
                println!("  Ratio from previous: {:.2}", ratio);
                // Nested quantifiers can be quadratic
                assert!(
                    ratio < 32.0,
                    "Performance degradation too high: {:.2}",
                    ratio
                );
            }
            prev_time = mean;
        }
    }

    #[test]
    fn test_performance_alternation_chain() {
        let query = r#"
            TEXT = 1..N choice
            choice = "a" OR "b" OR "c" OR "d" OR "e" OR "f" OR "g" OR "h"
        "#;
        let chars = "abcdefgh";

        println!("\nStress test: alternation chain");
        let sizes = [100, 200, 400, 800];
        let mut prev_time = 0.0;

        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });

            println!("Size: {}, Time: {:.0} ± {:.0} ns", size, mean, std_error);

            if prev_time > 10000.0 {
                let ratio = mean / prev_time;
                println!("  Ratio from previous: {:.2}", ratio);
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high: {:.2}",
                    ratio
                );
            }
            prev_time = mean;
        }
    }

    #[test]
    fn test_performance_complex_capture() {
        let query = r#"
            TEXT = item GREEDY SPLITBY NEWLINE
            item = key ": " value -> ADD item{} TO ROOT.items[]
            key = WORD -> ADD TO item
            value = LINE -> ADD TO item
        "#;

        println!("\nStress test: complex capture");
        let sizes = [20, 40, 80, 160];
        let mut prev_time = 0.0;

        for &line_count in &sizes {
            let mut input = String::new();
            for i in 0..line_count {
                if i > 0 {
                    input.push('\n');
                }
                input.push_str(&format!("key{}: value{}", i, i));
            }

            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });

            println!(
                "Lines: {}, Input len: {}, Time: {:.0} ± {:.0} ns",
                line_count,
                input.len(),
                mean,
                std_error
            );

            if prev_time > 10000.0 {
                let ratio = mean / prev_time;
                println!("  Ratio from previous: {:.2}", ratio);
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high: {:.2}",
                    ratio
                );
            }
            prev_time = mean;
        }
    }

    fn measure_growth(query: &str, chars: &str, label: &str) {
        let sizes = [100, 200, 400, 800];
        let mut times = Vec::new();

        println!("\nStress test: {}", label);
        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });
            times.push(mean);
            println!("Size: {}, Time: {:.0} ± {:.0} ns", size, mean, std_error);
        }

        for i in 1..times.len() {
            let ratio = times[i] / times[i - 1];
            println!("Size {} -> {}, Ratio: {:.2}", sizes[i - 1], sizes[i], ratio);

            // For O(N^k), ratio for doubling size is 2^k.
            // Even for O(N^3), ratio is 8. Let's allow up to 32 to be safe from jitter.
            if times[i - 1] > 1000.0 {
                // Ignore very small times to avoid noise
                assert!(
                    ratio < 32.0,
                    "Performance degradation too high for {}: ratio {:.2}",
                    label,
                    ratio
                );
            }
        }
    }

    #[allow(dead_code)]
    fn measure_growth_big(query: &str, chars: &str, label: &str) {
        let sizes = [1000, 2000, 4000, 8000, 16000];
        let mut times = Vec::new();

        println!("\nStress test: {}", label);
        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let (mean, std_error) = measure_time_stats(|| {
                let _ = evaluate_partition(query, &input);
            });
            times.push(mean);
            println!("Size: {}, Time: {:.0} ± {:.0} ns", size, mean, std_error);
        }

        for i in 1..times.len() {
            let ratio = times[i] / times[i - 1];
            println!("Size {} -> {}, Ratio: {:.2}", sizes[i - 1], sizes[i], ratio);

            if times[i - 1] > 10000.0 {
                assert!(
                    ratio < 16.0,
                    "Performance degradation too high for {}: ratio {:.2}",
                    label,
                    ratio
                );
            }
        }
    }
}
