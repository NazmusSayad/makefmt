use std::mem::take;

use super::nodes::*;
use super::tokens::Token;

pub const ESCAPE: u8 = 0xff;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Space {
    NotRequired,
    Required,
    Written,
}

struct Indenter {
    parent: usize,
    base_indent: isize,
    first_indent: isize,
    first_change: isize,
    cur_line: Vec<u8>,
}

#[derive(Default)]
struct Out {
    base: Vec<u8>,
    indenters: Vec<Indenter>,
}

impl Out {
    fn write_byte(&mut self, depth: usize, b: u8) {
        if depth == 0 {
            self.base.push(b);
            return;
        }
        let e = &mut self.indenters[depth - 1];
        e.cur_line.push(b);
        if b != b'\n' {
            return;
        }
        let line = take(&mut e.cur_line);
        let parent = e.parent;
        let tabs = line.iter().take_while(|&&c| c == b'\t').count();
        let trimmed = &line[tabs..];
        if trimmed.len() == 1 {
            self.write_byte(parent, trimmed[0]);
            return;
        }
        let mut line_indent = tabs as isize;
        if e.first_indent < 0 {
            e.first_indent = line_indent;
            e.first_change = e.base_indent - line_indent;
            line_indent = e.base_indent;
        } else if line_indent < e.first_indent {
            line_indent = e.base_indent;
        } else {
            line_indent += e.first_change;
        }
        self.write_byte(parent, ESCAPE);
        for _ in 0..line_indent.max(0) {
            self.write_byte(parent, b'\t');
        }
        self.write_byte(parent, ESCAPE);
        let mut escaped = false;
        for &b in trimmed {
            match b {
                ESCAPE => escaped = !escaped,
                b'\t' if !escaped => {
                    self.write_byte(parent, ESCAPE);
                    self.write_byte(parent, b'\t');
                    self.write_byte(parent, ESCAPE);
                    continue;
                }
                _ => {}
            }
            self.write_byte(parent, b);
        }
    }
}

pub fn print(file: &File, indent_spaces: usize) -> Vec<u8> {
    let mut p = Printer::new(Out::default(), 0, indent_spaces);
    p.want_space = Space::Written;
    p.first_line = true;
    p.stmt_list(&file.stmts, &file.last);
    p.newline(Pos::default());
    p.flush_heredocs();
    p.flush_comments();
    p.out.base
}

fn starts_with_lparen(s: &Stmt) -> bool {
    match &s.cmd {
        Some(Command::Binary(b)) => starts_with_lparen(&b.x),
        Some(Command::Subshell(_) | Command::Arithm(_)) => true,
        _ => false,
    }
}

fn ends_with_rparen(s: &Stmt) -> bool {
    if s.background || !s.redirs.is_empty() {
        return false;
    }
    match &s.cmd {
        Some(Command::Binary(b)) => ends_with_rparen(&b.y),
        Some(Command::Subshell(_) | Command::Arithm(_)) => true,
        _ => false,
    }
}

struct Printer {
    out: Out,
    depth: usize,
    indent_spaces: usize,
    want_space: Space,
    want_newline: bool,
    must_newline: bool,
    wrote_semi: bool,
    pending_comments: Vec<Comment>,
    first_line: bool,
    line: u32,
    last_level: usize,
    level: usize,
    level_incs: Vec<bool>,
    nested_binary: bool,
    pending_hdocs: Vec<Redirect>,
}

impl Printer {
    fn new(out: Out, depth: usize, indent_spaces: usize) -> Printer {
        Printer {
            out,
            depth,
            indent_spaces,
            want_space: Space::NotRequired,
            want_newline: false,
            must_newline: false,
            wrote_semi: false,
            pending_comments: Vec::new(),
            first_line: false,
            line: 0,
            last_level: 0,
            level: 0,
            level_incs: Vec::new(),
            nested_binary: false,
            pending_hdocs: Vec::new(),
        }
    }

    fn write_byte(&mut self, b: u8) {
        self.out.write_byte(self.depth, b);
    }

    fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_byte(b);
        }
    }

    fn space(&mut self) {
        self.write_byte(b' ');
        self.want_space = Space::Written;
    }

    fn space_pad(&mut self) {
        if self.want_space == Space::Required {
            self.write_byte(b' ');
            self.want_space = Space::Written;
        }
    }

    fn wants_newline(&self, pos: Pos) -> bool {
        self.must_newline || self.want_newline || pos.line() > self.line
    }

    fn bslash_newl(&mut self) {
        if self.want_space == Space::Required {
            self.space();
        }
        self.write_str("\\\n");
        self.line += 1;
        self.indent();
    }

    fn spaced_string(&mut self, s: &str) {
        self.space_pad();
        self.write_str(s);
        self.want_space = Space::Required;
    }

    fn semi_or_newl(&mut self, s: &str, pos: Pos) {
        if self.wants_newline(Pos::default()) || !self.pending_hdocs.is_empty() {
            self.newline(pos);
            self.indent();
        } else {
            if !self.wrote_semi {
                self.write_byte(b';');
            }
            self.space();
            self.advance_line(pos.line());
        }
        self.write_str(s);
        self.want_space = Space::Required;
    }

    fn write_lit(&mut self, s: &str) {
        if self.depth == 0 && s.contains('\t') {
            self.write_byte(ESCAPE);
            self.write_str(s);
            self.write_byte(ESCAPE);
            return;
        }
        self.write_str(s);
    }

    fn inc_level(&mut self) {
        let mut inc = false;
        if self.level <= self.last_level || self.level_incs.is_empty() {
            self.level += 1;
            inc = true;
        } else if let Some(last) = self.level_incs.last_mut()
            && *last
        {
            *last = false;
            inc = true;
        }
        self.level_incs.push(inc);
    }

    fn dec_level(&mut self) {
        if self.level_incs.pop() == Some(true) {
            self.level -= 1;
        }
    }

    fn indent(&mut self) {
        self.last_level = self.level;
        if self.level == 0 {
            return;
        }
        if self.indent_spaces == 0 {
            self.write_byte(ESCAPE);
            for _ in 0..self.level {
                self.write_byte(b'\t');
            }
            self.write_byte(ESCAPE);
            return;
        }
        for _ in 0..self.indent_spaces * self.level {
            self.write_byte(b' ');
        }
    }

    fn newline(&mut self, pos: Pos) {
        self.flush_heredocs();
        self.flush_comments();
        self.write_byte(b'\n');
        self.want_space = Space::Written;
        self.want_newline = false;
        self.must_newline = false;
        self.advance_line(pos.line());
    }

    fn advance_line(&mut self, line: u32) {
        self.line = self.line.max(line);
    }

    fn flush_heredocs(&mut self) {
        if self.pending_hdocs.is_empty() {
            return;
        }
        let hdocs = take(&mut self.pending_hdocs);
        let mut coms = take(&mut self.pending_comments);
        if coms.first().is_some_and(|c| c.pos().line() == self.line) {
            let c = coms.remove(0);
            self.pending_comments.push(c);
            self.flush_comments();
        }
        let new_level = self.level;
        self.level = self.last_level;
        for r in &hdocs {
            self.line += 1;
            self.write_byte(b'\n');
            self.want_space = Space::Written;
            self.want_newline = false;
            self.must_newline = false;
            let body = r.hdoc.borrow();
            if r.op == Token::DashHdoc && self.indent_spaces == 0 {
                if let Some(body) = body.as_ref() {
                    self.out.indenters.push(Indenter {
                        parent: self.depth,
                        base_indent: self.level as isize + 1,
                        first_indent: -1,
                        first_change: 0,
                        cur_line: Vec::new(),
                    });
                    let depth = self.out.indenters.len();
                    let mut tabs = Printer::new(take(&mut self.out), depth, self.indent_spaces);
                    tabs.line = body.pos().line();
                    tabs.word_parts(&body.parts, true);
                    self.out = tabs.out;
                    self.out.indenters.pop();
                }
                self.indent();
                self.unquoted_word(&r.word);
            } else {
                let depth = self.depth;
                self.depth = 0;
                if let Some(body) = body.as_ref() {
                    self.word_parts(&body.parts, true);
                }
                self.unquoted_word(&r.word);
                self.depth = depth;
            }
            if let Some(body) = body.as_ref() {
                self.advance_line(body.end().line());
            }
            self.want_space = Space::NotRequired;
        }
        self.level = new_level;
        self.pending_comments = coms;
        self.must_newline = true;
    }

    fn newlines(&mut self, pos: Pos) {
        if self.first_line && self.pending_comments.is_empty() {
            self.first_line = false;
            return;
        }
        if !self.wants_newline(pos) {
            return;
        }
        self.flush_heredocs();
        self.flush_comments();
        self.write_byte(b'\n');
        self.want_space = Space::Written;
        self.want_newline = false;
        self.must_newline = false;
        let l = pos.line();
        if l > self.line + 1 {
            self.write_byte(b'\n');
        }
        self.advance_line(l);
        self.indent();
    }

    fn right_paren(&mut self, pos: Pos) {
        self.newlines(pos);
        self.write_byte(b')');
        self.want_space = Space::Required;
    }

    fn closing_paren(&mut self, stmts: &[Stmt], last: &[Comment], open: Pos, close: Pos) {
        self.want_space = Space::NotRequired;
        if last.is_empty()
            && stmts.len() == 1
            && ends_with_rparen(&stmts[0])
            && open.line() == close.line()
        {
            self.want_space = Space::Required;
        }
        self.space_pad();
        self.right_paren(close);
    }

    fn semi_rsrv(&mut self, s: &str, pos: Pos) {
        if self.wants_newline(pos) {
            self.newlines(pos);
        } else {
            if !self.wrote_semi {
                self.write_byte(b';');
            }
            self.space_pad();
        }
        self.write_str(s);
        self.want_space = Space::Required;
    }

    fn flush_comments(&mut self) {
        if !self.pending_comments.is_empty() {
            self.flush_heredocs();
        }
        let coms = take(&mut self.pending_comments);
        for (i, c) in coms.iter().enumerate() {
            self.first_line = false;
            let cline = c.hash.line();
            if self.must_newline || i > 0 || (cline > self.line && self.line > 0) {
                self.write_byte(b'\n');
                if cline > self.line + 1 {
                    self.write_byte(b'\n');
                }
                self.indent();
                self.want_space = Space::Written;
                self.space_pad();
            } else if self.want_space == Space::Required {
                self.write_byte(b'\t');
            } else if self.want_space != Space::Written {
                self.space();
            }
            self.advance_line(cline);
            self.write_byte(b'#');
            self.write_lit(c.text.trim_end());
            self.want_newline = true;
            self.must_newline = true;
        }
    }

    fn comments(&mut self, comments: &[Comment]) {
        self.pending_comments.extend_from_slice(comments);
    }

    fn word_parts(&mut self, wps: &[WordPart], quoted: bool) {
        if !quoted && wps.first().is_some_and(|wp| wp.pos().line() > self.line) {
            self.bslash_newl();
        }
        for wp in wps {
            while quoted && wp.pos().line() > self.line {
                self.write_str("\\\n");
                self.line += 1;
            }
            self.word_part(wp);
            self.advance_line(wp.end().line());
        }
    }

    fn word_part(&mut self, wp: &WordPart) {
        match wp {
            WordPart::Lit(l) => {
                self.write_lit(&l.value);
                let n = l.value.len() - l.value.trim_end_matches('\\').len();
                if n % 2 == 1 {
                    self.write_byte(b'\\');
                }
            }
            WordPart::SglQuoted(q) => {
                if q.dollar {
                    self.write_byte(b'$');
                }
                self.write_byte(b'\'');
                self.write_lit(&q.value);
                self.write_byte(b'\'');
                self.advance_line(wp.end().line());
            }
            WordPart::DblQuoted(q) => self.dbl_quoted(q),
            WordPart::CmdSubst(cs) => {
                self.advance_line(wp.pos().line());
                self.cmd_subst(cs);
            }
            WordPart::ParamExp(pe) => self.param_exp(pe),
            WordPart::ArithmExp(a) => {
                self.write_str("$((");
                self.arithm_expr(Some(&a.x), false, false);
                self.write_str("))");
            }
            WordPart::ExtGlob(g) => {
                self.write_str(g.op.text());
                self.write_lit(&g.pattern.value);
                self.write_byte(b')');
            }
            WordPart::ProcSubst(ps) => {
                if self.want_space == Space::Required {
                    self.space();
                }
                self.write_str(ps.op.text());
                self.nested_stmts(&ps.stmts, &ps.last, ps.rparen);
                self.right_paren(ps.rparen);
            }
        }
    }

    fn dbl_quoted(&mut self, dq: &DblQuoted) {
        if dq.dollar {
            self.write_byte(b'$');
        }
        self.write_byte(b'"');
        if !dq.parts.is_empty() {
            self.word_parts(&dq.parts, true);
        }
        while self.line < dq.right.line() {
            self.write_str("\\\n");
            self.line += 1;
        }
        self.write_byte(b'"');
    }

    fn wrote_index(&mut self, index: Option<&ArithmExpr>) -> bool {
        let Some(index) = index else {
            return false;
        };
        self.write_byte(b'[');
        let compact = matches!(index, ArithmExpr::Binary(b) if b.op == Token::Comma);
        self.arithm_expr(Some(index), compact, false);
        self.write_byte(b']');
        true
    }

    fn param_exp(&mut self, pe: &ParamExp) {
        if pe.naked_index() {
            if let Some(param) = &pe.param {
                self.write_lit(&param.value);
            }
            self.wrote_index(pe.index.as_ref());
            return;
        }
        self.write_byte(b'$');
        if !pe.short {
            self.write_byte(b'{');
        }
        if pe.length {
            self.write_byte(b'#');
        } else if pe.excl {
            self.write_byte(b'!');
        }
        if let Some(param) = &pe.param {
            self.write_lit(&param.value);
        }
        self.wrote_index(pe.index.as_ref());
        if let Some(slice) = &pe.slice {
            self.write_byte(b':');
            self.arithm_expr(slice.offset.as_ref(), true, true);
            if let Some(length) = &slice.length {
                self.write_byte(b':');
                self.arithm_expr(Some(length), true, false);
            }
        } else if let Some(repl) = &pe.repl {
            if repl.all {
                self.write_byte(b'/');
            }
            self.write_byte(b'/');
            if let Some(orig) = &repl.orig {
                self.word(orig);
            }
            self.write_byte(b'/');
            if let Some(with) = &repl.with {
                self.word(with);
            }
        } else if let Some(names) = pe.names {
            self.write_lit(names.text());
        } else if let Some(exp) = &pe.exp {
            self.write_str(exp.op.text());
            if let Some(word) = &exp.word {
                self.word(word);
            }
        }
        if !pe.short {
            self.write_byte(b'}');
        }
    }

    fn cmd_subst(&mut self, cs: &CmdSubst) {
        if cs.temp_file {
            self.write_str("${");
            self.want_space = Space::Required;
            self.nested_stmts(&cs.stmts, &cs.last, cs.right);
            self.want_space = Space::NotRequired;
            self.semi_rsrv("}", cs.right);
        } else if cs.reply_var {
            self.write_str("${|");
            self.nested_stmts(&cs.stmts, &cs.last, cs.right);
            self.want_space = Space::NotRequired;
            self.semi_rsrv("}", cs.right);
        } else if cs.backquotes
            && cs.stmts.is_empty()
            && cs.last.len() == 1
            && cs.right.line() == self.line
        {
            self.write_str("`#");
            self.write_str(&cs.last[0].text);
            self.write_str("`");
        } else {
            self.write_str("$(");
            self.want_space = if cs.stmts.first().is_some_and(starts_with_lparen) {
                Space::Required
            } else {
                Space::NotRequired
            };
            self.nested_stmts(&cs.stmts, &cs.last, cs.right);
            self.closing_paren(&cs.stmts, &cs.last, cs.left, cs.right);
        }
    }

    fn loop_(&mut self, l: &Loop) {
        match l {
            Loop::WordIter(wi) => {
                self.write_lit(&wi.name.value);
                if wi.in_pos.is_valid() {
                    self.spaced_string(" in");
                    self.word_join(&wi.items);
                }
            }
            Loop::CStyle(c) => {
                self.write_str("((");
                if c.init.is_none() {
                    self.space();
                }
                self.arithm_expr(c.init.as_ref(), false, false);
                self.write_str("; ");
                self.arithm_expr(c.cond.as_ref(), false, false);
                self.write_str("; ");
                self.arithm_expr(c.post.as_ref(), false, false);
                self.write_str("))");
            }
        }
    }

    fn arithm_expr(&mut self, expr: Option<&ArithmExpr>, compact: bool, space_plus_minus: bool) {
        let Some(expr) = expr else {
            return;
        };
        match expr {
            ArithmExpr::Word(w) => self.word(w),
            ArithmExpr::Binary(b) => {
                self.arithm_expr(Some(&b.x), compact, space_plus_minus);
                if compact {
                    self.write_str(b.op.text());
                } else {
                    if b.op != Token::Comma {
                        self.space();
                    }
                    self.write_str(b.op.text());
                    self.space();
                }
                self.arithm_expr(Some(&b.y), compact, false);
            }
            ArithmExpr::Unary(u) => {
                if u.post {
                    self.arithm_expr(Some(&u.x), compact, space_plus_minus);
                    self.write_str(u.op.text());
                    return;
                }
                if space_plus_minus && matches!(u.op, Token::Plus | Token::Minus) {
                    self.space();
                }
                self.write_str(u.op.text());
                if u.op == Token::ExclMark && !compact {
                    self.space();
                }
                self.arithm_expr(Some(&u.x), compact, false);
            }
            ArithmExpr::Paren(p) => {
                self.write_byte(b'(');
                self.arithm_expr(Some(&p.x), false, false);
                self.write_byte(b')');
            }
        }
    }

    fn test_expr(&mut self, expr: &TestExpr) {
        if expr.pos().line() > self.line {
            self.newlines(expr.pos());
            self.space_pad();
        } else if self.want_space == Space::Required {
            self.space();
        }
        self.test_expr_same_line(expr);
    }

    fn test_expr_same_line(&mut self, expr: &TestExpr) {
        self.advance_line(expr.pos().line());
        match expr {
            TestExpr::Word(w) => self.word(w),
            TestExpr::Binary(b) => {
                self.test_expr_same_line(&b.x);
                self.space();
                self.write_str(b.op.text());
                if matches!(b.op, Token::AndAnd | Token::OrOr) {
                    self.want_space = Space::Required;
                    self.test_expr(&b.y);
                } else {
                    self.space();
                    self.test_expr_same_line(&b.y);
                }
            }
            TestExpr::Unary(u) => {
                self.write_str(u.op.text());
                self.space();
                self.test_expr_same_line(&u.x);
            }
            TestExpr::Paren(p) => {
                self.write_byte(b'(');
                self.want_space = Space::NotRequired;
                self.test_expr(&p.x);
                self.write_byte(b')');
            }
        }
    }

    fn word(&mut self, w: &Word) {
        self.word_parts(&w.parts, false);
        self.want_space = Space::Required;
    }

    fn unquoted_word(&mut self, w: &Word) {
        for wp in &w.parts {
            match wp {
                WordPart::SglQuoted(q) => self.write_lit(&q.value),
                WordPart::DblQuoted(q) => self.word_parts(&q.parts, true),
                WordPart::Lit(l) => {
                    let bytes = l.value.as_bytes();
                    let mut i = 0;
                    while i < bytes.len() {
                        if bytes[i] == b'\\' {
                            i += 1;
                            if i < bytes.len() {
                                self.write_byte(bytes[i]);
                            }
                        } else {
                            self.write_byte(bytes[i]);
                        }
                        i += 1;
                    }
                }
                _ => {}
            }
        }
    }

    fn word_join(&mut self, ws: &[Word]) {
        let mut any_newline = false;
        for w in ws {
            if w.pos().line() > self.line {
                if !any_newline {
                    self.inc_level();
                    any_newline = true;
                }
                self.bslash_newl();
            }
            self.space_pad();
            self.word(w);
        }
        if any_newline {
            self.dec_level();
        }
    }

    fn case_pattern_join(&mut self, pats: &[Word]) {
        let mut any_newline = false;
        for (i, w) in pats.iter().enumerate() {
            if i == 0 && w.lit() == "esac" {
                self.write_str("(");
            }
            if i > 0 {
                self.spaced_string("|");
            }
            if self.wants_newline(w.pos()) {
                if !any_newline {
                    self.inc_level();
                    any_newline = true;
                }
                self.bslash_newl();
            } else {
                self.space_pad();
            }
            self.word(w);
        }
        if any_newline {
            self.dec_level();
        }
    }

    fn elem_join(&mut self, elems: &[ArrayElem], last: &[Comment]) {
        self.inc_level();
        for el in elems {
            let mut left = Vec::new();
            for c in &el.comments {
                if c.pos().after(el.pos()) {
                    left.push(c.clone());
                    break;
                }
                self.comments(std::slice::from_ref(c));
            }
            if el.pos().line() > self.line {
                self.newlines(el.pos());
                self.space_pad();
            } else if self.want_space == Space::Required {
                self.space();
            }
            if self.wrote_index(el.index.as_ref()) {
                self.write_byte(b'=');
            }
            if let Some(value) = &el.value {
                self.word(value);
            }
            self.comments(&left);
        }
        if !last.is_empty() {
            self.comments(last);
            self.flush_comments();
        }
        self.dec_level();
    }

    fn redirect(&mut self, r: &Redirect) {
        if let Some(n) = &r.n {
            self.write_lit(&n.value);
        }
        self.write_str(r.op.text());
        self.want_space = Space::Required;
        self.word(&r.word);
    }

    fn stmt(&mut self, s: &Stmt) {
        self.wrote_semi = false;
        if s.negated {
            self.spaced_string("!");
        }
        let mut start_redirs = 0;
        if let Some(cmd) = &s.cmd {
            start_redirs = self.command(cmd, &s.redirs);
        }
        self.inc_level();
        for r in &s.redirs[start_redirs..] {
            if self.wants_newline(r.op_pos) {
                self.bslash_newl();
            }
            self.space_pad();
            self.redirect(r);
            if r.op == Token::Hdoc || r.op == Token::DashHdoc {
                self.pending_hdocs.push(r.clone());
            }
        }
        let sep = s.semicolon.is_valid() && s.semicolon.line() > self.line;
        if sep || s.background {
            if sep {
                self.bslash_newl();
            } else {
                self.space();
            }
            self.write_str(if s.background { "&" } else { ";" });
            self.wrote_semi = true;
            self.want_space = Space::Required;
        }
        self.dec_level();
    }

    fn print_redirs_until(&mut self, redirs: &[Redirect], start: usize, pos: Pos) -> usize {
        let mut start = start;
        for r in &redirs[start..] {
            if r.pos().after(pos) || r.op == Token::Hdoc || r.op == Token::DashHdoc {
                break;
            }
            self.space_pad();
            self.redirect(r);
            start += 1;
        }
        start
    }

    fn command(&mut self, cmd: &Command, redirs: &[Redirect]) -> usize {
        let mut start_redirs = 0;
        self.advance_line(cmd.pos().line());
        self.space_pad();
        match cmd {
            Command::Call(c) => {
                self.assigns(&c.assigns);
                if let Some(first) = c.args.first() {
                    start_redirs = self.print_redirs_until(redirs, start_redirs, first.pos());
                }
                if c.args.len() <= 1 {
                    self.word_join(&c.args);
                    return start_redirs;
                }
                self.word_join(&c.args[..1]);
                start_redirs = self.print_redirs_until(redirs, start_redirs, c.args[1].pos());
                self.word_join(&c.args[1..]);
            }
            Command::Block(b) => {
                self.write_byte(b'{');
                self.wrote_semi = true;
                self.want_space = Space::Required;
                self.nested_stmts(&b.stmts, &b.last, b.rbrace);
                self.semi_rsrv("}", b.rbrace);
            }
            Command::If(ic) => self.if_clause(ic, false),
            Command::Subshell(s) => {
                self.write_byte(b'(');
                if s.stmts.first().is_some_and(starts_with_lparen) {
                    self.want_space = Space::Required;
                    if s.lparen.line() != s.stmts[0].pos().line() || s.stmts.len() > 1 {
                        self.want_space = Space::NotRequired;
                    }
                } else if s.stmts.is_empty() {
                    self.want_space = Space::Required;
                } else {
                    self.want_space = Space::NotRequired;
                }
                self.space_pad();
                self.nested_stmts(&s.stmts, &s.last, s.rparen);
                self.closing_paren(&s.stmts, &s.last, s.lparen, s.rparen);
            }
            Command::While(w) => {
                self.spaced_string(if w.until { "until" } else { "while" });
                self.nested_stmts(&w.cond, &w.cond_last, Pos::default());
                self.semi_or_newl("do", w.do_pos);
                self.nested_stmts(&w.do_, &w.do_last, w.done_pos);
                self.semi_rsrv("done", w.done_pos);
            }
            Command::For(f) => {
                self.write_str(if f.select { "select " } else { "for " });
                self.loop_(&f.loop_);
                self.semi_or_newl("do", f.do_pos);
                self.nested_stmts(&f.do_, &f.do_last, f.done_pos);
                self.semi_rsrv("done", f.done_pos);
            }
            Command::Binary(b) => {
                self.stmt(&b.x);
                if b.y.pos().line() <= self.line {
                    self.spaced_string(b.op.text());
                    self.advance_line(b.y.pos().line());
                    self.stmt(&b.y);
                    return start_redirs;
                }
                let indent = !self.nested_binary;
                if indent {
                    self.inc_level();
                }
                self.spaced_string(b.op.text());
                self.advance_line(b.op_pos.line());
                self.comments(&b.y.comments);
                self.newline(Pos::default());
                self.indent();
                self.advance_line(b.y.pos().line());
                self.nested_binary = matches!(b.y.cmd, Some(Command::Binary(_)));
                self.stmt(&b.y);
                if indent {
                    self.dec_level();
                }
                self.nested_binary = false;
            }
            Command::Func(f) => {
                if f.rsrv_word {
                    self.spaced_string("function");
                }
                if let Some(name) = &f.name {
                    self.spaced_string(&name.value);
                }
                if !f.rsrv_word || f.parens {
                    self.write_str("()");
                    self.want_space = Space::NotRequired;
                }
                self.space();
                self.advance_line(f.body.pos().line());
                self.comments(&f.body.comments);
                self.stmt(&f.body);
            }
            Command::Case(c) => {
                self.write_str("case ");
                self.word(&c.word);
                self.write_str(" in");
                self.advance_line(c.in_.line());
                self.want_space = Space::Required;
                if c.items.is_empty() {
                    self.must_newline = true;
                }
                for ci in &c.items {
                    let mut last: &[Comment] = &[];
                    for (i, com) in ci.comments.iter().enumerate() {
                        if com.pos().after(ci.pos()) {
                            last = &ci.comments[i..];
                            break;
                        }
                        self.comments(std::slice::from_ref(com));
                    }
                    self.newlines(ci.pos());
                    self.space_pad();
                    self.case_pattern_join(&ci.patterns);
                    self.write_byte(b')');
                    self.want_space = Space::Required;
                    self.nested_stmts(&ci.stmts, &ci.last, ci.op_pos);
                    self.level += 1;
                    if self.wants_newline(ci.op_pos) {
                        self.newlines(ci.op_pos);
                        self.want_newline = true;
                    }
                    self.spaced_string(ci.op.text());
                    self.advance_line(ci.op_pos.line());
                    self.wrote_semi = true;
                    self.comments(last);
                    self.flush_comments();
                    self.level -= 1;
                }
                self.comments(&c.last);
                self.semi_rsrv("esac", c.esac);
            }
            Command::Arithm(a) => {
                self.write_str("((");
                self.arithm_expr(Some(&a.x), false, false);
                self.write_str("))");
            }
            Command::Test(t) => {
                self.write_str("[[ ");
                self.inc_level();
                self.test_expr(&t.x);
                self.dec_level();
                self.spaced_string("]]");
            }
            Command::Decl(d) => {
                self.spaced_string(&d.variant.value);
                self.assigns(&d.args);
            }
            Command::Time(t) => {
                self.spaced_string("time");
                if t.posix_format {
                    self.spaced_string("-p");
                }
                if let Some(stmt) = &t.stmt {
                    self.stmt(stmt);
                }
            }
            Command::Coproc(c) => {
                self.spaced_string("coproc");
                if let Some(name) = &c.name {
                    self.space();
                    self.word(name);
                }
                self.space();
                if let Some(stmt) = &c.stmt {
                    self.stmt(stmt);
                }
            }
            Command::Let(l) => {
                self.spaced_string("let");
                for x in &l.exprs {
                    self.space();
                    self.arithm_expr(Some(x), true, false);
                }
            }
        }
        start_redirs
    }

    fn if_clause(&mut self, ic: &IfClause, elif: bool) {
        if !elif {
            self.spaced_string("if");
        }
        self.nested_stmts(&ic.cond, &ic.cond_last, Pos::default());
        self.semi_or_newl("then", ic.then_pos);
        let then_end = match &ic.else_ {
            Some(el) => el.position,
            None => ic.fi_pos,
        };
        self.nested_stmts(&ic.then, &ic.then_last, then_end);
        match &ic.else_ {
            Some(el) if el.then_pos.is_valid() => {
                self.comments(&ic.last);
                self.semi_rsrv("elif", el.position);
                self.if_clause(el, true);
                return;
            }
            Some(el) => {
                let mut left = Vec::new();
                for c in &ic.last {
                    if c.pos().after(el.position) {
                        left.push(c.clone());
                        break;
                    }
                    self.comments(std::slice::from_ref(c));
                }
                self.semi_rsrv("else", el.position);
                self.comments(&left);
                self.nested_stmts(&el.then, &el.then_last, ic.fi_pos);
                self.comments(&el.last);
            }
            None => self.comments(&ic.last),
        }
        self.semi_rsrv("fi", ic.fi_pos);
    }

    fn stmt_list(&mut self, stmts: &[Stmt], last: &[Comment]) {
        let sep = self.want_newline || stmts.first().is_some_and(|s| s.pos().line() > self.line);
        for s in stmts {
            let pos = s.pos();
            let mut mid_coms = Vec::new();
            let mut end_coms = Vec::new();
            for c in &s.comments {
                if s.cmd.as_ref().is_some_and(|cmd| c.end().after(cmd.end())) {
                    end_coms.push(c.clone());
                    break;
                }
                if c.pos().after(pos) {
                    mid_coms.push(c.clone());
                    continue;
                }
                self.comments(std::slice::from_ref(c));
            }
            self.newlines(pos);
            self.advance_line(pos.line());
            self.comments(&mid_coms);
            self.stmt(s);
            self.comments(&end_coms);
            self.want_newline = true;
        }
        if stmts.len() == 1 && !sep {
            self.want_newline = false;
        }
        self.comments(last);
    }

    fn nested_stmts(&mut self, stmts: &[Stmt], last: &[Comment], closing: Pos) {
        self.inc_level();
        if stmts.len() > 1
            || (closing.line() > self.line
                && !stmts.is_empty()
                && stmts_end(stmts, last).line() < closing.line())
            || (!self.pending_comments.is_empty() && !stmts.is_empty())
        {
            self.want_newline = true;
        }
        self.stmt_list(stmts, last);
        if closing.is_valid() {
            self.flush_comments();
        }
        self.dec_level();
    }

    fn assigns(&mut self, assigns: &[Assign]) {
        self.inc_level();
        for a in assigns {
            if self.wants_newline(a.pos()) {
                self.bslash_newl();
            } else {
                self.space_pad();
            }
            if let Some(name) = &a.name {
                self.write_lit(&name.value);
                self.wrote_index(a.index.as_ref());
                if a.append {
                    self.write_byte(b'+');
                }
                if !a.naked {
                    self.write_byte(b'=');
                }
            }
            if let Some(value) = &a.value {
                self.advance_line(value.pos().line());
                self.word(value);
            } else if let Some(array) = &a.array {
                self.want_space = Space::NotRequired;
                self.write_byte(b'(');
                self.elem_join(&array.elems, &array.last);
                self.right_paren(array.rparen);
            }
            self.want_space = Space::Required;
        }
        self.dec_level();
    }
}
