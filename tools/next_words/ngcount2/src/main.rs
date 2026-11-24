// 2026-10-19 Claude: count words and adjacent word pairs from text on stdin (ngcount of 2026-10-18 plus two things).
// A pair is counted only when the two words are separated by spaces alone
// (punctuation, digits and line breaks end the run). Words are lowercased.
// New: LANG "tr" lowercases I -> ı and İ -> i (Turkish), and the written forms with capitals are counted where
// a word is not the first of its run (so sentence-initial capitals do not count), to restore Vater, I, English.
// Usage: ngcount2 OUT_PREFIX MIN_PAIR_COUNT [LANG] < text
// Output: OUT.1gram.tsv (word, count >= 2), OUT.2gram.tsv (w1, w2, count >= MIN), OUT.case.tsv (word, non-initial count, form, count)
use rustc_hash::FxHashMap;
use std::io::{self, BufRead, BufWriter, Write};

fn lower_into(word: &mut String, c: char, turkish: bool) {
    if turkish && c == 'I' {
        word.push('ı');
    } else if turkish && c == 'İ' {
        word.push('i');
    } else {
        word.extend(c.to_lowercase());
    }
}

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let prefix = &args[1];
    let min_pair: u32 = args[2].parse().unwrap();
    let turkish = args.get(3).map_or(false, |l| l == "tr");
    let mut ids: FxHashMap<String, u32> = FxHashMap::default();
    let mut words: Vec<String> = Vec::new();
    let mut unigrams: Vec<u64> = Vec::new();
    let mut inner: Vec<u64> = Vec::new(); // occurrences that are not the first word of a run
    let mut forms: FxHashMap<(u32, String), u32> = FxHashMap::default();
    let mut pairs: FxHashMap<u64, u32> = FxHashMap::default();
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();
    let mut word = String::new();
    let mut written = String::new();
    let mut tokens: u64 = 0;
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 { break; }
        let mut prev: Option<u32> = None;
        let mut gap_clean = true; // only spaces since the previous word
        let mut chars = line.chars().peekable();
        while let Some(c) = chars.next() {
            if c.is_alphabetic() {
                word.clear();
                written.clear();
                lower_into(&mut word, c, turkish);
                written.push(c);
                while let Some(&n) = chars.peek() {
                    if n.is_alphabetic() {
                        lower_into(&mut word, n, turkish);
                        written.push(n);
                        chars.next();
                    } else if n == '\'' || n == '\u{2019}' {
                        // keep an apostrophe only between letters (don't, it's, l'homme)
                        let mut look = chars.clone();
                        look.next();
                        if look.peek().map_or(false, |x| x.is_alphabetic()) {
                            word.push('\'');
                            written.push('\'');
                            chars.next();
                        } else {
                            break;
                        }
                    } else {
                        break;
                    }
                }
                let id = match ids.get(word.as_str()) {
                    Some(&id) => id,
                    None => {
                        let id = words.len() as u32;
                        ids.insert(word.clone(), id);
                        words.push(word.clone());
                        unigrams.push(0);
                        inner.push(0);
                        id
                    }
                };
                unigrams[id as usize] += 1;
                tokens += 1;
                let run_inner = prev.is_some() && gap_clean;
                if let (Some(p), true) = (prev, gap_clean) {
                    *pairs.entry(((p as u64) << 32) | id as u64).or_insert(0) += 1;
                }
                if run_inner {
                    inner[id as usize] += 1;
                    if written.chars().any(|x| x.is_uppercase()) {
                        *forms.entry((id, written.clone())).or_insert(0) += 1;
                    }
                }
                prev = Some(id);
                gap_clean = true;
            } else if c == ' ' || c == '\t' {
                // spaces keep the run
            } else {
                gap_clean = false;
            }
        }
    }
    let mut out = BufWriter::new(std::fs::File::create(format!("{prefix}.1gram.tsv"))?);
    for (i, w) in words.iter().enumerate() {
        if unigrams[i] >= 2 { writeln!(out, "{}\t{}", w, unigrams[i])?; }
    }
    let mut out = BufWriter::new(std::fs::File::create(format!("{prefix}.2gram.tsv"))?);
    let mut kept = 0u64;
    for (k, c) in pairs.iter() {
        if *c >= min_pair {
            writeln!(out, "{}\t{}\t{}", words[(k >> 32) as usize], words[(k & 0xffff_ffff) as usize], c)?;
            kept += 1;
        }
    }
    let mut out = BufWriter::new(std::fs::File::create(format!("{prefix}.case.tsv"))?);
    for ((id, form), c) in forms.iter() {
        if *c >= 2 {
            writeln!(out, "{}\t{}\t{}\t{}", words[*id as usize], inner[*id as usize], form, c)?;
        }
    }
    eprintln!("tokens={tokens} types={} pairs={} kept={kept} forms={}", words.len(), pairs.len(), forms.len());
    Ok(())
}
