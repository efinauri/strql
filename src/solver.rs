use crate::ast::*;
use crate::error::{NamedSourceExt, StrqlError, StrqlResult};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::rc::Rc;

type PatternId = usize;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct Preference(Vec<i64>);

impl Preference {
    fn with_size(size: usize) -> Self {
        Preference(vec![0; size])
    }

    #[inline]
    fn add_at(&mut self, depth: usize, val: i64) {
        debug_assert!(
            depth < self.0.len(),
            "Preference::add_at: depth {} out of bounds (len {})",
            depth,
            self.0.len()
        );
        self.0[depth] += val;
    }

    #[inline]
    fn combine(&mut self, other: &Preference) {
        debug_assert!(
            self.0.len() == other.0.len(),
            "Preference::combine: size mismatch (self.len={}, other.len={})",
            self.0.len(),
            other.0.len()
        );
        for (i, &val) in other.0.iter().enumerate() {
            self.0[i] += val;
        }
    }
}

impl PartialOrd for Preference {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Preference {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare lexicographically
        let len = self.0.len().max(other.0.len());
        for i in 0..len {
            let v1 = self.0.get(i).unwrap_or(&0);
            let v2 = other.0.get(i).unwrap_or(&0);
            match v1.cmp(&v2) {
                std::cmp::Ordering::Equal => continue,
                ord => return ord,
            }
        }
        std::cmp::Ordering::Equal
    }
}

#[derive(Debug, Clone)]
struct Match {
    score: i64,
    preference: Preference,
    trace: MatchTrace,
}

#[derive(Debug, Clone)]
enum FlatPattern {
    Literal(String),
    Variable(PatternId),
    Builtin(Builtin),
    Sequence(Vec<PatternId>),
    Alternation(Vec<PatternId>),
    Quantifier {
        min: Bound,
        max: Bound,
        pattern: PatternId,
        mode: QuantifierBias,
    },
    AnyCase(PatternId),
    Upper(PatternId),
    Lower(PatternId),
    Group(PatternId),
}

struct FlatStatement {
    name: String,
    pattern: FlatPattern,
    capture: Option<CaptureClause>,
    depth: usize,
}

#[derive(Debug, Clone)]
enum MatchOutcome {
    Unique(Match),
    Ambiguous {
        best_score: i64,
        best_preference: Preference,
    },
}

#[derive(Debug, Clone)]
struct MatchMap {
    data: Vec<Option<MatchOutcome>>,
    active: Vec<usize>,
}

impl MatchMap {
    fn new(len: usize) -> Self {
        Self {
            data: vec![None; len + 1],
            active: Vec::new(),
        }
    }

    fn get(&self, pos: usize) -> Option<&MatchOutcome> {
        if pos < self.data.len() {
            self.data[pos].as_ref()
        } else {
            None
        }
    }

    fn iter(&self) -> impl Iterator<Item = (&usize, &MatchOutcome)> {
        // Invariant: all indices in active must have Some value in data
        #[cfg(debug_assertions)]
        for &idx in &self.active {
            debug_assert!(
                idx < self.data.len() && self.data[idx].is_some(),
                "MatchMap invariant violated: active index {} has no data",
                idx
            );
        }
        self.active
            .iter()
            .map(|i| (i, self.data[*i].as_ref().unwrap()))
    }
}

#[derive(Debug, Clone)]
enum VResult {
    NoMatch,
    Matches(Rc<MatchMap>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum CaseMode {
    #[default]
    Normal,
    AnyCase,
    Upper,
    Lower,
}

pub struct Solver<'a> {
    input: &'a str,

    indexed_statements: Vec<FlatStatement>,
    pattern_ids: HashMap<String, PatternId>,

    memo: Vec<VResult>,  // size: indexed_statements.len() * (input.len() + 1)
    memo_set: Vec<bool>, // tracking which memo entries are valid
    case_mode: CaseMode,

    max_preference_depth: usize,
}

impl VResult {
    fn single(
        next_pos: usize,
        score: i64,
        trace: MatchTrace,
        input_len: usize,
        max_preference_depth: usize,
    ) -> Self {
        debug_assert!(
            next_pos <= input_len,
            "VResult::single: next_pos {} exceeds input_len {}",
            next_pos,
            input_len
        );
        debug_assert!(
            max_preference_depth > 0,
            "VResult::single: max_preference_depth must be > 0"
        );
        let mut matches = MatchMap::new(input_len);
        matches.data[next_pos] = Some(MatchOutcome::Unique(Match {
            score,
            preference: Preference::with_size(max_preference_depth),
            trace,
        }));
        matches.active.push(next_pos);
        VResult::Matches(Rc::new(matches))
    }
}

impl<'a> NamedSourceExt<'a> for Solver<'a> {
    fn src(&self) -> &'a str {
        self.input
    }

    fn source_name(&self) -> &str {
        "input"
    }
}

