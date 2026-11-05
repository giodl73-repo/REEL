//! Original repeat-aware source-phrase gate, separate from continuous ordering.
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use unicode_normalization::{UnicodeNormalization, char::canonical_combining_class};

pub(super) fn words(text: &str) -> Vec<String> {
    let normalized: String = text
        .to_lowercase()
        .nfkd()
        .filter(|c| canonical_combining_class(*c) == 0)
        .collect();
    normalized
        .split(|c: char| !c.is_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}
fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || matches!(c, '\u{001c}'..='\u{001f}'))
}
pub(super) fn lines(text: &str) -> Vec<&str> {
    text.split([
        '\n', '\r', '\u{000b}', '\u{000c}', '\u{001c}', '\u{001d}', '\u{001e}', '\u{0085}',
        '\u{2028}', '\u{2029}',
    ])
    .filter(|s| !trim(s).is_empty())
    .collect()
}

pub(super) fn render_words(
    text: &str,
    repetitions: u64,
    section: Option<usize>,
) -> Result<Vec<String>> {
    if !(1..=2).contains(&repetitions)
        || section.is_some_and(|n| !(2..=8).contains(&n) || repetitions != 2)
    {
        bail!("unsupported bounded lyric repetitions");
    }
    let Some(size) = section else {
        let tokens = words(text);
        return Ok(tokens
            .iter()
            .cycle()
            .take(tokens.len() * repetitions as usize)
            .cloned()
            .collect());
    };
    let mut result = Vec::new();
    let mut stanza = Vec::new();
    let flush = |stanza: &mut Vec<&str>, result: &mut Vec<String>| {
        for chunk in stanza.chunks(size) {
            let tokens = words(&chunk.join("\n"));
            result.extend(tokens.iter().cloned());
            result.extend(tokens);
        }
        stanza.clear();
    };
    for line in trim(text).split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.chars().all(|c| c == ' ' || c == '\t') {
            flush(&mut stanza, &mut result);
        } else {
            stanza.extend(lines(line).into_iter().map(trim));
        }
    }
    flush(&mut stanza, &mut result);
    Ok(result)
}

struct Token<'a> {
    word: &'a str,
    span: [f64; 2],
}
type Match = Vec<(usize, usize)>;
struct Budget {
    work: usize,
    candidates: usize,
    pairs: usize,
}
impl Budget {
    fn work(&mut self, amount: usize) -> Result<()> {
        self.work = self
            .work
            .checked_add(amount)
            .ok_or_else(|| anyhow::anyhow!("lyric work overflow"))?;
        if self.work > 1_000_000_000 {
            bail!("lyric work budget");
        }
        Ok(())
    }
    fn candidate(&mut self, size: usize) -> Result<()> {
        self.candidates += 1;
        self.pairs += size;
        if self.candidates > 100_000 || self.pairs > 1_000_000 {
            bail!("lyric candidate memory budget");
        }
        Ok(())
    }
}
fn candidates(
    expected: &[String],
    tokens: &[Token<'_>],
    budget: &mut Budget,
) -> Result<Vec<Match>> {
    if expected.is_empty() {
        return Ok(vec![]);
    }
    let mut positions: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, t) in tokens.iter().enumerate() {
        positions.entry(t.word).or_default().push(i);
    }
    let omissions =
        std::iter::once(None).chain((0..expected.len()).filter(|_| expected.len() > 1).map(Some));
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for omitted in omissions {
        let target: Vec<_> = expected
            .iter()
            .enumerate()
            .filter(|(i, _)| Some(*i) != omitted)
            .collect();
        let starts = positions
            .get(target[0].1.as_str())
            .map(Vec::as_slice)
            .unwrap_or_default();
        for &start in starts {
            let mut found = Vec::new();
            let mut cursor = start;
            for &(i, w) in &target {
                budget.work(1)?;
                let points = positions
                    .get(w.as_str())
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                let p = points.partition_point(|p| *p < cursor);
                if p == points.len() || points[p] - start + 1 > target.len() + 2 {
                    break;
                }
                found.push((i, points[p]));
                cursor = points[p] + 1;
            }
            if found.len() != target.len() {
                continue;
            }
            let first = found[0].1;
            let last = found[found.len() - 1].1;
            if tokens[last].span[1] - tokens[first].span[0]
                > (8usize.max(3 * expected.len())) as f64
            {
                continue;
            }
            if !seen.contains(&found) {
                budget.candidate(found.len())?;
                seen.insert(found.clone());
                result.push(found);
            }
        }
    }
    result.sort_by_key(|m| {
        (
            std::cmp::Reverse(m.len()),
            m[m.len() - 1].1 - m[0].1 + 1 - m.len(),
            m[0].1,
        )
    });
    Ok(result)
}

