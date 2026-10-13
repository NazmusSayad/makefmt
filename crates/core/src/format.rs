mod recipe;

use crate::Options;

const BLANKS: [char; 2] = [' ', '\t'];
const WORD_ENDS: [char; 3] = [' ', '\t', '#'];
const CONDITIONALS: [&str; 4] = ["ifeq", "ifneq", "ifdef", "ifndef"];
const MODIFIERS: [&str; 4] = ["override", "export", "unexport", "private"];
const KEYWORDS: [&str; 11] = [
    "include", "-include", "sinclude", "vpath", "export", "unexport", "undefine", "override",
    "private", "load", "-load",
];

enum Item<'a> {
    Blank,
    Raw(&'a str),
    Recipe(String),
    Text {
        depth: usize,
        text: String,
        tail: &'a [&'a str],
        keep: bool,
    },
    Assign {
        depth: usize,
        name: String,
        op: &'a str,
        value: &'a str,
        tail: &'a [&'a str],
    },
}

enum Sep {
    Assign,
    Rule,
}

struct Assignment<'a> {
    name: String,
    op: &'a str,
    value: &'a str,
}

pub fn format(source: &str, options: &Options) -> String {
    match source.strip_prefix('\u{FEFF}') {
        Some(rest) => format!("\u{FEFF}{}", format_source(rest, options)),
        None => format_source(source, options),
    }
}

fn format_source(source: &str, options: &Options) -> String {
    if source.replace("\r\n", "\n").contains('\r') {
        return source.to_string();
    }
    let eol = match source.find('\n') {
        Some(i) if source[..i].ends_with('\r') => "\r\n",
        _ => "\n",
    };
    let oneshell = source.contains(".ONESHELL");
    let posix = source.contains(".POSIX");
    let lang = match options.format_recipes && !oneshell {
        true => recipe::shell_lang(source),
        false => None,
    };
    let lines: Vec<&str> = source.lines().collect();
    let mut items = Vec::with_capacity(lines.len());
    let mut prefix = '\t';
    let mut depth = 0;
    let mut nest = 0;
    let mut hidden_conditional = false;
    let mut in_rule = false;
    let mut i = 0;
    while i < lines.len() {
        let start = i;
        while i + 1 < lines.len() && continued(lines[i]) {
            i += 1;
        }
        i += 1;
        let group = &lines[start..i];
        let line0 = group[0];
        let recipe = line0.starts_with(prefix);
        let keep = !options.trim_trailing_whitespace && group.iter().any(|line| line.contains('='));
        let body = if recipe {
            &line0[prefix.len_utf8()..]
        } else {
            line0
        };
        let head = strip_continuation(body).trim_start_matches(BLANKS);
        let first = if keep && !continued(line0) {
            head
        } else {
            trim_trailing(head)
        };

        if (recipe || nest > 0) && CONDITIONALS.contains(&first_word(first)) {
            hidden_conditional = true;
        }
        if group.iter().any(|line| line.contains(".RECIPEPREFIX"))
            && (recipe || nest > 0 || !first.starts_with('#'))
        {
            if recipe || nest > 0 || depth > 0 || hidden_conditional || group.len() > 1 {
                return source.to_string();
            }
            let Some(next) = static_prefix(first) else {
                return source.to_string();
            };
            prefix = next;
        }

        if nest > 0 {
            if !recipe && starts_word(first, "define") {
                nest += 1;
            }
            if !recipe && starts_word(first, "endef") {
                nest -= 1;
            }
            if nest > 0 {
                items.extend(group.iter().map(|line| Item::Raw(line)));
                continue;
            }
            in_rule = false;
            let bail = needs_raw(group, first, prefix, false, posix);
            push_text(&mut items, depth, collapse_words(first), group, keep, bail);
            continue;
        }

        if !recipe && line0.trim_matches(BLANKS).is_empty() {
            items.push(Item::Blank);
            continue;
        }

        let assign = assignment(first);
        let word = first_word(first);

        if recipe {
            if in_rule
                && let Some(lang) = lang
                && !is_define(first)
                && !CONDITIONALS.contains(&word)
                && !matches!(word, "else" | "endif")
                && let Some(lines) = recipe::format(group, prefix, lang, options.indent_width)
            {
                items.extend(lines.into_iter().map(Item::Recipe));
                continue;
            }
            let keep = keep || oneshell;
            for (k, line) in group.iter().enumerate() {
                let text = if keep { line } else { trim_trailing(line) };
                if k == 0 && text.is_empty() {
                    items.push(Item::Raw(&line[..prefix.len_utf8()]));
                } else {
                    items.push(Item::Raw(text));
                }
            }
            if assign.is_none() && is_define(first) {
                nest = 1;
            }
            continue;
        }

        if !first.starts_with('#')
            && !CONDITIONALS.contains(&word)
            && !matches!(word, "else" | "endif")
        {
            in_rule = assign.is_none()
                && !is_define(first)
                && !KEYWORDS.contains(&word)
                && matches!(separator(first), Some((Sep::Rule, _, end)) if !first[end..].contains('='));
        }

        let bail = needs_raw(group, first, prefix, assign.is_some(), posix);

        if let Some(assign) = assign {
            if bail {
                push_raw(&mut items, group, keep);
            } else {
                items.push(Item::Assign {
                    depth,
                    name: assign.name,
                    op: assign.op,
                    value: assign.value,
                    tail: &group[1..],
                });
            }
            continue;
        }

        let mut line_depth = depth;
        let text = if is_define(first) {
            nest = 1;
            collapse_words(first)
        } else if CONDITIONALS.contains(&word) || word == "else" || word == "endif" {
            if word == "endif" {
                depth = depth.saturating_sub(1);
                line_depth = depth;
            } else if word == "else" {
                line_depth = depth.saturating_sub(1);
            } else {
                depth += 1;
            }
            directive(first)
        } else if first.starts_with('#')
            || (matches!(word, "override" | "private")
                && matches!(separator(first), Some((Sep::Rule, ..))))
        {
            first.to_string()
        } else if KEYWORDS.contains(&word) {
            collapse_words(first)
        } else if let Some((Sep::Rule, start, end)) = separator(first) {
            rule(first, start, end)
        } else {
            first.to_string()
        };
        push_text(&mut items, line_depth, text, group, keep, bail);
    }

    let mut out = render(&items, eol, options);
    if lines.last().is_some_and(|line| continued(line)) && !source.ends_with('\n') {
        out.truncate(out.len() - eol.len());
    }
    out
}