impl<'a> Solver<'a> {
    fn merge_outcome(map: &mut MatchMap, next_pos: usize, new_outcome: MatchOutcome) {
        debug_assert!(
            next_pos < map.data.len(),
            "merge_outcome: next_pos {} out of bounds (data.len={})",
            next_pos,
            map.data.len()
        );
        if let Some(existing) = &mut map.data[next_pos] {
            let existing_score = match existing {
                MatchOutcome::Unique(m) => m.score,
                MatchOutcome::Ambiguous { best_score, .. } => *best_score,
            };
            let new_score = match &new_outcome {
                MatchOutcome::Unique(m) => m.score,
                MatchOutcome::Ambiguous { best_score, .. } => *best_score,
            };

            if new_score > existing_score {
                *existing = new_outcome;
                return;
            }

            if new_score < existing_score {
                return;
            }

            // Scores are equal, check preferences
            let existing_pref = match existing {
                MatchOutcome::Unique(m) => &m.preference,
                MatchOutcome::Ambiguous {
                    best_preference, ..
                } => best_preference,
            };
            let new_pref = match &new_outcome {
                MatchOutcome::Unique(m) => &m.preference,
                MatchOutcome::Ambiguous {
                    best_preference, ..
                } => best_preference,
            };

            if new_pref > existing_pref {
                *existing = new_outcome;
            } else if new_pref == existing_pref {
                let best_pref = match &new_outcome {
                    MatchOutcome::Unique(m) => m.preference.clone(),
                    MatchOutcome::Ambiguous {
                        best_preference, ..
                    } => best_preference.clone(),
                };
                *existing = MatchOutcome::Ambiguous {
                    best_score: new_score,
                    best_preference: best_pref,
                };
            }
        } else {
            map.data[next_pos] = Some(new_outcome);
            map.active.push(next_pos);
        }
    }

    pub fn new(program: &'a Program) -> StrqlResult<Self> {
        let mut name_to_id = HashMap::new();
        for (i, stmt) in program.statements.iter().enumerate() {
            name_to_id.insert(stmt.name.clone(), i);
        }

        let mut indexed_statements = Vec::new();
        for stmt in &program.statements {
            indexed_statements.push(FlatStatement {
                name: stmt.name.clone(),
                pattern: FlatPattern::Builtin(Builtin::AnyChar), // placeholder
                capture: stmt.capture.clone(),
                depth: 0,
            });
        }

        let mut solver = Self {
            input: "",
            indexed_statements,
            pattern_ids: name_to_id.clone(),
            memo: Vec::new(),
            memo_set: Vec::new(),
            case_mode: CaseMode::Normal,
            max_preference_depth: 0,
        };

        for (i, stmt) in program.statements.iter().enumerate() {
            let flat_id = solver.flatten_pattern(&stmt.pattern)?;
            solver.indexed_statements[i].pattern = FlatPattern::Variable(flat_id);
        }

        solver.compute_depths();
        Ok(solver)
    }

    fn flatten_pattern(&mut self, p: &Pattern) -> StrqlResult<PatternId> {
        let flat = match &p.node {
            PatternKind::Literal(s) => FlatPattern::Literal(s.clone()),
            PatternKind::Variable(name) => {
                return if let Some(&id) = self.pattern_ids.get(name) {
                    Ok(id)
                } else {
                    Err(StrqlError::UnboundVariable {
                        _name: name.clone(),
                        _src: self.src_to_named(),
                        _span: p.span.clone().into(),
                    })
                }
            }
            PatternKind::Builtin(b) => FlatPattern::Builtin(b.clone()),
            PatternKind::Sequence(seq) => {
                let ids = seq
                    .iter()
                    .map(|child| self.flatten_pattern(child))
                    .collect::<StrqlResult<Vec<_>>>()?;
                FlatPattern::Sequence(ids)
            }
            PatternKind::OrChain(alts) => {
                let ids = alts
                    .iter()
                    .map(|child| self.flatten_pattern(child))
                    .collect::<StrqlResult<Vec<_>>>()?;
                FlatPattern::Alternation(ids)
            }
            PatternKind::Repetition {
                min,
                max,
                pattern,
                bias: mode,
            } => {
                let id = self.flatten_pattern(pattern)?;
                FlatPattern::Quantifier {
                    min: min.clone(),
                    max: max.clone(),
                    pattern: id,
                    mode: *mode,
                }
            }
            PatternKind::AnyCase(inner) => {
                let id = self.flatten_pattern(inner)?;
                FlatPattern::AnyCase(id)
            }
            PatternKind::Upper(inner) => {
                let id = self.flatten_pattern(inner)?;
                FlatPattern::Upper(id)
            }
            PatternKind::Lower(inner) => {
                let id = self.flatten_pattern(inner)?;
                FlatPattern::Lower(id)
            }
            PatternKind::Group(inner) => {
                let id = self.flatten_pattern(inner)?;
                FlatPattern::Group(id)
            }
        };

        let id = self.indexed_statements.len();
        self.indexed_statements.push(FlatStatement {
            name: String::new(),
            pattern: flat,
            capture: None,
            depth: 0,
        });
        Ok(id)
    }