fn ordered(expected: &[String], tokens: &[Token<'_>], budget: &mut Budget) -> Result<Match> {
    let (n, m) = (expected.len(), tokens.len());
    budget.work(n * m)?;
    let width = m + 1;
    let mut grid = vec![0u16; (n + 1) * width];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            grid[i * width + j] = if expected[i] == tokens[j].word {
                1 + grid[(i + 1) * width + j + 1]
            } else {
                grid[(i + 1) * width + j].max(grid[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut found = Vec::new();
    while i < n && j < m {
        if expected[i] == tokens[j].word {
            found.push((i, j));
            i += 1;
            j += 1;
        } else if grid[(i + 1) * width + j] >= grid[i * width + j + 1] {
            i += 1
        } else {
            j += 1
        }
    }
    Ok(found)
}
fn row(
    number: usize,
    text: &str,
    expected: &[String],
    maps: &[Match],
    passes: &[Value],
    tokens: &[Vec<Token<'_>>],
) -> Value {
    let counts: Vec<_> = maps.iter().map(Vec::len).collect();
    let best = (0..maps.len())
        .max_by_key(|i| (counts[*i], std::cmp::Reverse(*i)))
        .unwrap();
    let missing: Vec<_> = expected
        .iter()
        .enumerate()
        .filter(|(i, _)| !maps[best].iter().any(|(n, _)| n == i))
        .map(|(_, w)| w)
        .collect();
    let starts = maps[best].iter().map(|(_, j)| tokens[best][*j].span[0]);
    let ends = maps[best].iter().map(|(_, j)| tokens[best][*j].span[1]);
    let span = if maps[best].is_empty() {
        Value::Null
    } else {
        json!([
            starts.fold(f64::INFINITY, f64::min),
            ends.fold(f64::NEG_INFINITY, f64::max)
        ])
    };
    json!({"line":number,"canonical":text,"total_words":expected.len(),"matched_words":counts,
        "best_pass":passes[best]["name"],"missing_words":missing,"recognized_span":span,
        "passes_gate":!expected.is_empty() && counts[best]>0 && missing.len()<=1})
}
fn summary(rows: Vec<Value>) -> Value {
    let qualifying = rows.iter().filter(|r| r["passes_gate"] == true).count();
    json!({"passed":!rows.is_empty() && qualifying==rows.len(),"total_lines":rows.len(),"qualifying_lines":qualifying,
        "exact_lines":rows.iter().filter(|r|r["missing_words"].as_array().unwrap().is_empty()).count(),
        "one_word_gap_lines":rows.iter().filter(|r|r["passes_gate"]==true && r["missing_words"].as_array().unwrap().len()==1).count(),
        "lines":rows,"human_listening":"pending","selected":false,"repeated_lines_allowed":true,"max_unrecognized_words_per_line":1})
}

pub(super) fn report(text: &str, passes: &[Value]) -> Result<Value> {
    if text.len() > 131_072 || passes.is_empty() || passes.len() > 16 {
        bail!("lyric source/pass budget")
    }
    let source = lines(text);
    let expected: Vec<_> = source.iter().map(|s| words(s)).collect();
    let all: Vec<_> = expected.iter().flatten().cloned().collect();
    if all.len() > 4000 {
        bail!("lyric source token budget")
    }
    let mut tokens = Vec::new();
    let mut names = BTreeSet::new();
    for pass in passes {
        let name = pass["name"]
            .as_str()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("lyric pass name"))?;
        if !names.insert(name) {
            bail!("duplicate lyric pass")
        }
        let values = pass["tokens"]
            .as_array()
            .ok_or_else(|| anyhow::anyhow!("lyric tokens"))?;
        if values.len() > 10_000 {
            bail!("lyric observed token budget")
        }
        let mut stream = Vec::new();
        for value in values {
            let word = value["word"]
                .as_str()
                .filter(|w| !w.is_empty() && w.bytes().all(|b| b.is_ascii_lowercase()))
                .ok_or_else(|| {
                    anyhow::anyhow!("original words must retain normalized independent tokens")
                })?;
            let span = value["span"]
                .as_array()
                .filter(|s| s.len() == 2)
                .ok_or_else(|| anyhow::anyhow!("lyric span"))?;
            let a = span[0]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("lyric span start"))?;
            let b = span[1]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("lyric span end"))?;
            if !a.is_finite() || !b.is_finite() || a < 0.0 || a > b || b > 600.0 {
                bail!("lyric span bounds")
            }
            stream.push(Token { word, span: [a, b] });
        }
        tokens.push(stream);
    }
    let mut budget = Budget {
        work: 0,
        candidates: 0,
        pairs: 0,
    };
    let mut presence = Vec::new();
    let mut order_maps = Vec::new();
    for stream in &tokens {
        budget.candidates = 0;
        budget.pairs = 0;
        let choices: Vec<_> = expected
            .iter()
            .map(|line| candidates(line, stream, &mut budget))
            .collect::<Result<_>>()?;
        let mut priority: Vec<_> = (0..source.len()).collect();
        priority.sort_by_key(|i| {
            (
                expected[*i].len() == 1,
                if choices[*i].is_empty() {
                    100_000
                } else {
                    choices[*i].len()
                },
                std::cmp::Reverse(expected[*i].len()),
            )
        });
        let mut assigned = vec![vec![]; source.len()];
        let mut used = BTreeSet::new();
        for i in priority {
            for candidate in &choices[i] {
                if candidate.iter().all(|(_, j)| !used.contains(j)) {
                    for (_, j) in candidate {
                        used.insert(*j);
                    }
                    assigned[i] = candidate.clone();
                    break;
                }
            }
        }
        presence.push(assigned);
        order_maps.push(ordered(&all, stream, &mut budget)?);
    }
    let mut rows = Vec::new();
    let mut ordered_rows = Vec::new();
    let mut offset = 0;
    for (i, line) in source.iter().enumerate() {
        rows.push(row(
            i + 1,
            line,
            &expected[i],
            &presence.iter().map(|m| m[i].clone()).collect::<Vec<_>>(),
            passes,
            &tokens,
        ));
        let maps: Vec<_> = order_maps
            .iter()
            .map(|m| {
                m.iter()
                    .filter(|(n, _)| *n >= offset && *n < offset + expected[i].len())
                    .map(|(n, j)| (n - offset, *j))
                    .collect()
            })
            .collect();
        ordered_rows.push(row(i + 1, line, &expected[i], &maps, passes, &tokens));
        offset += expected[i].len();
    }
    let ordered = summary(ordered_rows);
    let mut report = summary(rows);
    report["coverage_recipe"] = json!("repeat-aware-near-contiguous-phrases-v2");
    report["continuous_order_passed"] = ordered["passed"].clone();
    report["continuous_order_qualifying_lines"] = ordered["qualifying_lines"].clone();
    report["source_order_diagnostic"] = ordered;
    report["evidence"] = json!(
        "Original independent words on exact unchanged samples; recognition screening is not proof of singing or human approval"
    );
    Ok(report)
}

#[cfg(test)]
mod tests;