fn render(items: &[Item], eol: &str, options: &Options) -> String {
    let unit = if options.indent_conditionals {
        options.indent_width
    } else {
        0
    };
    let mut out = String::new();
    let mut blanks = 0;
    let mut width = 0;
    let skipped = |item: &&Item| options.align_across_comments && is_comment(item);
    for (n, item) in items.iter().enumerate() {
        if let Item::Blank = item {
            if !out.is_empty() {
                blanks += 1;
            }
            continue;
        }
        for _ in 0..blanks.min(options.max_blank_lines.get()) {
            out.push_str(eol);
        }
        blanks = 0;
        match item {
            Item::Blank => {}
            Item::Raw(line) => {
                out.push_str(line);
                out.push_str(eol);
            }
            Item::Recipe(line) => {
                out.push_str(line);
                out.push_str(eol);
            }
            Item::Text {
                depth,
                text,
                tail,
                keep,
            } => push_line(&mut out, eol, depth * unit, text, tail, *keep, options),
            Item::Assign {
                depth,
                name,
                op,
                value,
                tail,
            } => {
                let previous = items[..n].iter().rev().find(|item| !skipped(item));
                let starts_group =
                    !matches!(previous, Some(Item::Assign { depth: d, .. }) if d == depth);
                if starts_group {
                    width = items[n..]
                        .iter()
                        .filter(|item| !skipped(item))
                        .map_while(|item| match item {
                            Item::Assign { depth: d, name, .. } if d == depth => {
                                Some(name.chars().count())
                            }
                            _ => None,
                        })
                        .fold(0, usize::max);
                }
                let pad = if options.align_assignments {
                    width - name.chars().count()
                } else {
                    0
                };
                let mut head = String::new();
                push_assignment(&mut head, name, pad, op, value);
                push_line(
                    &mut out,
                    eol,
                    depth * unit,
                    &head,
                    tail,
                    !options.trim_trailing_whitespace,
                    options,
                );
            }
        }
    }
    out
}

fn is_comment(item: &Item) -> bool {
    matches!(item, Item::Text { text, .. } if text.starts_with('#'))
}

