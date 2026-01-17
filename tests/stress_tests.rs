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
        String::from(res)
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

    fn measure_growth(query: &str, chars: &str, label: &str) {
        let sizes = [100, 200, 400, 800];
        let mut times = Vec::new();

        for &size in &sizes {
            let input = generate_random_input(size, 42, chars);
            let start = Instant::now();
            let _ = evaluate_partition(query, &input);
            let elapsed = start.elapsed().as_nanos();
            times.push(elapsed);
        }

        println!("\nStress test: {}", label);
        for i in 1..times.len() {
            let ratio = (times[i] as f64) / (times[i - 1] as f64);
            println!("Size {} -> {}, Ratio: {:.2}", sizes[i - 1], sizes[i], ratio);

            // For O(N^k), ratio for doubling size is 2^k.
            // Even for O(N^3), ratio is 8. Let's allow up to 32 to be safe from jitter.
            if times[i - 1] > 1000 {
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
}
