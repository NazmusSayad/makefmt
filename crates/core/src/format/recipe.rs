use super::continued;
use crate::shfmt::{self, Lang};

const MARKER: &str = "__mkfmt";
const FLAGS: [char; 5] = [' ', '\t', '@', '-', '+'];
const ASSIGN_OPS: [&str; 5] = ["=", ":=", "::=", ":::=", "?="];

pub fn shell_lang(source: &str, oneshell: bool) -> Option<Lang> {
    let mut found = None;
    for line in source.lines() {
        let mut rest = line;
        while let Some(at) = rest.find("SHELL") {
            let before = rest[..at].chars().next_back();
            rest = &rest[at + "SHELL".len()..];
            if !matches!(before, None | Some(' ' | '\t' | ':')) {
                continue;
            }
            let after = rest.trim_start_matches([' ', '\t']);
            let Some(value) = ASSIGN_OPS.iter().find_map(|op| after.strip_prefix(op)) else {
                if after.is_empty() || after.starts_with("+=") || after.starts_with("!=") {
                    return None;
                }
                continue;
            };
            let lang = value_lang(value, oneshell)?;
            if found.is_some_and(|found| found != lang) {
                return None;
            }
            found = Some(lang);
        }
    }
    match found {
        Some(lang) => Some(lang),
        None => Some(Lang::Posix),
    }
}

fn value_lang(value: &str, oneshell: bool) -> Option<Lang> {
    let value = match value.find('#') {
        Some(at) => &value[..at],
        None => value,
    };
    if value.contains(['$', '\\']) {
        return None;
    }
    let words: Vec<&str> = value.split_whitespace().collect();
    let name = match words.as_slice() {
        [path] => basename(path),
        [env, name] if basename(env) == "env" && !oneshell => name,
        _ => return None,
    };
    match name {
        "sh" => Some(Lang::Posix),
        "bash" => Some(Lang::Bash),
        _ => None,
    }
}

fn basename(path: &str) -> &str {
    match path.rfind('/') {
        Some(at) => &path[at + 1..],
        None => path,
    }
}

pub fn tidy(line: &str, prefix: char) -> String {
    let (pre, rest) = head(line, prefix);
    format!("{pre}{rest}")
}

fn head(line: &str, prefix: char) -> (String, &str) {
    let body = &line[prefix.len_utf8()..];
    let rest = body.trim_start_matches(FLAGS);
    let flags = body[..body.len() - rest.len()].replace([' ', '\t'], "");
    (format!("{prefix}{flags}"), rest)
}

pub fn format(group: &[&str], prefix: char, lang: Lang, indent: usize) -> Option<Vec<String>> {
    if continued(group[group.len() - 1]) {
        return None;
    }
    let (pre, _) = head(group[0], prefix);
    let text = script(group, prefix, false);
    let (shell, refs) = to_shell(&text)?;
    if shell.trim().is_empty() {
        return None;
    }
    let formatted = shfmt::format(&shell, lang, indent).ok()?;
    let restored = restore(formatted.strip_suffix('\n')?, &refs)?;
    let lines: Vec<&str> = restored.split('\n').collect();
    let mut out = Vec::with_capacity(lines.len());
    for (k, line) in lines.iter().enumerate() {
        let mut text = match k {
            0 => format!("{pre}{line}"),
            _ => format!("{prefix}{line}"),
        };
        if k + 1 < lines.len() && !continued(line) {
            text.push_str(if joins(line) { " \\" } else { "; \\" });
        }
        out.push(text);
    }
    let rendered: Vec<&str> = out.iter().map(String::as_str).collect();
    if continued(rendered[rendered.len() - 1]) {
        return None;
    }
    let text = script(&rendered, prefix, false);
    let (again, again_refs) = to_shell(&text)?;
    if again_refs != refs || shfmt::format(&again, lang, indent).ok()? != formatted {
        return None;
    }
    Some(out)
}