fn push_line(
    out: &mut String,
    eol: &str,
    indent: usize,
    head: &str,
    tail: &[&str],
    keep: bool,
    options: &Options,
) {
    push_spaces(out, indent);
    out.push_str(head);
    let Some((last, middle)) = tail.split_last() else {
        out.push_str(eol);
        return;
    };
    let tail_indent = indent + options.indent_width;
    let head_width = indent + head.chars().count();
    let width = if options.align_continuations {
        middle
            .iter()
            .map(|line| tail_indent + continued_content(line).chars().count())
            .fold(head_width, usize::max)
    } else {
        0
    };
    push_spaces(out, width.saturating_sub(head_width));
    out.push_str(" \\");
    out.push_str(eol);
    for line in middle {
        let content = continued_content(line);
        push_spaces(out, tail_indent);
        out.push_str(content);
        push_spaces(
            out,
            width.saturating_sub(tail_indent + content.chars().count()),
        );
        out.push_str(" \\");
        out.push_str(eol);
    }
    let content = last.trim_start_matches(BLANKS);
    let content = if keep {
        content
    } else {
        trim_trailing(content)
    };
    if !content.is_empty() {
        push_spaces(out, tail_indent);
        out.push_str(content);
    }
    out.push_str(eol);
}

fn push_spaces(out: &mut String, count: usize) {
    out.extend(std::iter::repeat_n(' ', count));
}

fn push_assignment(out: &mut String, name: &str, pad: usize, op: &str, value: &str) {
    out.push_str(name);
    push_spaces(out, pad);
    out.push(' ');
    out.push_str(op);
    if !value.is_empty() {
        out.push(' ');
        out.push_str(value);
    }
}

fn push_text<'a>(
    items: &mut Vec<Item<'a>>,
    depth: usize,
    text: String,
    group: &'a [&'a str],
    keep: bool,
    bail: bool,
) {
    if bail {
        push_raw(items, group, keep);
    } else {
        items.push(Item::Text {
            depth,
            text,
            tail: &group[1..],
            keep,
        });
    }
}

fn push_raw<'a>(items: &mut Vec<Item<'a>>, group: &[&'a str], keep: bool) {
    for line in group {
        items.push(Item::Raw(if keep { line } else { trim_trailing(line) }));
    }
}

fn needs_raw(group: &[&str], first: &str, prefix: char, assign: bool, posix: bool) -> bool {
    let (last, middle) = (group[group.len() - 1], &group[..group.len() - 1]);
    (posix && group.len() > 1)
        || continued(last)
        || middle.iter().any(|line| {
            let content = line[..line.len() - 1].trim_end_matches(BLANKS);
            content.ends_with('$') || content.ends_with('\\')
        })
        || (group.len() > 1 && !assign && group.iter().any(|line| line.contains(';')))
        || group
            .iter()
            .any(|line| line.contains("\\=") || line.contains("\\:") || line.contains("\\&"))
        || first.starts_with(prefix)
}

fn continued(line: &str) -> bool {
    (line.len() - line.trim_end_matches('\\').len()) % 2 == 1
}

fn strip_continuation(line: &str) -> &str {
    if continued(line) {
        &line[..line.len() - 1]
    } else {
        line
    }
}

fn continued_content(line: &str) -> &str {
    line[..line.len() - 1].trim_matches(BLANKS)
}

fn trim_trailing(line: &str) -> &str {
    let trimmed = line.trim_end_matches(BLANKS);
    if trimmed.ends_with(['\\', '$']) {
        line
    } else {
        trimmed
    }
}

fn first_word(s: &str) -> &str {
    &s[..s.find(WORD_ENDS).unwrap_or(s.len())]
}

fn starts_word(s: &str, word: &str) -> bool {
    s.strip_prefix(word)
        .is_some_and(|rest| rest.is_empty() || rest.starts_with(BLANKS))
}

fn is_define(first: &str) -> bool {
    first
        .split(BLANKS)
        .filter(|word| !word.is_empty())
        .find(|word| !MODIFIERS.contains(word))
        == Some("define")
}

fn directive(s: &str) -> String {
    let word = first_word(s);
    let rest = s[word.len()..].trim_start_matches(BLANKS);
    if rest.is_empty() {
        return word.to_string();
    }
    if word == "else" && CONDITIONALS.contains(&first_word(rest)) {
        return format!("{word} {}", directive(rest));
    }
    format!("{word} {rest}")
}