    fn compute_depths(&mut self) {
        let n = self.indexed_statements.len();
        for i in 0..n {
            self.indexed_statements[i].depth = usize::MAX;
        }

        if let Some(&root_id) = self.pattern_ids.get("TEXT") {
            let mut queue = std::collections::VecDeque::new();
            queue.push_back((root_id, 0));
            self.indexed_statements[root_id].depth = 0;

            while let Some((id, d)) = queue.pop_front() {
                let next_d = d + 1;
                let pattern = &self.indexed_statements[id].pattern;

                let mut children = Vec::new();
                match pattern {
                    FlatPattern::Variable(target) => children.push(*target),
                    FlatPattern::Sequence(ids) | FlatPattern::Alternation(ids) => {
                        for &child_id in ids {
                            children.push(child_id);
                        }
                    }
                    FlatPattern::Quantifier {
                        pattern: child_id, ..
                    } => {
                        children.push(*child_id);
                    }
                    FlatPattern::AnyCase(child_id)
                    | FlatPattern::Upper(child_id)
                    | FlatPattern::Lower(child_id)
                    | FlatPattern::Group(child_id) => {
                        children.push(*child_id);
                    }
                    _ => {}
                }

                for child_id in children {
                    if self.indexed_statements[child_id].depth > next_d {
                        self.indexed_statements[child_id].depth = next_d;
                        queue.push_back((child_id, next_d));
                    }
                }
            }
        }

        // Ensure all have some reasonable depth if unreachable
        let mut max_depth = 0;
        for i in 0..n {
            let depth = self.indexed_statements[i].depth;
            if depth == usize::MAX {
                self.indexed_statements[i].depth = 0;
            } else {
                max_depth = std::cmp::max(max_depth, depth);
            }
        }
        self.max_preference_depth = max_depth + 1;

        // Verify invariant: all depths must be < max_preference_depth
        #[cfg(debug_assertions)]
        for (i, stmt) in self.indexed_statements.iter().enumerate() {
            debug_assert!(
                stmt.depth < self.max_preference_depth,
                "compute_depths invariant violated: statement {} has depth {} >= max_preference_depth {}",
                i,
                stmt.depth,
                self.max_preference_depth
            );
        }
    }

    pub fn solve(&mut self, input: &'a str) -> StrqlResult<Value> {
        self.input = input;
        let size = self.indexed_statements.len() * (input.len() + 1);
        self.memo = vec![VResult::NoMatch; size];
        self.memo_set = vec![false; size];

        let text_id = if let Some(&id) = self.pattern_ids.get("TEXT") {
            id
        } else {
            return Err(StrqlError::NoTextStatement {
                _src: self.src_to_named(),
            });
        };

        match self.viterbi(text_id, 0)? {
            VResult::NoMatch => {
                let mut max_pos = 0;
                for res in &self.memo {
                    if let VResult::Matches(map) = res {
                        for &pos in &map.active {
                            if pos > max_pos {
                                max_pos = pos;
                            }
                        }
                    }
                }

                if max_pos > 0 {
                    Err(StrqlError::PartialMatch {
                        _matched: max_pos,
                        _total: input.len(),
                        _src: self.src_to_named(),
                        _span: (0..max_pos).into(),
                    })
                } else {
                    Err(StrqlError::PatternNoMatch {
                        _src: self.src_to_named(),
                    })
                }
            }

            VResult::Matches(matches) => match matches.get(input.len()) {
                Some(MatchOutcome::Unique(m)) => Ok(self.replay_captures(&m.trace)),
                Some(MatchOutcome::Ambiguous { .. }) => Err(StrqlError::AmbiguousParse {
                    _src: self.src_to_named(),
                }),
                None => {
                    let max_pos = matches.active.iter().max().cloned().unwrap_or(0);
                    Err(StrqlError::PartialMatch {
                        _matched: max_pos,
                        _total: input.len(),
                        _src: self.src_to_named(),
                        _span: (0..max_pos).into(),
                    })
                }
            },
        }
    }

    fn viterbi(&mut self, id: PatternId, pos: usize) -> StrqlResult<VResult> {
        debug_assert!(
            id < self.indexed_statements.len(),
            "viterbi: pattern id {} out of bounds (len {})",
            id,
            self.indexed_statements.len()
        );
        debug_assert!(
            pos <= self.input.len(),
            "viterbi: pos {} exceeds input length {}",
            pos,
            self.input.len()
        );

        let idx = id * (self.input.len() + 1) + pos;
        debug_assert!(
            idx < self.memo.len(),
            "viterbi: memo index {} out of bounds (len {})",
            idx,
            self.memo.len()
        );

        if self.memo_set[idx] {
            return Ok(self.memo[idx].clone());
        }

        let res = self.eval_pattern(id, pos)?;

        self.memo[idx] = res.clone();
        self.memo_set[idx] = true;
        Ok(res)
    }