pub fn format_oneshell(
    lines: &[&str],
    prefix: char,
    lang: Lang,
    indent: usize,
) -> Option<Vec<String>> {
    let (pre, _) = head(lines[0], prefix);
    let layout = script(lines, prefix, true);
    let real = script(lines, prefix, false);
    let (shell, refs) = to_shell(&layout)?;
    let (real_shell, real_refs) = to_shell(&real)?;
    if shell.trim().is_empty() {
        return None;
    }
    let formatted = shfmt::format(&shell, lang, indent).ok()?;
    let expected = shfmt::format(&real_shell, lang, indent).ok()?;
    let restored = restore(formatted.strip_suffix('\n')?, &refs)?;
    for blank in [String::new(), prefix.to_string()] {
        let mut out = Vec::new();
        let mut joined = false;
        for (k, line) in restored.split('\n').enumerate() {
            out.push(match k {
                0 => format!("{pre}{line}"),
                _ if joined || !line.is_empty() => format!("{prefix}{line}"),
                _ => blank.clone(),
            });
            joined = continued(line);
        }
        let rendered: Vec<&str> = out.iter().map(String::as_str).collect();
        let layout = script(&rendered, prefix, true);
        let real = script(&rendered, prefix, false);
        let same = |text: &str, refs: &[&str], formatted: &str| {
            to_shell(text).is_some_and(|(shell, again)| {
                again == refs && shfmt::format(&shell, lang, indent).is_ok_and(|f| f == formatted)
            })
        };
        if !continued(rendered[rendered.len() - 1])
            && same(&layout, &refs, &formatted)
            && same(&real, &real_refs, &expected)
        {
            return Some(out);
        }
    }
    None
}

fn script(lines: &[&str], prefix: char, layout: bool) -> String {
    let mut out = Vec::with_capacity(lines.len());
    let mut joined = false;
    for line in lines {
        if joined {
            out.push(line.strip_prefix(prefix).unwrap_or(line));
        } else if let Some(body) = line.strip_prefix(prefix) {
            out.push(body.trim_start_matches(FLAGS));
        } else if layout {
            out.push("");
        }
        joined = continued(line);
    }
    out.join("\n")
}

fn to_shell(text: &str) -> Option<(String, Vec<&str>)> {
    if text.contains(MARKER) {
        return None;
    }
    let mut shell = String::with_capacity(text.len());
    let mut refs = Vec::new();
    let mut i = 0;
    while let Some(offset) = text[i..].find('$') {
        let at = i + offset;
        shell.push_str(&text[i..at]);
        let next = text[at + 1..].chars().next()?;
        let end = match next {
            '$' => {
                shell.push('$');
                i = at + 2;
                continue;
            }
            '(' | '{' => {
                let close = if next == '(' { ')' } else { '}' };
                let mut depth = 0;
                let len = text[at + 1..].find(|c: char| {
                    if c == next {
                        depth += 1;
                    } else if c == close {
                        depth -= 1;
                    }
                    depth == 0
                })?;
                at + 2 + len
            }
            ' ' | '\t' | '\n' | '\\' => return None,
            _ => at + 1 + next.len_utf8(),
        };
        let reference = &text[at..end];
        if reference.contains('\n') {
            return None;
        }
        shell.push_str(&format!("{MARKER}{}__", refs.len()));
        refs.push(reference);
        i = end;
    }
    shell.push_str(&text[i..]);
    Some((shell, refs))
}

fn restore(formatted: &str, refs: &[&str]) -> Option<String> {
    let mut out = String::with_capacity(formatted.len());
    let mut rest = formatted;
    while let Some(at) = rest.find(MARKER) {
        out.push_str(&rest[..at].replace('$', "$$"));
        let tail = &rest[at + MARKER.len()..];
        let digits = tail.len() - tail.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        let n: usize = tail[..digits].parse().ok()?;
        out.push_str(refs.get(n)?);
        rest = tail[digits..].strip_prefix("__")?;
    }
    out.push_str(&rest.replace('$', "$$"));
    Some(out)
}

fn joins(line: &str) -> bool {
    let line = line.trim_end();
    line.is_empty()
        || ["&&", "||", "|", "|&", "{", "(", ";;", ";&"]
            .iter()
            .any(|end| line.ends_with(end))
        || matches!(
            line.rsplit([' ', '\t', ';']).next(),
            Some("then" | "do" | "else" | "in")
        )
}