fn scan(s: &str, mut stop: impl FnMut(usize) -> bool) -> usize {
    let bytes = s.as_bytes();
    let mut depth = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' if bytes.get(i + 1) != Some(&b'$') => i += 2,
            b'#' => return i,
            b'$' => {
                if matches!(bytes.get(i + 1), Some(b'(' | b'{')) {
                    depth += 1;
                }
                i += 2;
            }
            b'(' | b'{' if depth > 0 => {
                depth += 1;
                i += 1;
            }
            b')' | b'}' if depth > 0 => {
                depth -= 1;
                i += 1;
            }
            _ if depth == 0 && stop(i) => return i,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn separator(s: &str) -> Option<(Sep, usize, usize)> {
    let bytes = s.as_bytes();
    let mut prev = None;
    let i = scan(s, |i| {
        if matches!(bytes[i], b'=' | b':') {
            return true;
        }
        prev = Some(i);
        false
    });
    let follows = |c: &[u8]| prev.is_some_and(|p: usize| p + 1 == i && c.contains(&bytes[p]));
    match bytes.get(i) {
        Some(b'=') => {
            let start = if follows(b"?+!") { i - 1 } else { i };
            Some((Sep::Assign, start, i + 1))
        }
        Some(b':') => {
            let mut end = i;
            while bytes.get(end) == Some(&b':') {
                end += 1;
            }
            if bytes.get(end) == Some(&b'=') && end - i <= 3 {
                return Some((Sep::Assign, i, end + 1));
            }
            let start = if follows(b"&") { i - 1 } else { i };
            Some((Sep::Rule, start, end))
        }
        _ => None,
    }
}

fn find_semicolon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let i = scan(s, |i| bytes[i] == b';');
    (bytes.get(i) == Some(&b';')).then_some(i)
}

fn assignment(s: &str) -> Option<Assignment<'_>> {
    let Some((Sep::Assign, start, end)) = separator(s) else {
        return None;
    };
    let name = collapse_words(&s[..start]);
    if name.is_empty() || !valid_name(&name) {
        return None;
    }
    Some(Assignment {
        name,
        op: &s[start..end],
        value: s[end..].trim_start_matches(BLANKS),
    })
}

fn valid_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let mut start = 0;
    let mut valid = true;
    scan(name, |i| {
        if bytes[i] == b' ' {
            valid &= MODIFIERS.contains(&&name[start..i]);
            start = i + 1;
        }
        false
    });
    valid
}

fn collapse_words(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut start = 0;
    scan(s, |i| {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            push_word(&mut out, &s[start..i]);
            start = i + 1;
        }
        false
    });
    push_word(&mut out, &s[start..]);
    out
}

fn push_word(out: &mut String, word: &str) {
    if word.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(' ');
    }
    out.push_str(word);
}

fn rule(s: &str, start: usize, end: usize) -> String {
    let mut out = collapse_words(&s[..start]);
    let sep = &s[start..end];
    if (sep.starts_with('&') || out.ends_with('&')) && !out.is_empty() {
        out.push(' ');
    }
    out.push_str(sep);
    let rest = &s[end..];
    let semicolon = find_semicolon(rest);
    let before = match semicolon {
        Some(p) => &rest[..p],
        None => rest,
    };
    let inner = separator(before);
    if let Some((Sep::Assign, ..)) = inner
        && let Some(assign) = assignment(rest)
    {
        out.push(' ');
        push_assignment(&mut out, &assign.name, 0, assign.op, assign.value);
        return out;
    }
    if let Some((Sep::Rule, ..)) = inner {
        let bytes = before.as_bytes();
        let colon = scan(before, |i| bytes[i] == b':');
        out.push(' ');
        out.push_str(&prerequisites(&before[..colon]));
        out.push(':');
        push_word(&mut out, &prerequisites(&before[colon + 1..]));
    } else {
        push_word(&mut out, &prerequisites(before));
    }
    if let Some(p) = semicolon {
        out.push(';');
        let after = trim_trailing(rest[p + 1..].trim_start_matches(BLANKS));
        if !after.is_empty() {
            out.push(' ');
            out.push_str(after);
        }
    }
    out
}

fn prerequisites(s: &str) -> String {
    if s.contains(['"', '\'']) || s.contains("$$") {
        return trim_trailing(s.trim_start_matches(BLANKS)).to_string();
    }
    collapse_words(s)
}

fn static_prefix(first: &str) -> Option<char> {
    let assign = assignment(first)?;
    if assign.name != ".RECIPEPREFIX" && !assign.name.ends_with(" .RECIPEPREFIX") {
        return None;
    }
    if !matches!(assign.op, "=" | ":=" | "::=" | ":::=") || assign.value.contains(['$', '\\']) {
        return None;
    }
    match assign.value.chars().next() {
        None | Some('#') => Some('\t'),
        Some(c) if c.is_ascii() => Some(c),
        Some(_) => None,
    }
}
