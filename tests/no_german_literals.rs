//! Sichtbare Texte stehen in `src/i18n/`, nicht im Code. Der Test findet deutsche Texte an
//! Umlauten und deutschen Anführungszeichen (Texte ohne diese fängt er nicht).

use std::path::Path;

const GERMAN: [char; 9] = ['ä', 'ö', 'ü', 'Ä', 'Ö', 'Ü', 'ß', '„', '“'];

/// String-Literale einer Zeile, ohne den Teil nach `//`.
fn literals(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let (mut in_str, mut escaped, mut current) = (false, false, String::new());
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
                out.push(std::mem::take(&mut current));
                i += 1;
                continue;
            }
            current.push(c);
        } else if c == '/' && chars.get(i + 1) == Some(&'/') {
            break;
        } else if c == '"' {
            in_str = true;
        }
        i += 1;
    }
    out
}

fn scan(dir: &Path, hits: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "i18n") {
                scan(&path, hits);
            }
            continue;
        }
        if path.extension().is_none_or(|e| e != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for (n, line) in text.lines().enumerate() {
            // Ab dem Testmodul zählt nichts mehr.
            if line.trim_start().starts_with("#[cfg(test)]") {
                break;
            }
            for lit in literals(line) {
                if lit.chars().any(|c| GERMAN.contains(&c)) {
                    hits.push(format!("{}:{}: {lit}", path.display(), n + 1));
                }
            }
        }
    }
}

#[test]
fn no_german_text_outside_the_tables() {
    let mut hits = Vec::new();
    scan(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut hits);
    assert!(hits.is_empty(), "{} deutsche Texte im Code:\n{}", hits.len(), hits.join("\n"));
}