    fn eval_pattern(&mut self, id: PatternId, pos: usize) -> StrqlResult<VResult> {
        let input_len = self.input.len();
        let pattern_type = self.indexed_statements[id].pattern.clone();
        let mut res = match &pattern_type {
            FlatPattern::Literal(s) => {
                let matched = match self.case_mode {
                    CaseMode::Normal => self.input[pos..].starts_with(s),
                    CaseMode::AnyCase => self.input[pos..]
                        .get(..s.len())
                        .map(|sub| sub.eq_ignore_ascii_case(s))
                        .unwrap_or(false),
                    CaseMode::Upper => self.input[pos..]
                        .get(..s.len())
                        .map(|sub| {
                            sub.eq_ignore_ascii_case(s) && !sub.chars().any(|c| c.is_lowercase())
                        })
                        .unwrap_or(false),
                    CaseMode::Lower => self.input[pos..]
                        .get(..s.len())
                        .map(|sub| {
                            sub.eq_ignore_ascii_case(s) && !sub.chars().any(|c| c.is_uppercase())
                        })
                        .unwrap_or(false),
                };

                if matched {
                    VResult::single(
                        pos + s.len(),
                        s.len() as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    )
                } else {
                    VResult::NoMatch
                }
            }

            FlatPattern::Variable(target_id) => self.viterbi(*target_id, pos)?,

            FlatPattern::Builtin(_) => self.eval_builtin(id, pos)?,

            FlatPattern::Group(inner_id) => self.viterbi(*inner_id, pos)?,

            FlatPattern::AnyCase(inner_id) => {
                let old = self.case_mode;
                self.case_mode = CaseMode::AnyCase;
                let res = self.viterbi(*inner_id, pos)?;
                self.case_mode = old;
                res
            }

            FlatPattern::Upper(inner_id) => {
                let old = self.case_mode;
                self.case_mode = CaseMode::Upper;
                let res = self.viterbi(*inner_id, pos)?;
                self.case_mode = old;
                res
            }

            FlatPattern::Lower(inner_id) => {
                let old = self.case_mode;
                self.case_mode = CaseMode::Lower;
                let res = self.viterbi(*inner_id, pos)?;
                self.case_mode = old;
                res
            }

            FlatPattern::Sequence(seq) => {
                let mut current_results = VResult::single(
                    pos,
                    0,
                    MatchTrace::default(),
                    input_len,
                    self.max_preference_depth,
                );

                for &p_id in seq {
                    let mut next_results_map = MatchMap::new(input_len);
                    if let VResult::Matches(matches) = current_results {
                        for (&cur_pos, outcome) in matches.iter() {
                            match outcome {
                                MatchOutcome::Unique(m) => {
                                    let res = self.viterbi(p_id, cur_pos)?;
                                    if let VResult::Matches(sub_matches) = res {
                                        for (&next_pos, sub_outcome) in sub_matches.iter() {
                                            let new_outcome = match sub_outcome {
                                                MatchOutcome::Unique(sm) => {
                                                    let mut new_trace = m.trace.clone();
                                                    new_trace.extend(sm.trace.clone());
                                                    let mut new_pref = m.preference.clone();
                                                    new_pref.combine(&sm.preference);
                                                    MatchOutcome::Unique(Match {
                                                        score: m.score + sm.score,
                                                        preference: new_pref,
                                                        trace: new_trace,
                                                    })
                                                }
                                                MatchOutcome::Ambiguous {
                                                    best_score: bs,
                                                    best_preference: bp,
                                                } => {
                                                    let mut new_pref = m.preference.clone();
                                                    new_pref.combine(bp);
                                                    MatchOutcome::Ambiguous {
                                                        best_score: m.score + bs,
                                                        best_preference: new_pref,
                                                    }
                                                }
                                            };
                                            Self::merge_outcome(
                                                &mut next_results_map,
                                                next_pos,
                                                new_outcome,
                                            );
                                        }
                                    }
                                }
                                MatchOutcome::Ambiguous {
                                    best_score: bs,
                                    best_preference: bp,
                                } => {
                                    let res = self.viterbi(p_id, cur_pos)?;
                                    if let VResult::Matches(sub_matches) = res {
                                        for (&next_pos, sub_outcome) in sub_matches.iter() {
                                            let (sub_score, sub_pref) = match sub_outcome {
                                                MatchOutcome::Unique(sm) => {
                                                    (sm.score, &sm.preference)
                                                }
                                                MatchOutcome::Ambiguous {
                                                    best_score: s,
                                                    best_preference: p,
                                                } => (*s, p),
                                            };
                                            let mut new_pref = bp.clone();
                                            new_pref.combine(sub_pref);
                                            Self::merge_outcome(
                                                &mut next_results_map,
                                                next_pos,
                                                MatchOutcome::Ambiguous {
                                                    best_score: bs + sub_score,
                                                    best_preference: new_pref,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if next_results_map.active.is_empty() {
                        current_results = VResult::NoMatch;
                        break;
                    }
                    current_results = VResult::Matches(Rc::new(next_results_map));
                }
                current_results
            }

            FlatPattern::Alternation(alts) => {
                let mut combined_map = MatchMap::new(input_len);
                for &p_id in alts {
                    let res = self.viterbi(p_id, pos)?;
                    if let VResult::Matches(matches) = res {
                        for (&next_pos, outcome) in matches.iter() {
                            Self::merge_outcome(&mut combined_map, next_pos, outcome.clone());
                        }
                    }
                }
                if combined_map.active.is_empty() {
                    VResult::NoMatch
                } else {
                    VResult::Matches(Rc::new(combined_map))
                }
            }

            FlatPattern::Quantifier {
                min,
                max,
                pattern: _,
                mode,
            } => {
                let min_val = if let Bound::Literal(n) = min { *n } else { 0 };
                let max_val = if let Bound::Literal(n) = max {
                    *n
                } else {
                    input_len - pos
                };
                self.eval_quantifier(id, min_val, max_val, *mode, pos)?
            }
        };

        // Track variable matches and captures
        if let VResult::Matches(matches_rc) = res {
            let stmt_name = self.indexed_statements[id].name.clone();
            let has_name = !stmt_name.is_empty();
            let has_capture = self.indexed_statements[id].capture.is_some();

            if has_name || has_capture {
                let mut matches = (*matches_rc).clone();
                for &next_pos in &matches.active {
                    let outcome = matches.data[next_pos].as_mut().unwrap();
                    let matched_text = &self.input[pos..next_pos];

                    match outcome {
                        MatchOutcome::Unique(m) => {
                            // Always track named variable matches for dynamic field resolution
                            if has_name {
                                m.trace.events.insert(
                                    0,
                                    TraceEvent::VariableMatch {
                                        name: stmt_name.clone(),
                                        value: matched_text.to_string(),
                                    },
                                );
                            }

                            // Add capture event if there's a capture clause
                            if let Some(ref capture_clause) = self.indexed_statements[id].capture {
                                let mut clause = capture_clause.clone();
                                let explicit_name = !clause.name.is_empty();
                                if clause.name.is_empty() {
                                    clause.name = stmt_name.clone();
                                }

                                m.trace.events.insert(
                                    0,
                                    TraceEvent::Capture {
                                        value: matched_text.to_string(),
                                        clause,
                                        explicit_name,
                                    },
                                );
                            }
                        }
                        MatchOutcome::Ambiguous { .. } => {
                            // Ambiguous matches don't have a trace to add events to
                        }
                    }
                }
                res = VResult::Matches(Rc::new(matches));
            } else {
                res = VResult::Matches(matches_rc);
            }
        }

        Ok(res)
    }

    fn eval_quantifier(
        &mut self,
        id: PatternId,
        min: usize,
        max: usize,
        mode: QuantifierBias,
        pos: usize,
    ) -> StrqlResult<VResult> {
        debug_assert!(
            min <= max,
            "eval_quantifier: min {} > max {}",
            min,
            max
        );
        debug_assert!(
            pos <= self.input.len(),
            "eval_quantifier: pos {} exceeds input length {}",
            pos,
            self.input.len()
        );

        let input_len = self.input.len();
        let sub_pattern_id = match &self.indexed_statements[id].pattern {
            FlatPattern::Quantifier { pattern, .. } => *pattern,
            _ => {
                return Err(StrqlError::Internal {
                    _message: "Indexed quantifier pattern does not index quantifier",
                })
            }
        };

        debug_assert!(
            sub_pattern_id < self.indexed_statements.len(),
            "eval_quantifier: sub_pattern_id {} out of bounds",
            sub_pattern_id
        );

        let mut results_by_k: Vec<VResult> = Vec::new();
        results_by_k.push(VResult::single(
            pos,
            0,
            MatchTrace::default(),
            input_len,
            self.max_preference_depth,
        ));

        for k in 1..=max {
            let mut next_results_map = MatchMap::new(input_len);
            if let VResult::Matches(prev_matches) = &results_by_k[k - 1] {
                for (&cur_pos, outcome) in prev_matches.iter() {
                    let res = self.viterbi(sub_pattern_id, cur_pos)?;
                    if let VResult::Matches(sub_matches) = res {
                        for (&next_pos, sub_outcome) in sub_matches.iter() {
                            let new_outcome = match outcome {
                                MatchOutcome::Unique(m) => match sub_outcome {
                                    MatchOutcome::Unique(sm) => {
                                        let mut new_trace = m.trace.clone();
                                        new_trace.extend(sm.trace.clone());
                                        let mut new_pref = m.preference.clone();
                                        new_pref.combine(&sm.preference);
                                        MatchOutcome::Unique(Match {
                                            score: m.score + sm.score,
                                            preference: new_pref,
                                            trace: new_trace,
                                        })
                                    }
                                    MatchOutcome::Ambiguous {
                                        best_score: bs,
                                        best_preference: bp,
                                    } => {
                                        let mut new_pref = m.preference.clone();
                                        new_pref.combine(bp);
                                        MatchOutcome::Ambiguous {
                                            best_score: m.score + bs,
                                            best_preference: new_pref,
                                        }
                                    }
                                },
                                MatchOutcome::Ambiguous {
                                    best_score: bs,
                                    best_preference: bp,
                                } => {
                                    let (sub_score, sub_pref) = match sub_outcome {
                                        MatchOutcome::Unique(sm) => (sm.score, &sm.preference),
                                        MatchOutcome::Ambiguous {
                                            best_score: s,
                                            best_preference: p,
                                        } => (*s, p),
                                    };
                                    let mut new_pref = bp.clone();
                                    new_pref.combine(sub_pref);
                                    MatchOutcome::Ambiguous {
                                        best_score: bs + sub_score,
                                        best_preference: new_pref,
                                    }
                                }
                            };
                            Self::merge_outcome(&mut next_results_map, next_pos, new_outcome);
                        }
                    }
                }
            }
            if next_results_map.active.is_empty() {
                break;
            }
            results_by_k.push(VResult::Matches(Rc::new(next_results_map)));
        }

        // Collect results for k in min..=max
        let mut pos_to_k_outcomes: HashMap<usize, Vec<(usize, MatchOutcome)>> = HashMap::new();
        for k in min..results_by_k.len() {
            if let VResult::Matches(matches) = &results_by_k[k] {
                for (&next_pos, outcome) in matches.iter() {
                    pos_to_k_outcomes
                        .entry(next_pos)
                        .or_default()
                        .push((k, outcome.clone()));
                }
            }
        }

        let mut final_map = MatchMap::new(input_len);
        for (next_pos, k_outcomes) in pos_to_k_outcomes {
            let mut best_k_outcomes: Vec<(usize, MatchOutcome)> = Vec::new();

            for (k, mut outcome) in k_outcomes {
                // Apply local preference for this k
                let k_pref = match mode {
                    QuantifierBias::Greedy => k as i64,
                    QuantifierBias::Lazy => -(k as i64),
                    QuantifierBias::Neutral => 0,
                };
                let depth = self.indexed_statements[id].depth;
                match &mut outcome {
                    MatchOutcome::Unique(m) => m.preference.add_at(depth, k_pref),
                    MatchOutcome::Ambiguous {
                        best_preference, ..
                    } => best_preference.add_at(depth, k_pref),
                }

                if best_k_outcomes.is_empty() {
                    best_k_outcomes.push((k, outcome));
                } else {
                    let (_, existing_outcome) = &best_k_outcomes[0];
                    let existing_pref = match existing_outcome {
                        MatchOutcome::Unique(m) => &m.preference,
                        MatchOutcome::Ambiguous {
                            best_preference, ..
                        } => best_preference,
                    };
                    let new_pref = match &outcome {
                        MatchOutcome::Unique(m) => &m.preference,
                        MatchOutcome::Ambiguous {
                            best_preference, ..
                        } => best_preference,
                    };

                    if new_pref > existing_pref {
                        best_k_outcomes.clear();
                        best_k_outcomes.push((k, outcome));
                    } else if new_pref == existing_pref {
                        best_k_outcomes.push((k, outcome));
                    }
                }
            }

            if best_k_outcomes.len() > 1 {
                let score = match &best_k_outcomes[0].1 {
                    MatchOutcome::Unique(m) => m.score,
                    MatchOutcome::Ambiguous { best_score, .. } => *best_score,
                };
                let pref = match &best_k_outcomes[0].1 {
                    MatchOutcome::Unique(m) => m.preference.clone(),
                    MatchOutcome::Ambiguous {
                        best_preference, ..
                    } => best_preference.clone(),
                };
                final_map.data[next_pos] = Some(MatchOutcome::Ambiguous {
                    best_score: score,
                    best_preference: pref,
                });
                final_map.active.push(next_pos);
            } else if !best_k_outcomes.is_empty() {
                final_map.data[next_pos] = Some(best_k_outcomes.remove(0).1);
                final_map.active.push(next_pos);
            }
        }

        if final_map.active.is_empty() {
            Ok(VResult::NoMatch)
        } else {
            Ok(VResult::Matches(Rc::new(final_map)))
        }
    }

    fn eval_builtin(&self, id: PatternId, pos: usize) -> StrqlResult<VResult> {
        let input_len = self.input.len();
        let b = match &self.indexed_statements[id].pattern {
            FlatPattern::Builtin(b) => b,
            _ => {
                return Err(StrqlError::Internal {
                    _message: "indexed builtin pattern does not index builtin",
                })
            }
        };
        let input = self.input;
        let rest = &input[pos..];

        if rest.is_empty() {
            return Ok(VResult::NoMatch);
        }

        match b {
            Builtin::Letter => {
                let ch = rest.chars().next().unwrap();
                let matched = match self.case_mode {
                    CaseMode::Normal | CaseMode::AnyCase => ch.is_ascii_alphabetic(),
                    CaseMode::Upper => ch.is_ascii_uppercase(),
                    CaseMode::Lower => ch.is_ascii_lowercase(),
                };
                if matched {
                    let len = ch.len_utf8();
                    Ok(VResult::single(
                        pos + len,
                        len as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }

            Builtin::Digit => {
                let ch = rest.chars().next().unwrap();
                if ch.is_ascii_digit() {
                    let len = ch.len_utf8();
                    Ok(VResult::single(
                        pos + len,
                        len as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }

            Builtin::Space => {
                let ch = rest.chars().next().unwrap();
                if ch.is_whitespace() && ch != '\n' {
                    let len = ch.len_utf8();
                    Ok(VResult::single(
                        pos + len,
                        len as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }

            Builtin::Newline => {
                if rest.starts_with('\n') {
                    Ok(VResult::single(
                        pos + 1,
                        1,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }

            Builtin::AnyChar => {
                let ch = rest.chars().next().unwrap();
                let ok = match self.case_mode {
                    CaseMode::Normal | CaseMode::AnyCase => true,
                    CaseMode::Upper => !ch.is_ascii_lowercase(),
                    CaseMode::Lower => !ch.is_ascii_uppercase(),
                };
                if ok {
                    let len = ch.len_utf8();
                    Ok(VResult::single(
                        pos + len,
                        len as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }

            Builtin::Line => {
                let mut end = pos;
                let mut has_lowercase = false;
                let mut has_uppercase = false;
                for ch in rest.chars() {
                    if ch == '\n' {
                        break;
                    }
                    if ch.is_ascii_lowercase() {
                        has_lowercase = true;
                    }
                    if ch.is_ascii_uppercase() {
                        has_uppercase = true;
                    }
                    end += ch.len_utf8();
                }

                let ok = match self.case_mode {
                    CaseMode::Normal | CaseMode::AnyCase => true,
                    CaseMode::Upper => !has_lowercase,
                    CaseMode::Lower => !has_uppercase,
                };

                if ok {
                    Ok(VResult::single(
                        end,
                        (end - pos) as i64,
                        MatchTrace::default(),
                        input_len,
                        self.max_preference_depth,
                    ))
                } else {
                    Ok(VResult::NoMatch)
                }
            }
        }
    }

    // ---------------- CAPTURE REPLAY ----------------

    fn replay_captures(&self, trace: &MatchTrace) -> Value {
        let mut root = json!({});
        let mut named_paths: HashMap<String, Vec<ResolvedSegment>> = HashMap::new();
        let mut captured_values: HashMap<String, String> = HashMap::new();

        for event in &trace.events {
            match event {
                TraceEvent::VariableMatch { name, value } => {
                    // Track variable matches for dynamic field resolution
                    captured_values.insert(name.clone(), value.clone());
                }
                TraceEvent::Capture {
                    value,
                    clause,
                    explicit_name,
                } => {
                    // Store the captured value first so it's available for dynamic fields
                    if !clause.name.is_empty() {
                        captured_values.insert(clause.name.clone(), value.to_string());
                    }
                    self.apply_capture(
                        &mut root,
                        &mut named_paths,
                        &captured_values,
                        value,
                        clause,
                        *explicit_name,
                    );
                }
            }
        }

        root
    }

    fn apply_capture(
        &self,
        root: &mut Value,
        named_paths: &mut HashMap<String, Vec<ResolvedSegment>>,
        captured_values: &HashMap<String, String>,
        value: &str,
        clause: &CaptureClause,
        _explicit_name: bool,
    ) {
        let mut segments = Vec::new();
        let mut i = 0;

        // 1. Resolve starting point
        if let Some(PathSegment::Root) = clause.path.segments.get(0) {
            segments.push(ResolvedSegment::Root);
            i = 1;
        } else if let Some(PathSegment::Field(name)) = clause.path.segments.get(0) {
            if let Some(path) = named_paths.get(name) {
                segments.extend(path.clone());
                i = 1;
            } else {
                segments.push(ResolvedSegment::Root);
            }
        } else {
            segments.push(ResolvedSegment::Root);
        }

        // 2. Resolve remaining segments
        for segment in &clause.path.segments[i..] {
            match segment {
                PathSegment::Root => {}
                PathSegment::Field(name) => segments.push(ResolvedSegment::Field(name.clone())),
                PathSegment::DynamicField(var) => {
                    let name = captured_values.get(var).cloned().unwrap_or_default();
                    segments.push(ResolvedSegment::Field(name));
                }
                PathSegment::ArrayAppend => {}
            }
        }

        let is_array_append = clause.path.ends_with_array();
        let val_to_insert = if clause.is_object {
            json!({})
        } else {
            Value::String(value.to_string())
        };

        let mut current = root;
        let mut current_path = Vec::new();

        // 3. Navigate/Create path
        for (idx, seg) in segments.iter().enumerate() {
            let is_last = !is_array_append && idx == segments.len() - 1;

            if is_last {
                // For the last segment, we need to actually insert the value/object
                let field_name = match seg {
                    ResolvedSegment::Root => {
                        // When path ends at Root, add as a field to root
                        clause.name.clone()
                    }
                    ResolvedSegment::Field(name) => {
                        // When path ends with a field name, that's the field to set
                        name.clone()
                    }
                    ResolvedSegment::Index(_) => {
                        // When path ends at an index, we'll handle below
                        String::new()
                    }
                };

                match seg {
                    ResolvedSegment::Root | ResolvedSegment::Field(_) => {
                        if !current.is_object() {
                            *current = json!({});
                        }

                        if clause.is_object {
                            // Creating a named empty object at this field
                            current
                                .as_object_mut()
                                .unwrap()
                                .entry(field_name.clone())
                                .or_insert_with(|| json!({}));
                        } else {
                            // Adding a value to this field
                            current
                                .as_object_mut()
                                .unwrap()
                                .insert(field_name.clone(), val_to_insert.clone());
                        }

                        if matches!(seg, ResolvedSegment::Root) {
                            current_path.push(ResolvedSegment::Root);
                        } else if let ResolvedSegment::Field(name) = seg {
                            current_path.push(ResolvedSegment::Field(name.clone()));
                        }
                    }
                    ResolvedSegment::Index(idx) => {
                        if !current.is_array() {
                            *current = json!([]);
                        }
                        let arr = current.as_array_mut().unwrap();
                        if *idx >= arr.len() {
                            arr.resize(*idx + 1, json!({}));
                        }

                        let target = &mut arr[*idx];
                        if clause.is_object {
                            if !target.is_object() {
                                *target = json!({});
                            }
                        } else {
                            // Adding a value - should add as a field to the object
                            if !target.is_object() {
                                *target = json!({});
                            }
                            target
                                .as_object_mut()
                                .unwrap()
                                .insert(clause.name.clone(), val_to_insert.clone());
                        }
                        current_path.push(ResolvedSegment::Index(*idx));
                    }
                }
                break;
            } else {
                match seg {
                    ResolvedSegment::Root => {
                        current_path.push(ResolvedSegment::Root);
                    }
                    ResolvedSegment::Field(name) => {
                        if !current.is_object() {
                            *current = json!({});
                        }
                        let next_is_index = if idx + 1 < segments.len() {
                            matches!(segments[idx + 1], ResolvedSegment::Index(_))
                        } else {
                            is_array_append
                        };
                        current = current
                            .as_object_mut()
                            .unwrap()
                            .entry(name.clone())
                            .or_insert_with(|| if next_is_index { json!([]) } else { json!({}) });
                        current_path.push(ResolvedSegment::Field(name.clone()));
                    }
                    ResolvedSegment::Index(idx) => {
                        if !current.is_array() {
                            *current = json!([]);
                        }
                        let arr = current.as_array_mut().unwrap();
                        if *idx >= arr.len() {
                            arr.resize(*idx + 1, json!({}));
                        }
                        current = &mut arr[*idx];
                        current_path.push(ResolvedSegment::Index(*idx));
                    }
                }
            }
        }

        if is_array_append {
            if !clause.is_object && value.is_empty() {
                return;
            }

            if !current.is_array() {
                *current = json!([]);
            }
            let arr = current.as_array_mut().unwrap();
            arr.push(val_to_insert);
            current_path.push(ResolvedSegment::Index(arr.len() - 1));
        }

        if !clause.name.is_empty() {
            named_paths.insert(clause.name.clone(), current_path);
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ResolvedSegment {
    Root,
    Field(String),
    Index(usize),
}

#[derive(Clone, Default, PartialEq, Debug)]
struct MatchTrace {
    events: Vec<TraceEvent>,
}

impl MatchTrace {
    fn extend(&mut self, other: MatchTrace) {
        self.events.extend(other.events);
    }
}

#[derive(Clone, PartialEq, Debug)]
enum TraceEvent {
    Capture {
        value: String,
        clause: CaptureClause,
        explicit_name: bool,
    },
    VariableMatch {
        name: String,
        value: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse;
    use serde_json::json;

    #[test]
    fn simple_unique_capture() {
        let program = parse(
            r#"
            TEXT = WORD -> ADD TO ROOT.result
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("hello");

        assert!(res.is_ok());
        assert_eq!(res.unwrap(), json!({ "result": "hello" }));
    }

    #[test]
    fn splitby_array_capture() {
        let program = parse(
            r#"
            TEXT = w GREEDY SPLITBY ", "
            w = WORD -> ADD TO ROOT.items[]
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("a, b, c");

        assert!(res.is_ok());
        assert_eq!(
            res.unwrap(),
            json!({
                "items": ["a", "b", "c"]
            })
        );
    }

    #[test]
    fn nested_statements_capture() {
        let program = parse(
            r#"
            TEXT = l GREEDY SPLITBY NEWLINE
            l = WORD " is " WORD -> ADD item{} TO ROOT.results[]
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("cats is animals\ndogs is pets");

        assert!(res.is_ok());

        let out = res.unwrap();
        let arr = out["results"].as_array().unwrap();
        assert_eq!(arr.len(), 2);
    }

    #[test]
    fn ambiguous_splitby_any() {
        let program = parse(
            r#"
            TEXT = w SPLITBY "."
            w = ANY -> ADD TO ROOT.items[]
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("a. b. c.");

        assert!(res.is_err());
        assert!(matches!(
            res.unwrap_err(),
            StrqlError::AmbiguousParse { .. }
        ));
    }

    #[test]
    fn no_match() {
        let program = parse(
            r#"
            TEXT = DIGIT
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("abc");

        assert!(res.is_err());
        assert!(matches!(
            res.unwrap_err(),
            StrqlError::PatternNoMatch { .. }
        ));
    }

    #[test]
    fn optional_quantifier_capture() {
        let program = parse(
            r#"
            TEXT = w 0..1 "!"
            w = 1..N LETTER -> ADD TO ROOT
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();

        let res1 = solver.solve("hello");
        assert!(res1.is_ok());
        assert_eq!(res1.unwrap(), json!({"w": "hello"}));

        let res2 = solver.solve("hello!");
        assert!(res2.is_ok());
        assert_eq!(res2.unwrap(), json!({"w": "hello"}));
    }

    #[test]
    fn partial_match_error() {
        let program = parse(
            r#"
            TEXT = "ABC" "DEF"
        "#,
        )
        .unwrap();

        let mut solver = Solver::new(&program).unwrap();
        let res = solver.solve("ABCXYZ");

        assert!(res.is_err());
        match res.unwrap_err() {
            StrqlError::PartialMatch {
                _matched, _total, ..
            } => {
                assert_eq!(_matched, 3);
                assert_eq!(_total, 6);
            }
            e => panic!("Expected PartialMatch, got {:?}", e),
        }
    }
}
