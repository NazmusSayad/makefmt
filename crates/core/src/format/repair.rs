use super::{
    BLANKS, CONDITIONALS, KEYWORDS, Sep, assignment, continued, first_word, is_define, separator,
    starts_word, strip_continuation,
};

pub fn repair(source: &str) -> Option<String> {
    if source.contains(".RECIPEPREFIX") {
        return None;
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut groups = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        while i + 1 < lines.len() && continued(content(lines[i])) {
            i += 1;
        }
        i += 1;
        groups.push((start, i));
    }
    let mut fixes = Vec::new();
    let mut in_rule = false;
    let mut nest = 0;
    let mut depth = 0usize;
    for (g, &(start, end)) in groups.iter().enumerate() {
        let line0 = content(lines[start]);
        let first = text(line0);
        let word = first_word(first);
        if nest > 0 {
            if !line0.starts_with('\t') && starts_word(first, "define") {
                nest += 1;
            }
            if !line0.starts_with('\t') && starts_word(first, "endef") {
                nest -= 1;
                in_rule = false;
            }
            continue;
        }
        if line0.starts_with('\t') {
            if assignment(first).is_none() && is_define(first) {
                nest = 1;
            }
            continue;
        }
        if CONDITIONALS.contains(&word) {
            depth += 1;
        }
        if word == "endif" {
            depth = depth.saturating_sub(1);
        }
        if first.is_empty() || first.starts_with('#') || is_conditional(word) {
            continue;
        }
        let rule = is_rule(first);
        if in_rule
            && depth == 0
            && !matches!(separator(first), Some((Sep::Rule, ..)))
            && end - start == 1
            && line0.starts_with(' ')
            && !is_define(first)
            && word != "endef"
            && fatal_after(&lines, &groups[g + 1..])
        {
            fixes.push(start);
            continue;
        }
        in_rule = rule;
        if is_define(first) {
            nest = 1;
        }
    }
    if fixes.is_empty() {
        return None;
    }
    let mut out = String::with_capacity(source.len() + fixes.len());
    for (k, line) in lines.iter().enumerate() {
        if fixes.contains(&k) {
            out.push('\t');
            out.push_str(line.trim_start_matches(BLANKS));
        } else {
            out.push_str(line);
        }
    }
    Some(out)
}

fn fatal_after(lines: &[&str], groups: &[(usize, usize)]) -> bool {
    for &(start, _) in groups {
        let line0 = content(lines[start]);
        let first = text(line0);
        let word = first_word(first);
        if first.is_empty() || first.starts_with('#') {
            continue;
        }
        if is_conditional(word) || is_define(first) || word == "endef" {
            return false;
        }
        if assignment(first).is_some() || KEYWORDS.contains(&word) {
            continue;
        }
        return line0.starts_with('\t');
    }
    false
}

fn content(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

fn text(line: &str) -> &str {
    strip_continuation(line).trim_matches(BLANKS)
}

fn is_conditional(word: &str) -> bool {
    CONDITIONALS.contains(&word) || matches!(word, "else" | "endif")
}

fn is_rule(first: &str) -> bool {
    assignment(first).is_none()
        && !is_define(first)
        && !KEYWORDS.contains(&first_word(first))
        && matches!(separator(first), Some((Sep::Rule, _, end)) if !first[end..].contains('='))
}
