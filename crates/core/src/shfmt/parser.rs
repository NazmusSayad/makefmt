use std::cell::RefCell;
use std::mem::take;
use std::rc::Rc;

use super::lexer::*;
use super::nodes::*;
use super::tokens::{Token, quote};

pub type PResult<T> = Result<T, String>;

pub const NO_STATE: u32 = 1 << 0;
pub const RUNE_BY_RUNE: u32 = 1 << 1;
pub const UNQUOTED_WORD_CONT: u32 = 1 << 2;
pub const SUB_CMD: u32 = 1 << 3;
pub const SUB_CMD_BCKQUO: u32 = 1 << 4;
pub const SUB_CMD_BRACES: u32 = 1 << 5;
pub const DBL_QUOTES: u32 = 1 << 6;
pub const HDOC_WORD: u32 = 1 << 7;
pub const HDOC_BODY: u32 = 1 << 8;
pub const HDOC_BODY_TABS: u32 = 1 << 9;
pub const ARITHM_EXPR: u32 = 1 << 10;
pub const ARITHM_EXPR_LET: u32 = 1 << 11;
pub const ARITHM_EXPR_CMD: u32 = 1 << 12;
pub const TEST_EXPR: u32 = 1 << 13;
pub const TEST_EXPR_REGEXP: u32 = 1 << 14;
pub const SWITCH_CASE: u32 = 1 << 15;
pub const PARAM_EXP_ARITHM: u32 = 1 << 16;
pub const PARAM_EXP_SLICE: u32 = 1 << 17;
pub const PARAM_EXP_REPL: u32 = 1 << 18;
pub const PARAM_EXP_EXP: u32 = 1 << 19;
pub const ARRAY_ELEMS: u32 = 1 << 20;

pub const ALL_KEEP_SPACES: u32 =
    RUNE_BY_RUNE | PARAM_EXP_REPL | DBL_QUOTES | HDOC_BODY | HDOC_BODY_TABS | PARAM_EXP_EXP;
pub const ALL_REG_TOKENS: u32 = NO_STATE
    | UNQUOTED_WORD_CONT
    | SUB_CMD
    | SUB_CMD_BCKQUO
    | SUB_CMD_BRACES
    | HDOC_WORD
    | SWITCH_CASE
    | ARRAY_ELEMS
    | TEST_EXPR;
pub const ALL_ARITHM_EXPR: u32 =
    ARITHM_EXPR | ARITHM_EXPR_LET | ARITHM_EXPR_CMD | PARAM_EXP_ARITHM | PARAM_EXP_SLICE;
pub const ALL_PARAM_EXP: u32 = PARAM_EXP_ARITHM | PARAM_EXP_SLICE | PARAM_EXP_REPL | PARAM_EXP_EXP;

pub const LANG_BASH: u32 = 1 << 0;
pub const LANG_POSIX: u32 = 1 << 1;
pub const LANG_MKSH: u32 = 1 << 2;
pub const LANG_BATS: u32 = 1 << 3;
pub const LANG_ZSH: u32 = 1 << 4;
pub const LANG_BASH_LIKE: u32 = LANG_BASH | LANG_BATS;

const LANG_NAMES: [(u32, &str); 5] = [
    (LANG_BASH, "bash"),
    (LANG_POSIX, "posix"),
    (LANG_MKSH, "mksh"),
    (LANG_BATS, "bats"),
    (LANG_ZSH, "zsh"),
];

pub const READ_BUF_SIZE: usize = 1 << 10;

const MAX_DEPTH: usize = 100;

pub struct PendingHdoc {
    op: Token,
    word: Word,
    pos: Pos,
    body: Rc<RefCell<Option<Word>>>,
}

#[derive(Clone, Copy)]
pub struct SaveState {
    quote: u32,
    buried_hdocs: usize,
}

pub struct Parser<'a> {
    pub(super) src: &'a [u8],
    pub(super) bs: &'a [u8],
    pub(super) bsp: usize,
    pub(super) r: Rune,
    pub(super) w: usize,
    pub(super) spaced: bool,
    pub(super) read_err: bool,
    pub(super) read_eof: bool,
    pub(super) tok: Token,
    pub(super) val: String,
    pub(super) offs: usize,
    pub(super) line: i64,
    pub(super) col: i64,
    pub(super) pos: Pos,
    pub(super) quote: u32,
    pub(super) eql_offs: isize,
    pub(super) lang: u32,
    pub(super) forbid_nested: bool,
    pub(super) depth: usize,
    pub(super) buried_hdocs: usize,
    pub(super) heredocs: Vec<PendingHdoc>,
    pub(super) hdoc_stops: Vec<Option<Vec<u8>>>,
    pub(super) open_bquotes: usize,
    pub(super) open_bquote_dbls: usize,
    pub(super) last_bquote_esc: usize,
    pub(super) rx_open_parens: i32,
    pub(super) rx_first_part: bool,
    pub(super) acc_coms: Vec<Comment>,
    pub(super) lit_bs: Option<Vec<u8>>,
}

pub fn parse(src: &str, lang: u32) -> PResult<File> {
    let mut p = Parser {
        src: src.as_bytes(),
        bs: &[],
        bsp: 0,
        r: Rune::Ch('\0'),
        w: 0,
        spaced: false,
        read_err: false,
        read_eof: false,
        tok: Token::Illegal,
        val: String::new(),
        offs: 0,
        line: 1,
        col: 1,
        pos: Pos::default(),
        quote: NO_STATE,
        eql_offs: 0,
        lang,
        forbid_nested: false,
        depth: 0,
        buried_hdocs: 0,
        heredocs: Vec::new(),
        hdoc_stops: Vec::new(),
        open_bquotes: 0,
        open_bquote_dbls: 0,
        last_bquote_esc: 0,
        rx_open_parens: 0,
        rx_first_part: false,
        acc_coms: Vec::new(),
        lit_bs: None,
    };
    p.rune();
    p.next()?;
    let (stmts, last) = p.stmt_list(&[])?;
    p.do_heredocs()?;
    Ok(File { stmts, last })
}

pub fn word_one(part: WordPart) -> Word {
    Word { parts: vec![part] }
}

pub fn valid_name(val: &str) -> bool {
    if val.is_empty() {
        return false;
    }
    val.chars()
        .enumerate()
        .all(|(i, c)| c.is_ascii_alphabetic() || c == '_' || (i > 0 && c.is_ascii_digit()))
}

pub fn number_literal(val: &str) -> bool {
    !val.is_empty() && val.chars().all(|c| c.is_ascii_digit())
}

fn is_bash_compound_command(tok: Token, val: &str) -> bool {
    match tok {
        Token::LeftParen | Token::DblLeftParen => true,
        Token::LitWord => matches!(
            val,
            "{" | "if"
                | "while"
                | "until"
                | "for"
                | "case"
                | "[["
                | "coproc"
                | "let"
                | "function"
                | "declare"
                | "local"
                | "export"
                | "readonly"
                | "typeset"
                | "nameref"
        ),
        _ => false,
    }
}

fn unquoted_word_part(buf: &mut Vec<u8>, wp: &WordPart, quotes: bool) -> bool {
    match wp {
        WordPart::Lit(lit) => {
            let mut quoted = false;
            let bytes = lit.value.as_bytes();
            let mut i = 0;
            while i < bytes.len() {
                let b = bytes[i];
                if b == b'\\' && !quotes {
                    i += 1;
                    if i < bytes.len() {
                        buf.push(bytes[i]);
                    }
                    quoted = true;
                } else {
                    buf.push(b);
                }
                i += 1;
            }
            quoted
        }
        WordPart::SglQuoted(q) => {
            buf.extend_from_slice(q.value.as_bytes());
            true
        }
        WordPart::DblQuoted(q) => {
            for part in &q.parts {
                unquoted_word_part(buf, part, true);
            }
            true
        }
        _ => false,
    }
}

fn unquoted_word_bytes(w: &Word) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut quoted = false;
    for part in &w.parts {
        quoted = unquoted_word_part(&mut buf, part, false);
    }
    (buf, quoted)
}

impl Parser<'_> {
    pub(super) fn lang_in(&self, set: u32) -> bool {
        self.lang & set == self.lang
    }

    pub(super) fn lit(&self, pos: Pos, value: String) -> Lit {
        Lit {
            value_pos: pos,
            value_end: self.next_pos(),
            value,
        }
    }

    pub(super) fn pos_err<T>(&self, pos: Pos, text: String) -> PResult<T> {
        Err(format!("{}: {}", pos.display(), text))
    }

    pub(super) fn cur_err<T>(&self, text: String) -> PResult<T> {
        self.pos_err(self.pos, text)
    }

    pub(super) fn check_lang(&self, pos: Pos, set: u32, feature: String) -> PResult<()> {
        if self.lang_in(set) {
            return Ok(());
        }
        let mut set = set;
        if set & LANG_BASH_LIKE == LANG_BASH_LIKE {
            set &= !LANG_BATS;
        }
        let langs: Vec<&str> = LANG_NAMES
            .iter()
            .filter(|(bit, _)| set & bit != 0)
            .map(|(_, name)| *name)
            .collect();
        let used = LANG_NAMES
            .iter()
            .find(|(bit, _)| *bit == self.lang)
            .map(|(_, name)| *name)
            .unwrap_or("unknown shell language variant");
        let verb = if feature.ends_with('s') { "are" } else { "is" };
        Err(format!(
            "{}: {} {} a {} feature; tried parsing as {}",
            pos.display(),
            feature,
            verb,
            langs.join("/"),
            used
        ))
    }

    pub(super) fn follow_err<T>(&self, pos: Pos, left: String, right: &str) -> PResult<T> {
        self.pos_err(pos, format!("{left} must be followed by {right}"))
    }

    pub(super) fn follow_err_exp<T>(&self, pos: Pos, left: String) -> PResult<T> {
        self.follow_err(pos, left, "an expression")
    }

    fn follow(&mut self, lpos: Pos, left: &str, tok: Token) -> PResult<()> {
        if !self.got(tok)? {
            return self.follow_err(lpos, quote(left), &tok.quoted());
        }
        Ok(())
    }

    fn follow_rsrv(&mut self, lpos: Pos, left: &str, val: &str) -> PResult<Pos> {
        let (pos, ok) = self.got_rsrv(val)?;
        if !ok {
            return self.follow_err(lpos, quote(left), &quote(val));
        }
        Ok(pos)
    }

    fn follow_stmts(
        &mut self,
        left: &str,
        lpos: Pos,
        stops: &[&str],
    ) -> PResult<(Vec<Stmt>, Vec<Comment>)> {
        if self.got(Token::Semicolon)? {
            return self.follow_err(lpos, quote(left), "a statement list");
        }
        let (stmts, last) = self.stmt_list(stops)?;
        if stmts.is_empty() {
            return self.follow_err(lpos, quote(left), "a statement list");
        }
        Ok((stmts, last))
    }

    fn follow_word_tok(&mut self, tok: Token, pos: Pos) -> PResult<Word> {
        match self.get_word()? {
            Some(w) => Ok(w),
            None => self.follow_err(pos, tok.quoted(), "a word"),
        }
    }

    fn stmt_end(&mut self, npos: Pos, start: &str, end: &str) -> PResult<Pos> {
        let (pos, ok) = self.got_rsrv(end)?;
        if !ok {
            return self.pos_err(
                npos,
                format!("{} statement must end with {}", quote(start), quote(end)),
            );
        }
        Ok(pos)
    }

    pub(super) fn quote_err<T>(&self, lpos: Pos, quote_tok: Token) -> PResult<T> {
        self.pos_err(
            lpos,
            format!(
                "reached {} without closing quote {}",
                self.tok.quoted(),
                quote_tok.quoted()
            ),
        )
    }

    pub(super) fn matching_err<T>(&self, lpos: Pos, left: Token, right: Token) -> PResult<T> {
        self.pos_err(
            lpos,
            format!(
                "reached {} without matching {} with {}",
                self.tok.quoted(),
                left.quoted(),
                right.quoted()
            ),
        )
    }

    pub(super) fn matched(&mut self, lpos: Pos, left: Token, right: Token) -> PResult<Pos> {
        let pos = self.pos;
        if !self.got(right)? {
            return self.matching_err(lpos, left, right);
        }
        Ok(pos)
    }

    pub(super) fn got(&mut self, tok: Token) -> PResult<bool> {
        if self.tok == tok {
            self.next()?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn got_rsrv(&mut self, val: &str) -> PResult<(Pos, bool)> {
        let pos = self.pos;
        if self.tok == Token::LitWord && self.val == val {
            self.next()?;
            return Ok((pos, true));
        }
        Ok((pos, false))
    }

    pub(super) fn deeper(&mut self) -> PResult<()> {
        if self.depth == MAX_DEPTH {
            return self.cur_err("nesting is too deep".into());
        }
        self.depth += 1;
        Ok(())
    }

    pub(super) fn nested<T>(&mut self, f: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<T> {
        self.deeper()?;
        let result = f(self);
        self.depth -= 1;
        result
    }

    pub(super) fn pre_nested(&mut self, quote: u32) -> SaveState {
        let s = SaveState {
            quote: self.quote,
            buried_hdocs: self.buried_hdocs,
        };
        self.buried_hdocs = self.heredocs.len();
        self.quote = quote;
        s
    }

    pub(super) fn post_nested(&mut self, s: SaveState) {
        self.quote = s.quote;
        self.buried_hdocs = s.buried_hdocs;
    }

    pub(super) fn do_heredocs(&mut self) -> PResult<()> {
        if self.heredocs.len() <= self.buried_hdocs {
            return Ok(());
        }
        self.rune();
        let old = self.quote;
        let hdocs = self.heredocs.split_off(self.buried_hdocs);
        for (i, r) in hdocs.iter().enumerate() {
            self.quote = if r.op == Token::DashHdoc {
                HDOC_BODY_TABS
            } else {
                HDOC_BODY
            };
            let (stop, quoted) = unquoted_word_bytes(&r.word);
            self.hdoc_stops.push(Some(stop));
            if i > 0 && self.r.is('\n') {
                self.rune();
            }
            let body = if quoted {
                self.quoted_hdoc_word()
            } else {
                self.next()?;
                self.get_word()?
            };
            *r.body.borrow_mut() = body;
            if let Some(Some(stop)) = self.hdoc_stops.last() {
                return self.pos_err(
                    r.pos,
                    format!(
                        "unclosed here-document {}",
                        quote(&String::from_utf8_lossy(stop))
                    ),
                );
            }
            self.hdoc_stops.pop();
        }
        self.quote = old;
        Ok(())
    }

    pub(super) fn stmt_list(&mut self, stops: &[&str]) -> PResult<(Vec<Stmt>, Vec<Comment>)> {
        let mut stmts = Vec::new();
        let mut got_end = true;
        while self.tok != Token::Eof {
            let new_line = self.got(Token::Newl)?;
            match self.tok {
                Token::LitWord => {
                    if stops.contains(&self.val.as_str()) {
                        break;
                    }
                    if self.val == "}" {
                        return self.cur_err(format!(
                            "{} can only be used to close a block",
                            Token::RightBrace.quoted()
                        ));
                    }
                }
                Token::RightParen => {
                    if self.quote == SUB_CMD {
                        break;
                    }
                }
                Token::BckQuote => {
                    if self.backquote_end() {
                        break;
                    }
                }
                Token::DblSemicolon | Token::SemiAnd | Token::DblSemiAnd => {
                    if self.quote == SWITCH_CASE {
                        break;
                    }
                    return self.cur_err(format!(
                        "{} can only be used in a case clause",
                        self.tok.quoted()
                    ));
                }
                _ => {}
            }
            if !new_line && !got_end {
                return self.cur_err("statements must be separated by &, ; or a newline".into());
            }
            if self.tok == Token::Eof {
                break;
            }
            let Some(s) = self.get_stmt(true, false, false)? else {
                return self.invalid_stmt_start();
            };
            got_end = s.semicolon.is_valid();
            stmts.push(s);
        }
        let mut split = self.acc_coms.len();
        if self.tok == Token::LitWord && matches!(self.val.as_str(), "elif" | "else" | "fi") {
            for i in (0..self.acc_coms.len()).rev() {
                if self.acc_coms[i].pos().col() != self.pos.col() {
                    break;
                }
                split = i;
            }
        }
        let last = self.acc_coms.drain(..split).collect();
        Ok((stmts, last))
    }

    fn invalid_stmt_start<T>(&self) -> PResult<T> {
        match self.tok {
            Token::Semicolon | Token::And | Token::Or | Token::AndAnd | Token::OrOr => self
                .cur_err(format!(
                    "{} can only immediately follow a statement",
                    self.tok.quoted()
                )),
            Token::RightParen => self.cur_err(format!(
                "{} can only be used to close a subshell",
                self.tok.quoted()
            )),
            _ => self.cur_err(format!(
                "{} is not a valid start for a statement",
                self.tok.quoted()
            )),
        }
    }

    pub(super) fn get_word(&mut self) -> PResult<Option<Word>> {
        let w = self.word_any_number()?;
        if w.parts.is_empty() {
            return Ok(None);
        }
        Ok(Some(w))
    }

    pub(super) fn get_lit(&mut self) -> PResult<Option<Lit>> {
        if self.tok.is_lit() {
            let l = self.lit(self.pos, self.val.clone());
            self.next()?;
            return Ok(Some(l));
        }
        Ok(None)
    }

    pub(super) fn word_any_number(&mut self) -> PResult<Word> {
        Ok(Word {
            parts: self.nested(Self::word_parts)?,
        })
    }

    fn word_parts(&mut self) -> PResult<Vec<WordPart>> {
        let reset = self.quote == NO_STATE;
        if reset {
            self.quote = UNQUOTED_WORD_CONT;
        }
        let mut wps = Vec::new();
        while let Some(part) = self.word_part()? {
            wps.push(part);
            if self.spaced {
                break;
            }
        }
        if reset {
            self.quote = NO_STATE;
        }
        Ok(wps)
    }

    fn ensure_no_nested(&self, pos: Pos) -> PResult<()> {
        if self.forbid_nested {
            return self.pos_err(pos, "expansions not allowed in heredoc words".into());
        }
        Ok(())
    }

    fn word_part(&mut self) -> PResult<Option<WordPart>> {
        match self.tok {
            Token::Lit | Token::LitWord | Token::LitRedir => {
                let l = self.lit(self.pos, self.val.clone());
                self.next()?;
                Ok(Some(WordPart::Lit(l)))
            }
            Token::DollBrace => {
                self.ensure_no_nested(self.pos)?;
                match self.r {
                    Rune::Ch('|' | ' ' | '\t' | '\n') => {
                        if self.r.is('|') {
                            self.check_lang(
                                self.pos,
                                LANG_BASH_LIKE | LANG_MKSH,
                                "`${|stmts;}`".into(),
                            )?;
                        }
                        self.check_lang(
                            self.pos,
                            LANG_BASH_LIKE | LANG_MKSH,
                            "`${ stmts;}`".into(),
                        )?;
                        let mut cs = CmdSubst {
                            left: self.pos,
                            temp_file: !self.r.is('|'),
                            reply_var: self.r.is('|'),
                            ..CmdSubst::default()
                        };
                        let old = self.pre_nested(SUB_CMD_BRACES);
                        self.rune();
                        self.next()?;
                        (cs.stmts, cs.last) = self.stmt_list(&["}"])?;
                        self.post_nested(old);
                        let (pos, ok) = self.got_rsrv("}")?;
                        if !ok {
                            return self.matching_err(cs.left, Token::DollBrace, Token::RightBrace);
                        }
                        cs.right = pos;
                        Ok(Some(WordPart::CmdSubst(cs)))
                    }
                    _ => Ok(self.param_exp()?.map(WordPart::ParamExp)),
                }
            }
            Token::DollDblParen | Token::DollBrack => {
                self.ensure_no_nested(self.pos)?;
                let left_tok = self.tok;
                let left = self.pos;
                let bracket = left_tok == Token::DollBrack;
                let old = self.pre_nested(ARITHM_EXPR);
                self.next()?;
                if self.got(Token::Hash)? {
                    self.check_lang(left, LANG_MKSH, "unsigned expressions".into())?;
                }
                let x = self.follow_arithm(left_tok, left)?;
                let right = if bracket {
                    if self.tok != Token::RightBrack {
                        return self.arithm_matching_err(left, Token::DollBrack, Token::RightBrack);
                    }
                    self.post_nested(old);
                    let right = self.pos;
                    self.next()?;
                    right
                } else {
                    self.arithm_end(Token::DollDblParen, left, old)?
                };
                Ok(Some(WordPart::ArithmExp(Box::new(ArithmExp {
                    left,
                    right,
                    bracket,
                    x,
                }))))
            }
            Token::DollParen => {
                self.ensure_no_nested(self.pos)?;
                Ok(Some(WordPart::CmdSubst(self.cmd_subst()?)))
            }
            Token::Dollar => match self.param_exp()? {
                None => {
                    let l = self.lit(self.pos, "$".into());
                    self.next()?;
                    Ok(Some(WordPart::Lit(l)))
                }
                Some(pe) => {
                    self.ensure_no_nested(pe.dollar)?;
                    Ok(Some(WordPart::ParamExp(pe)))
                }
            },
            Token::AssgnParen => {
                self.check_lang(
                    self.pos,
                    LANG_ZSH,
                    format!("{} process substitutions", self.tok.quoted()),
                )?;
                self.proc_subst().map(Some)
            }
            Token::CmdIn | Token::CmdOut => self.proc_subst().map(Some),
            Token::SglQuote | Token::DollSglQuote => {
                let left = self.pos;
                let dollar = self.tok == Token::DollSglQuote;
                let mut r = self.r;
                self.new_lit(r);
                loop {
                    match r {
                        Rune::Ch('\\') => {
                            if dollar {
                                self.rune();
                            }
                        }
                        Rune::Ch('\'') => {
                            let right = self.next_pos();
                            let value = self.end_lit();
                            self.rune();
                            self.next()?;
                            return Ok(Some(WordPart::SglQuoted(SglQuoted {
                                left,
                                right,
                                dollar,
                                value,
                            })));
                        }
                        Rune::EscNewl => {
                            if let Some(lit) = &mut self.lit_bs {
                                lit.extend_from_slice(b"\\\n");
                            }
                        }
                        Rune::Eof => {
                            self.tok = Token::Eof;
                            return self.quote_err(left, Token::SglQuote);
                        }
                        _ => {}
                    }
                    r = self.rune();
                }
            }
            Token::DblQuote | Token::DollDblQuote => {
                if self.quote == DBL_QUOTES {
                    return Ok(None);
                }
                Ok(Some(WordPart::DblQuoted(self.dbl_quoted()?)))
            }
            Token::BckQuote => {
                if self.backquote_end() {
                    return Ok(None);
                }
                self.ensure_no_nested(self.pos)?;
                let mut cs = CmdSubst {
                    left: self.pos,
                    backquotes: true,
                    ..CmdSubst::default()
                };
                let old = self.pre_nested(SUB_CMD_BCKQUO);
                self.open_bquotes += 1;
                if old.quote == DBL_QUOTES {
                    self.open_bquote_dbls += 1;
                }
                self.rune();
                self.next()?;
                (cs.stmts, cs.last) = self.stmt_list(&[])?;
                if self.tok == Token::BckQuote && self.last_bquote_esc + 1 < self.open_bquotes {
                    self.tok = Token::Eof;
                    return self.quote_err(cs.left, Token::BckQuote);
                }
                self.post_nested(old);
                self.open_bquotes -= 1;
                if old.quote == DBL_QUOTES {
                    self.open_bquote_dbls -= 1;
                }
                cs.right = self.pos;
                self.rune();
                if !self.got(Token::BckQuote)? {
                    return self.quote_err(cs.left, Token::BckQuote);
                }
                Ok(Some(WordPart::CmdSubst(cs)))
            }
            Token::GlobQuest
            | Token::GlobStar
            | Token::GlobPlus
            | Token::GlobAt
            | Token::GlobExcl => {
                self.check_lang(
                    self.pos,
                    LANG_BASH_LIKE | LANG_MKSH,
                    "extended globs".into(),
                )?;
                let op = self.tok;
                let op_pos = self.pos;
                let mut lparens = 1;
                let mut r = self.r;
                self.new_lit(r);
                loop {
                    match r {
                        Rune::Eof => break,
                        Rune::Ch('(') => lparens += 1,
                        Rune::Ch(')') => {
                            lparens -= 1;
                            if lparens == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    r = self.rune();
                }
                let value = self.end_lit();
                let pattern = self.lit(op_pos.add_col(2), value);
                self.rune();
                self.next()?;
                if lparens != 0 {
                    return self.matching_err(op_pos, op, Token::RightParen);
                }
                Ok(Some(WordPart::ExtGlob(ExtGlob {
                    op_pos,
                    op,
                    pattern,
                })))
            }
            _ => Ok(None),
        }
    }

    fn proc_subst(&mut self) -> PResult<WordPart> {
        self.ensure_no_nested(self.pos)?;
        let op = self.tok;
        let op_pos = self.pos;
        let old = self.pre_nested(SUB_CMD);
        self.next()?;
        let (stmts, last) = self.stmt_list(&[])?;
        self.post_nested(old);
        let rparen = self.matched(op_pos, op, Token::RightParen)?;
        Ok(WordPart::ProcSubst(ProcSubst {
            op_pos,
            rparen,
            op,
            stmts,
            last,
        }))
    }

    fn cmd_subst(&mut self) -> PResult<CmdSubst> {
        let mut cs = CmdSubst {
            left: self.pos,
            ..CmdSubst::default()
        };
        let old = self.pre_nested(SUB_CMD);
        self.next()?;
        (cs.stmts, cs.last) = self.stmt_list(&[])?;
        self.post_nested(old);
        cs.right = self.matched(cs.left, Token::DollParen, Token::RightParen)?;
        Ok(cs)
    }

    fn dbl_quoted(&mut self) -> PResult<DblQuoted> {
        let left = self.pos;
        let dollar = self.tok == Token::DollDblQuote;
        let old = self.quote;
        self.quote = DBL_QUOTES;
        self.next()?;
        let parts = self.nested(Self::word_parts)?;
        self.quote = old;
        let right = self.pos;
        if !self.got(Token::DblQuote)? {
            return self.quote_err(left, Token::DblQuote);
        }
        Ok(DblQuoted {
            left,
            right,
            dollar,
            parts,
        })
    }

    fn param_exp(&mut self) -> PResult<Option<Box<ParamExp>>> {
        let old = self.quote;
        self.quote = RUNE_BY_RUNE;
        let mut pe = Box::new(ParamExp {
            dollar: self.pos,
            short: self.tok == Token::Dollar,
            ..ParamExp::default()
        });
        if !pe.short && self.r.is('(') {
            self.check_lang(pe.pos(), LANG_ZSH, "parameter expansion flags".into())?;
        }
        if !pe.short {
            let r = self.r;
            match r {
                Rune::Ch('#') => {
                    if self.param_name_start(&pe) {
                        pe.length = true;
                    }
                }
                Rune::Ch('%') => {
                    if self.param_name_start(&pe) {
                        self.check_lang(pe.pos(), LANG_MKSH, "`${%foo}`".into())?;
                    }
                }
                Rune::Ch('!') => {
                    if self.param_name_start(&pe) {
                        self.check_lang(pe.pos(), LANG_BASH_LIKE | LANG_MKSH, "`${!foo}`".into())?;
                        pe.excl = true;
                    }
                }
                Rune::Ch('+') if self.param_name_start(&pe) => {
                    self.check_lang(pe.pos(), LANG_ZSH, "`${+foo}`".into())?;
                }
                _ => {}
            }
        }
        if !self.param_exp_parameter(&mut pe)? {
            self.quote = old;
            return Ok(None);
        }
        if pe.short {
            self.quote = old;
            self.next()?;
            return Ok(Some(pe));
        }
        if self.r.is('[') {
            self.check_lang(
                self.next_pos(),
                LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                "arrays".into(),
            )?;
            if let Some(param) = &pe.param
                && !valid_name(&param.value)
            {
                return self.pos_err(
                    self.next_pos(),
                    "cannot index a special parameter name".into(),
                );
            }
            self.pos = self.next_pos();
            self.rune();
            pe.index = Some(self.either_index()?);
        }
        let tok_rune = self.r;
        self.pos = self.next_pos();
        self.tok = self.param_token(self.r);
        if self.tok == Token::RightBrace {
            pe.rbrace = self.pos;
            self.quote = old;
            self.next()?;
            return Ok(Some(pe));
        }
        if self.tok != Token::Eof && pe.length {
            return self.cur_err("cannot combine multiple parameter expansion operators".into());
        }
        match self.tok {
            Token::Slash | Token::DblSlash => {
                self.check_lang(
                    self.pos,
                    LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                    "search and replace".into(),
                )?;
                let all = self.tok == Token::DblSlash;
                self.quote = PARAM_EXP_REPL;
                self.next()?;
                let orig = self.get_word()?;
                self.quote = PARAM_EXP_EXP;
                let mut with = None;
                if self.got(Token::Slash)? {
                    with = self.get_word()?;
                }
                pe.repl = Some(Box::new(Replace { all, orig, with }));
            }
            Token::Colon => {
                self.check_lang(
                    self.pos,
                    LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                    "slicing".into(),
                )?;
                let mut colon_pos = self.pos;
                self.quote = PARAM_EXP_SLICE;
                self.next()?;
                let mut offset = None;
                if self.tok != Token::Colon {
                    offset = Some(self.follow_arithm(Token::Colon, colon_pos)?);
                }
                colon_pos = self.pos;
                let mut length = None;
                if self.got(Token::Colon)? {
                    length = Some(self.follow_arithm(Token::Colon, colon_pos)?);
                }
                pe.slice = Some(Box::new(Slice { offset, length }));
                self.quote = old;
                pe.rbrace = self.pos;
                self.matched_arithm(pe.dollar, Token::DollBrace, Token::RightBrace)?;
                return Ok(Some(pe));
            }
            Token::Caret | Token::DblCaret | Token::Comma | Token::DblComma => {
                self.check_lang(self.pos, LANG_BASH_LIKE, "this expansion operator".into())?;
                pe.exp = Some(self.param_exp_exp()?);
            }
            Token::At | Token::Star => {
                if self.tok == Token::Star && !pe.excl {
                    return self.cur_err(format!(
                        "not a valid parameter expansion operator: {}",
                        self.tok.quoted()
                    ));
                }
                if pe.excl && self.r.is('}') {
                    self.check_lang(
                        pe.pos(),
                        LANG_BASH_LIKE,
                        format!("`${{!foo{}}}`", self.tok.text()),
                    )?;
                    pe.names = Some(self.tok);
                    self.next()?;
                } else {
                    if self.tok == Token::At {
                        self.check_lang(
                            self.pos,
                            LANG_BASH_LIKE | LANG_MKSH,
                            "this expansion operator".into(),
                        )?;
                    }
                    pe.exp = Some(self.param_exp_exp()?);
                }
            }
            Token::Plus
            | Token::ColPlus
            | Token::Minus
            | Token::ColMinus
            | Token::Quest
            | Token::ColQuest
            | Token::Assgn
            | Token::ColAssgn
            | Token::Perc
            | Token::DblPerc
            | Token::Hash
            | Token::DblHash
            | Token::ColHash
            | Token::ColPipe
            | Token::ColStar => {
                pe.exp = Some(self.param_exp_exp()?);
            }
            Token::Eof => {}
            _ => {
                if let Rune::Ch(c) = tok_rune
                    && param_name_rune(c)
                {
                    return match &pe.param {
                        Some(param) => self.cur_err(format!(
                            "{} cannot be followed by a word",
                            quote(&param.value)
                        )),
                        None => self.cur_err(
                            "nested parameter expansion cannot be followed by a word".into(),
                        ),
                    };
                }
                let text = match tok_rune {
                    Rune::Ch(c) => c.to_string(),
                    _ => "\u{FFFD}".to_string(),
                };
                return self.cur_err(format!(
                    "not a valid parameter expansion operator: {}",
                    quote(&text)
                ));
            }
        }
        if self.tok != Token::Eof && self.tok != Token::RightBrace {
            self.tok = self.param_token(self.r);
        }
        self.quote = old;
        pe.rbrace = self.matched(pe.dollar, Token::DollBrace, Token::RightBrace)?;
        Ok(Some(pe))
    }

    fn param_name_start(&mut self, pe: &ParamExp) -> bool {
        let r = self.peek();
        if param_name_start_rune(r as char) || (!pe.short && (r == b'"' || r == 0x80)) {
            self.rune();
            return true;
        }
        false
    }

    fn param_exp_parameter(&mut self, pe: &mut ParamExp) -> PResult<bool> {
        if !pe.short {
            let mut quote_pos = Pos::default();
            if self.r.is('"') {
                quote_pos = self.next_pos();
                self.rune();
            }
            if !self.r.is('$') {
                if quote_pos.is_valid() {
                    return self.pos_err(pe.pos(), "invalid nested parameter expansion".into());
                }
            } else {
                let p1 = self.peek();
                if p1 == b'{' || p1 == b'(' {
                    self.pos = self.next_pos();
                    self.check_lang(self.pos, LANG_ZSH, "nested parameter expansions".into())?;
                }
            }
        }
        let r = self.r;
        match r {
            Rune::Ch('?' | '-') if pe.length && self.peek() != b'}' => {
                pe.length = false;
                let pos = self.next_pos();
                let mut l = self.lit(pos.add_col(-1), "#".into());
                l.value_end = pos;
                pe.param = Some(l);
            }
            Rune::Ch(c @ ('?' | '-' | '@' | '*' | '#' | '!' | '$')) => {
                let pos = self.next_pos();
                self.rune();
                pe.param = Some(self.lit(pos, c.to_string()));
            }
            _ => {
                let pos = self.next_pos();
                match self.r {
                    Rune::Ch(c) if pe.short && single_rune_param(c) => {
                        self.val = c.to_string();
                        self.rune();
                    }
                    _ => {
                        self.new_lit(self.r);
                        while self.r != Rune::Eof {
                            let name_rune = matches!(self.r, Rune::Ch(c) if param_name_rune(c));
                            if !name_rune && self.r != Rune::EscNewl {
                                break;
                            }
                            self.rune();
                        }
                        self.val = self.end_lit();
                        if !number_literal(&self.val) && !valid_name(&self.val) {
                            if pe.short {
                                return Ok(false);
                            }
                            return self.pos_err(pos, "invalid parameter name".into());
                        }
                    }
                }
                pe.param = Some(self.lit(pos, self.val.clone()));
            }
        }
        Ok(true)
    }

    fn param_exp_exp(&mut self) -> PResult<Box<Expansion>> {
        let op = self.tok;
        if matches!(op, Token::ColHash | Token::ColPipe | Token::ColStar) {
            self.check_lang(self.pos, LANG_ZSH, format!("${{name{}arg}}", op.text()))?;
        }
        self.quote = PARAM_EXP_EXP;
        self.next()?;
        if op == Token::At {
            if !self.tok.is_lit() {
                return self.cur_err("@ expansion operator requires a literal".into());
            }
            match self.val.as_str() {
                "a" | "k" | "u" | "A" | "E" | "K" | "L" | "P" | "U" => {
                    self.check_lang(self.pos, LANG_BASH_LIKE, "this expansion operator".into())?;
                }
                "#" => {
                    self.check_lang(self.pos, LANG_MKSH, "this expansion operator".into())?;
                }
                "Q" => {}
                _ => {
                    return self
                        .cur_err(format!("invalid @ expansion operator {}", quote(&self.val)));
                }
            }
        }
        Ok(Box::new(Expansion {
            op,
            word: self.get_word()?,
        }))
    }

    pub(super) fn either_index(&mut self) -> PResult<ArithmExpr> {
        let old = self.quote;
        let lpos = self.pos;
        self.quote = PARAM_EXP_ARITHM;
        self.next()?;
        if self.tok == Token::Star || self.tok == Token::At {
            self.val = self.tok.text().to_string();
            self.tok = Token::LitWord;
        }
        let expr = self.follow_arithm(Token::LeftBrack, lpos)?;
        self.quote = old;
        self.matched_arithm(lpos, Token::LeftBrack, Token::RightBrack)?;
        Ok(expr)
    }

    pub(super) fn stop_token(&self) -> bool {
        match self.tok {
            Token::Eof
            | Token::Newl
            | Token::Semicolon
            | Token::And
            | Token::Or
            | Token::AndAnd
            | Token::OrOr
            | Token::OrAnd
            | Token::DblSemicolon
            | Token::SemiAnd
            | Token::DblSemiAnd
            | Token::RightParen => true,
            Token::BckQuote => self.backquote_end(),
            _ => false,
        }
    }

    pub(super) fn backquote_end(&self) -> bool {
        self.last_bquote_esc < self.open_bquotes
    }

    fn has_valid_ident(&self) -> bool {
        if !self.tok.is_lit() {
            return false;
        }
        if self.eql_offs > 0 {
            let mut end = self.eql_offs as usize;
            if self.val.as_bytes().get(end - 1) == Some(&b'+')
                && self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH)
            {
                end -= 1;
            }
            if self.val.get(..end).is_some_and(valid_name) {
                return true;
            }
        } else if !valid_name(&self.val) {
            return false;
        }
        self.r.is('[')
    }

    fn get_assign(&mut self, need_equal: bool) -> PResult<Assign> {
        let mut a = Assign::default();
        if self.eql_offs > 0 {
            let eql = self.eql_offs as usize;
            let mut name_end = eql;
            if self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH)
                && self.val.as_bytes().get(eql - 1) == Some(&b'+')
            {
                a.append = true;
                name_end -= 1;
            }
            let name_value = self.val.get(..name_end).unwrap_or_default().to_string();
            let mut name = self.lit(self.pos, name_value);
            name.value_end = name.value_pos.add_col(name_end as i64);
            a.name = Some(name);
            let rest = self.val.get(eql + 1..).unwrap_or_default().to_string();
            let mut left = self.lit(self.pos.add_col(1), rest);
            if !left.value.is_empty() {
                left.value_pos = left.value_pos.add_col(eql as i64);
                a.value = Some(word_one(WordPart::Lit(left)));
            }
            self.next()?;
        } else {
            a.name = Some(self.lit(self.pos, self.val.clone()));
            self.rune();
            self.pos = self.pos.add_col(1);
            a.index = Some(self.either_index()?);
            if self.spaced || self.stop_token() {
                if need_equal {
                    return self.follow_err(a.pos(), quote("a[b]"), &Token::Assgn.quoted());
                }
                a.naked = true;
                return Ok(a);
            }
            if self.tok == Token::AssgnParen {
                return self.cur_err("arrays cannot be nested".into());
            }
            if self.val.starts_with('+') {
                a.append = true;
                self.val.remove(0);
                self.pos = self.pos.add_col(1);
            }
            if !self.val.starts_with('=') {
                let left = if a.append { "a[b]+" } else { "a[b]" };
                return self.follow_err(a.pos(), quote(left), &Token::Assgn.quoted());
            }
            self.pos = self.pos.add_col(1);
            self.val.remove(0);
            if self.val.is_empty() {
                self.next()?;
            }
        }
        if self.spaced || self.stop_token() {
            return Ok(a);
        }
        if a.value.is_none() && self.tok == Token::LeftParen {
            self.check_lang(
                self.pos,
                LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                "arrays".into(),
            )?;
            let lparen = self.pos;
            let new_quote = if self.lang_in(LANG_BASH_LIKE | LANG_ZSH) {
                ARRAY_ELEMS
            } else {
                self.quote
            };
            let old = self.pre_nested(new_quote);
            self.next()?;
            self.got(Token::Newl)?;
            let mut elems = Vec::new();
            while self.tok != Token::Eof && self.tok != Token::RightParen {
                let mut ae = ArrayElem {
                    comments: take(&mut self.acc_coms),
                    ..ArrayElem::default()
                };
                if self.tok == Token::LeftBrack {
                    let left = self.pos;
                    ae.index = Some(self.either_index()?);
                    if self.tok == Token::AssgnParen {
                        return self.cur_err("arrays cannot be nested".into());
                    }
                    self.follow(left, "[x]", Token::Assgn)?;
                }
                ae.value = self.get_word()?;
                if ae.value.is_none()
                    && !matches!(self.tok, Token::Newl | Token::RightParen | Token::LeftBrack)
                {
                    return self.cur_err("array element values must be words".into());
                }
                if self
                    .acc_coms
                    .first()
                    .is_some_and(|c| c.pos().line() == ae.end().line())
                {
                    let c = self.acc_coms.remove(0);
                    ae.comments.push(c);
                }
                elems.push(ae);
                self.got(Token::Newl)?;
            }
            let last = take(&mut self.acc_coms);
            self.post_nested(old);
            let rparen = self.matched(lparen, Token::LeftParen, Token::RightParen)?;
            a.array = Some(ArrayExpr {
                rparen,
                elems,
                last,
            });
        } else if let Some(w) = self.get_word()? {
            match &mut a.value {
                None => a.value = Some(w),
                Some(value) => value.parts.extend(w.parts),
            }
        }
        Ok(a)
    }

    pub(super) fn peek_redir(&self) -> bool {
        matches!(
            self.tok,
            Token::LitRedir
                | Token::RdrOut
                | Token::AppOut
                | Token::RdrIn
                | Token::RdrInOut
                | Token::DplIn
                | Token::DplOut
                | Token::RdrClob
                | Token::AppClob
                | Token::Hdoc
                | Token::DashHdoc
                | Token::WordHdoc
                | Token::RdrAll
                | Token::RdrAllClob
                | Token::AppAll
                | Token::AppAllClob
        )
    }

    fn do_redirect(&mut self, s: &mut Stmt) -> PResult<()> {
        let n = self.get_lit()?;
        if let Some(n) = &n
            && n.value.starts_with('{')
        {
            self.check_lang(
                n.pos(),
                LANG_BASH_LIKE | LANG_ZSH,
                "`{varname}` redirects".into(),
            )?;
        }
        let op = self.tok;
        let op_pos = self.pos;
        match op {
            Token::RdrAll | Token::AppAll => {
                self.check_lang(
                    self.pos,
                    LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                    format!("{} redirects", op.quoted()),
                )?;
            }
            Token::AppClob | Token::RdrAllClob | Token::AppAllClob => {
                self.check_lang(self.pos, LANG_ZSH, format!("{} redirects", op.quoted()))?;
            }
            _ => {}
        }
        self.next()?;
        let body = Rc::new(RefCell::new(None));
        match op {
            Token::Hdoc | Token::DashHdoc => {
                let old = self.quote;
                self.quote = HDOC_WORD;
                self.forbid_nested = true;
                let word = self.follow_word_tok(op, op_pos)?;
                self.quote = old;
                self.forbid_nested = false;
                let r = Redirect {
                    op_pos,
                    op,
                    n,
                    word: word.clone(),
                    hdoc: body.clone(),
                };
                self.heredocs.push(PendingHdoc {
                    op,
                    word,
                    pos: r.pos(),
                    body,
                });
                s.redirs.push(r);
                if self.tok == Token::Newl {
                    if self
                        .acc_coms
                        .first()
                        .is_some_and(|c| c.pos().line() == s.end().line())
                    {
                        let c = self.acc_coms.remove(0);
                        s.comments.push(c);
                    }
                    self.do_heredocs()?;
                }
            }
            _ => {
                if op == Token::WordHdoc {
                    self.check_lang(
                        op_pos,
                        LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH,
                        "herestrings".into(),
                    )?;
                }
                let word = self.follow_word_tok(op, op_pos)?;
                s.redirs.push(Redirect {
                    op_pos,
                    op,
                    n,
                    word,
                    hdoc: body,
                });
            }
        }
        Ok(())
    }

    fn get_stmt(&mut self, read_end: bool, bin_cmd: bool, fn_body: bool) -> PResult<Option<Stmt>> {
        let (pos, ok) = self.got_rsrv("!")?;
        let mut s = Stmt::at(pos);
        if ok {
            s.negated = true;
            if self.stop_token() {
                return self.pos_err(
                    s.pos(),
                    format!("{} cannot form a statement alone", Token::ExclMark.quoted()),
                );
            }
            if self.got_rsrv("!")?.1 {
                return self.pos_err(s.pos(), "cannot negate a command multiple times".into());
            }
        }
        let Some(mut s) = self.nested(|p| p.got_stmt_pipe(s, false))? else {
            return Ok(None);
        };
        let depth = self.depth;
        while self.tok == Token::AndAnd || self.tok == Token::OrOr {
            if bin_cmd {
                return Ok(Some(s));
            }
            self.deeper()?;
            let op_pos = self.pos;
            let op = self.tok;
            self.next()?;
            self.got(Token::Newl)?;
            let Some(y) = self.get_stmt(false, true, false)? else {
                return self.follow_err(op_pos, op.quoted(), "a statement");
            };
            let mut x = s;
            s = Stmt::at(x.position);
            s.comments = take(&mut x.comments);
            s.cmd = Some(Command::Binary(Box::new(BinaryCmd { op_pos, op, x, y })));
        }
        self.depth = depth;
        if read_end {
            match self.tok {
                Token::Semicolon => {
                    s.semicolon = self.pos;
                    self.next()?;
                }
                Token::And => {
                    s.semicolon = self.pos;
                    self.next()?;
                    s.background = true;
                }
                _ => {}
            }
        }
        if !bin_cmd
            && !fn_body
            && self
                .acc_coms
                .first()
                .is_some_and(|c| c.pos().line() == s.end().line())
        {
            let c = self.acc_coms.remove(0);
            s.comments.push(c);
        }
        Ok(Some(s))
    }

    fn got_stmt_pipe(&mut self, mut s: Stmt, bin_cmd: bool) -> PResult<Option<Stmt>> {
        s.comments = take(&mut self.acc_coms);
        while self.peek_redir() {
            self.do_redirect(&mut s)?;
        }
        let redirs_start = s.redirs.len();
        match self.tok {
            Token::LitWord => {
                let full = LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH;
                match self.val.as_str() {
                    "{" => self.block(&mut s)?,
                    "if" => self.if_clause(&mut s)?,
                    "while" => self.while_clause(&mut s, false)?,
                    "until" => self.while_clause(&mut s, true)?,
                    "for" => self.for_clause(&mut s)?,
                    "case" => self.case_clause(&mut s)?,
                    "}" => {
                        return self.cur_err(format!(
                            "{} can only be used to close a block",
                            Token::RightBrace.quoted()
                        ));
                    }
                    "then" | "elif" => {
                        return self
                            .cur_err(format!("{} can only be used in an `if`", quote(&self.val)));
                    }
                    "fi" => {
                        return self.cur_err(format!(
                            "{} can only be used to end an `if`",
                            quote(&self.val)
                        ));
                    }
                    "do" => {
                        return self
                            .cur_err(format!("{} can only be used in a loop", quote(&self.val)));
                    }
                    "done" => {
                        return self.cur_err(format!(
                            "{} can only be used to end a loop",
                            quote(&self.val)
                        ));
                    }
                    "esac" => {
                        return self.cur_err(format!(
                            "{} can only be used to end a `case`",
                            quote(&self.val)
                        ));
                    }
                    "!" => {
                        if !s.negated {
                            return self.cur_err(format!(
                                "{} can only be used in full statements",
                                Token::ExclMark.quoted()
                            ));
                        }
                    }
                    "[[" if self.lang_in(full) => self.test_clause(&mut s)?,
                    "]]" if self.lang_in(full) => {
                        return self.cur_err(format!(
                            "{} can only be used to close a test",
                            Token::DblRightBrack.quoted()
                        ));
                    }
                    "let" if self.lang_in(full) => self.let_clause(&mut s)?,
                    "function" if self.lang_in(full) => self.bash_func_decl(&mut s)?,
                    "declare" if self.lang_in(LANG_BASH_LIKE | LANG_ZSH) => {
                        self.decl_clause(&mut s)?
                    }
                    "local" | "export" | "readonly" | "typeset" | "nameref"
                        if self.lang_in(full) =>
                    {
                        self.decl_clause(&mut s)?
                    }
                    "time" if self.lang_in(full) => self.time_clause(&mut s)?,
                    "coproc" if self.lang_in(LANG_BASH_LIKE) => self.coproc_clause(&mut s)?,
                    "select" if self.lang_in(full) => self.select_clause(&mut s)?,
                    _ => {}
                }
                if s.cmd.is_none() {
                    if self.has_valid_ident() {
                        self.call_expr(&mut s, None, true)?;
                    } else {
                        let name = self.lit(self.pos, self.val.clone());
                        self.next()?;
                        if self.tok == Token::LeftParen {
                            self.next()?;
                            self.follow(name.value_pos, "foo(", Token::RightParen)?;
                            if self.lang_in(LANG_POSIX) && !valid_name(&name.value) {
                                return self.pos_err(name.pos(), "invalid func name".into());
                            }
                            let pos = name.value_pos;
                            self.func_decl(&mut s, pos, false, true, Some(name))?;
                        } else {
                            let w = word_one(WordPart::Lit(name));
                            self.call_expr(&mut s, Some(w), false)?;
                        }
                    }
                }
            }
            Token::BckQuote
            | Token::Lit
            | Token::DollBrace
            | Token::DollDblParen
            | Token::DollParen
            | Token::Dollar
            | Token::CmdIn
            | Token::AssgnParen
            | Token::CmdOut
            | Token::SglQuote
            | Token::DollSglQuote
            | Token::DblQuote
            | Token::DollDblQuote
            | Token::DollBrack
            | Token::GlobQuest
            | Token::GlobStar
            | Token::GlobPlus
            | Token::GlobAt
            | Token::GlobExcl => {
                if self.tok == Token::BckQuote && self.backquote_end() {
                } else if self.has_valid_ident() {
                    self.call_expr(&mut s, None, true)?;
                } else {
                    let w = self.word_any_number()?;
                    if self.got(Token::LeftParen)? {
                        return self.pos_err(w.pos(), "invalid func name".into());
                    }
                    self.call_expr(&mut s, Some(w), false)?;
                }
            }
            Token::LeftParen => {
                if self.r.is(')') {
                    self.rune();
                    let fpos = self.pos;
                    self.next()?;
                    if self.tok == Token::LitWord && self.val == "{" {
                        self.check_lang(fpos, LANG_ZSH, "anonymous functions".into())?;
                    }
                    self.func_decl(&mut s, fpos, false, true, None)?;
                } else {
                    self.subshell(&mut s)?;
                }
            }
            Token::DblLeftParen => self.arithm_exp_cmd(&mut s)?,
            _ => {}
        }
        if s.cmd.is_none() && s.redirs.is_empty() {
            return Ok(None);
        }
        if redirs_start > 0 && s.cmd.is_some() && !matches!(s.cmd, Some(Command::Call(_))) {
            self.check_lang(
                s.pos(),
                LANG_ZSH,
                "redirects before compound commands".into(),
            )?;
        }
        while self.peek_redir() {
            self.do_redirect(&mut s)?;
        }
        let depth = self.depth;
        while self.tok == Token::Or || self.tok == Token::OrAnd {
            if bin_cmd {
                return Ok(Some(s));
            }
            self.deeper()?;
            let op_pos = self.pos;
            let op = self.tok;
            self.next()?;
            self.got(Token::Newl)?;
            let Some(y) = self.got_stmt_pipe(Stmt::at(self.pos), true)? else {
                return self.follow_err(op_pos, op.quoted(), "a statement");
            };
            let mut x = s;
            s = Stmt::at(x.position);
            s.comments = take(&mut x.comments);
            s.negated = x.negated;
            x.negated = false;
            s.cmd = Some(Command::Binary(Box::new(BinaryCmd { op_pos, op, x, y })));
        }
        self.depth = depth;
        Ok(Some(s))
    }

    fn subshell(&mut self, s: &mut Stmt) -> PResult<()> {
        let lparen = self.pos;
        let old = self.pre_nested(SUB_CMD);
        self.next()?;
        let (stmts, last) = self.follow_stmts("(", lparen, &[])?;
        self.post_nested(old);
        let rparen = self.matched(lparen, Token::LeftParen, Token::RightParen)?;
        s.cmd = Some(Command::Subshell(Subshell {
            lparen,
            rparen,
            stmts,
            last,
        }));
        Ok(())
    }

    fn arithm_exp_cmd(&mut self, s: &mut Stmt) -> PResult<()> {
        let left = self.pos;
        let old = self.pre_nested(ARITHM_EXPR_CMD);
        self.next()?;
        if self.got(Token::Hash)? {
            self.check_lang(left, LANG_MKSH, "unsigned expressions".into())?;
        }
        let x = self.follow_arithm(Token::DblLeftParen, left)?;
        let right = self.arithm_end(Token::DblLeftParen, left, old)?;
        s.cmd = Some(Command::Arithm(ArithmCmd { left, right, x }));
        Ok(())
    }

    fn block(&mut self, s: &mut Stmt) -> PResult<()> {
        let lbrace = self.pos;
        self.next()?;
        let (stmts, last) = self.follow_stmts("{", lbrace, &["}"])?;
        let (rbrace, ok) = self.got_rsrv("}")?;
        if !ok {
            return self.matching_err(lbrace, Token::LeftBrace, Token::RightBrace);
        }
        s.cmd = Some(Command::Block(Block {
            lbrace,
            rbrace,
            stmts,
            last,
        }));
        Ok(())
    }

    fn if_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let mut root = IfClause {
            position: self.pos,
            ..IfClause::default()
        };
        self.next()?;
        (root.cond, root.cond_last) = self.follow_stmts("if", root.position, &["then"])?;
        root.then_pos = self.follow_rsrv(root.position, "if <cond>", "then")?;
        (root.then, root.then_last) =
            self.follow_stmts("then", root.then_pos, &["fi", "elif", "else"])?;
        let mut chain = vec![root];
        let depth = self.depth;
        while self.tok == Token::LitWord && self.val == "elif" {
            self.deeper()?;
            let mut elf = IfClause {
                position: self.pos,
                ..IfClause::default()
            };
            if let Some(cur) = chain.last_mut() {
                cur.last = take(&mut self.acc_coms);
            }
            self.next()?;
            (elf.cond, elf.cond_last) = self.follow_stmts("elif", elf.position, &["then"])?;
            elf.then_pos = self.follow_rsrv(elf.position, "elif <cond>", "then")?;
            (elf.then, elf.then_last) =
                self.follow_stmts("then", elf.then_pos, &["fi", "elif", "else"])?;
            chain.push(elf);
        }
        self.depth = depth;
        let (else_pos, ok) = self.got_rsrv("else")?;
        if ok {
            if let Some(cur) = chain.last_mut() {
                cur.last = take(&mut self.acc_coms);
            }
            let mut els = IfClause {
                position: else_pos,
                ..IfClause::default()
            };
            (els.then, els.then_last) = self.follow_stmts("else", els.position, &["fi"])?;
            chain.push(els);
        }
        if let Some(cur) = chain.last_mut() {
            cur.last = take(&mut self.acc_coms);
        }
        let fi_pos = self.stmt_end(chain[0].position, "if", "fi")?;
        let mut nested: Option<Box<IfClause>> = None;
        while let Some(mut ic) = chain.pop() {
            ic.fi_pos = fi_pos;
            ic.else_ = nested;
            nested = Some(Box::new(ic));
        }
        s.cmd = nested.map(Command::If);
        Ok(())
    }

    fn while_clause(&mut self, s: &mut Stmt, until: bool) -> PResult<()> {
        let while_pos = self.pos;
        let rsrv = if until { "until" } else { "while" };
        let rsrv_cond = format!("{rsrv} <cond>");
        self.next()?;
        let (cond, cond_last) = self.follow_stmts(rsrv, while_pos, &["do"])?;
        let do_pos = self.follow_rsrv(while_pos, &rsrv_cond, "do")?;
        let (do_, do_last) = self.follow_stmts("do", do_pos, &["done"])?;
        let done_pos = self.stmt_end(while_pos, rsrv, "done")?;
        s.cmd = Some(Command::While(WhileClause {
            while_pos,
            do_pos,
            done_pos,
            until,
            cond,
            cond_last,
            do_,
            do_last,
        }));
        Ok(())
    }

    fn for_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let for_pos = self.pos;
        self.next()?;
        let loop_ = self.loop_(for_pos)?;
        let (start, end, do_pos, braces);
        let (pos, ok) = self.got_rsrv("{")?;
        if ok {
            self.check_lang(
                pos,
                LANG_BASH_LIKE | LANG_MKSH,
                "for loops with braces".into(),
            )?;
            do_pos = pos;
            braces = true;
            start = "{";
            end = "}";
        } else {
            do_pos = self.follow_rsrv(for_pos, "for foo [in words]", "do")?;
            braces = false;
            start = "do";
            end = "done";
        }
        s.comments.append(&mut self.acc_coms);
        let (do_, do_last) = self.follow_stmts(start, do_pos, &[end])?;
        let done_pos = self.stmt_end(for_pos, "for", end)?;
        s.cmd = Some(Command::For(Box::new(ForClause {
            for_pos,
            do_pos,
            done_pos,
            select: false,
            braces,
            loop_,
            do_,
            do_last,
        })));
        Ok(())
    }

    fn loop_(&mut self, fpos: Pos) -> PResult<Loop> {
        if self.tok == Token::LeftParen || self.tok == Token::DblLeftParen {
            self.check_lang(self.pos, LANG_BASH_LIKE | LANG_ZSH, "c-style fors".into())?;
        }
        if self.tok == Token::DblLeftParen {
            let lparen = self.pos;
            let old = self.pre_nested(ARITHM_EXPR_CMD);
            self.next()?;
            let init = self.arithm_expr(false)?;
            let mut cond = None;
            if !self.got(Token::DblSemicolon)? {
                self.follow(self.pos, "expr", Token::Semicolon)?;
                cond = self.arithm_expr(false)?;
                self.follow(self.pos, "expr", Token::Semicolon)?;
            }
            let post = self.arithm_expr(false)?;
            self.arithm_end(Token::DblLeftParen, lparen, old)?;
            self.got(Token::Semicolon)?;
            self.got(Token::Newl)?;
            return Ok(Loop::CStyle(CStyleLoop { init, cond, post }));
        }
        Ok(Loop::WordIter(self.word_iter("for", fpos)?))
    }

    fn word_iter(&mut self, ftok: &str, fpos: Pos) -> PResult<WordIter> {
        let Some(name) = self.get_lit()? else {
            return self.follow_err(fpos, quote(ftok), "a literal");
        };
        let mut wi = WordIter {
            name,
            in_pos: Pos::default(),
            items: Vec::new(),
        };
        if self.got(Token::Semicolon)? {
            self.got(Token::Newl)?;
            return Ok(wi);
        }
        self.got(Token::Newl)?;
        let (pos, ok) = self.got_rsrv("in")?;
        if ok {
            wi.in_pos = pos;
            while !self.stop_token() {
                match self.get_word()? {
                    Some(w) => wi.items.push(w),
                    None => return self.cur_err("word list can only contain words".into()),
                }
            }
            self.got(Token::Semicolon)?;
            self.got(Token::Newl)?;
        } else if self.tok == Token::LitWord && self.val == "do" {
        } else {
            return self.follow_err(
                fpos,
                quote(&format!("{ftok} foo")),
                "`in`, `do`, `;`, or a newline",
            );
        }
        Ok(wi)
    }

    fn select_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let for_pos = self.pos;
        self.next()?;
        let wi = self.word_iter("select", for_pos)?;
        let do_pos = self.follow_rsrv(for_pos, "select foo [in words]", "do")?;
        let (do_, do_last) = self.follow_stmts("do", do_pos, &["done"])?;
        let done_pos = self.stmt_end(for_pos, "select", "done")?;
        s.cmd = Some(Command::For(Box::new(ForClause {
            for_pos,
            do_pos,
            done_pos,
            select: true,
            braces: false,
            loop_: Loop::WordIter(wi),
            do_,
            do_last,
        })));
        Ok(())
    }

    fn case_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let case = self.pos;
        self.next()?;
        let Some(word) = self.get_word()? else {
            return self.follow_err(case, quote("case"), "a word");
        };
        self.got(Token::Newl)?;
        let (_, ok) = self.got_rsrv("{")?;
        if ok {
            self.check_lang(case, LANG_MKSH, "`case i {`".into())?;
        }
        let in_ = self.follow_rsrv(case, "case x", "in")?;
        let items = self.case_items("esac")?;
        let last = take(&mut self.acc_coms);
        let esac = self.stmt_end(case, "case", "esac")?;
        s.cmd = Some(Command::Case(CaseClause {
            case,
            in_,
            esac,
            word,
            items,
            last,
        }));
        Ok(())
    }

    fn case_items(&mut self, stop: &str) -> PResult<Vec<CaseItem>> {
        self.got(Token::Newl)?;
        let mut items = Vec::new();
        while self.tok != Token::Eof && !(self.tok == Token::LitWord && self.val == stop) {
            let mut ci = CaseItem {
                op: Token::DblSemicolon,
                op_pos: Pos::default(),
                comments: take(&mut self.acc_coms),
                patterns: Vec::new(),
                stmts: Vec::new(),
                last: Vec::new(),
            };
            self.got(Token::LeftParen)?;
            while self.tok != Token::Eof {
                match self.get_word()? {
                    Some(w) => ci.patterns.push(w),
                    None => {
                        return self.cur_err("case patterns must consist of words".into());
                    }
                }
                if self.tok == Token::RightParen {
                    break;
                }
                if !self.got(Token::Or)? {
                    return self.cur_err(format!(
                        "case patterns must be separated with {}",
                        Token::Or.quoted()
                    ));
                }
            }
            let old = self.pre_nested(SWITCH_CASE);
            self.next()?;
            (ci.stmts, ci.last) = self.stmt_list(&[stop])?;
            self.post_nested(old);
            if !matches!(
                self.tok,
                Token::DblSemicolon | Token::SemiAnd | Token::DblSemiAnd
            ) {
                items.push(ci);
                return Ok(items);
            }
            ci.last.append(&mut self.acc_coms);
            ci.op_pos = self.pos;
            ci.op = self.tok;
            self.next()?;
            self.got(Token::Newl)?;
            let mut split = self.acc_coms.len();
            for i in (0..self.acc_coms.len()).rev() {
                if self.acc_coms[i].pos().col() != self.pos.col() {
                    break;
                }
                split = i;
            }
            ci.comments.extend(self.acc_coms.drain(..split));
            items.push(ci);
        }
        Ok(items)
    }

    fn test_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let left = self.pos;
        let old = self.pre_nested(TEST_EXPR);
        self.next()?;
        let Some(x) = self.nested(|p| p.test_expr_binary(false))? else {
            return self.follow_err_exp(left, Token::DblLeftBrack.quoted());
        };
        let right = self.pos;
        let (_, ok) = self.got_rsrv("]]")?;
        if !ok {
            return self.matching_err(left, Token::DblLeftBrack, Token::DblRightBrack);
        }
        self.post_nested(old);
        s.cmd = Some(Command::Test(TestClause { left, right, x }));
        Ok(())
    }

    fn test_expr_binary(&mut self, past_and_or: bool) -> PResult<Option<TestExpr>> {
        self.got(Token::Newl)?;
        let left = if past_and_or {
            self.test_expr_unary()?
        } else {
            self.test_expr_binary(true)?
        };
        let Some(left) = left else {
            return Ok(None);
        };
        self.got(Token::Newl)?;
        match self.tok {
            Token::AndAnd | Token::OrOr | Token::RdrIn | Token::RdrOut => {}
            Token::LitWord => {
                if self.val == "]]" {
                    return Ok(Some(left));
                }
                self.tok = test_binary_op(&self.val);
                if self.tok == Token::Illegal {
                    return self
                        .cur_err(format!("not a valid test operator: {}", quote(&self.val)));
                }
            }
            Token::Eof | Token::RightParen => return Ok(Some(left)),
            Token::Lit => {
                return self.cur_err("test operator words must consist of a single literal".into());
            }
            _ => {
                return self.cur_err(format!("not a valid test operator: {}", self.tok.quoted()));
            }
        }
        let op_pos = self.pos;
        let op = self.tok;
        let y = match op {
            Token::AndAnd | Token::OrOr => {
                self.next()?;
                match self.nested(|p| p.test_expr_binary(false))? {
                    Some(y) => y,
                    None => return self.follow_err_exp(op_pos, op.quoted()),
                }
            }
            _ => {
                if op == Token::TsReMatch {
                    self.check_lang(self.pos, LANG_BASH_LIKE | LANG_ZSH, "regex tests".into())?;
                    self.rx_open_parens = 0;
                    self.rx_first_part = true;
                    self.quote = TEST_EXPR_REGEXP;
                }
                if !matches!(left, TestExpr::Word(_)) {
                    return self.pos_err(
                        op_pos,
                        format!(
                            "expected {}, {} or {} after complex expr",
                            Token::AndAnd.quoted(),
                            Token::OrOr.quoted(),
                            Token::DblRightBrack.quoted()
                        ),
                    );
                }
                self.next()?;
                TestExpr::Word(self.follow_word_tok(op, op_pos)?)
            }
        };
        Ok(Some(TestExpr::Binary(Box::new(BinaryTest {
            op,
            x: left,
            y,
        }))))
    }

    fn test_expr_unary(&mut self) -> PResult<Option<TestExpr>> {
        match self.tok {
            Token::Eof | Token::RightParen => return Ok(None),
            Token::LitWord => {
                let op = test_unary_op(&self.val);
                match op {
                    Token::Illegal => {}
                    Token::TsRefVar | Token::TsModif => {
                        if self.lang_in(LANG_BASH_LIKE) {
                            self.tok = op;
                        }
                    }
                    _ => self.tok = op,
                }
            }
            _ => {}
        }
        match self.tok {
            Token::ExclMark => {
                let op_pos = self.pos;
                self.next()?;
                let Some(x) = self.nested(|p| p.test_expr_binary(false))? else {
                    return self.follow_err_exp(op_pos, Token::ExclMark.quoted());
                };
                Ok(Some(TestExpr::Unary(Box::new(UnaryTest {
                    op_pos,
                    op: Token::ExclMark,
                    x,
                }))))
            }
            Token::TsExists
            | Token::TsRegFile
            | Token::TsDirect
            | Token::TsCharSp
            | Token::TsBlckSp
            | Token::TsNmPipe
            | Token::TsSocket
            | Token::TsSmbLink
            | Token::TsSticky
            | Token::TsGIDSet
            | Token::TsUIDSet
            | Token::TsGrpOwn
            | Token::TsUsrOwn
            | Token::TsModif
            | Token::TsRead
            | Token::TsWrite
            | Token::TsExec
            | Token::TsNoEmpty
            | Token::TsFdTerm
            | Token::TsEmpStr
            | Token::TsNempStr
            | Token::TsOptSet
            | Token::TsVarSet
            | Token::TsRefVar => {
                let op_pos = self.pos;
                let op = self.tok;
                self.next()?;
                let x = TestExpr::Word(self.follow_word_tok(op, op_pos)?);
                Ok(Some(TestExpr::Unary(Box::new(UnaryTest { op_pos, op, x }))))
            }
            Token::LeftParen => {
                let lparen = self.pos;
                self.next()?;
                let Some(x) = self.nested(|p| p.test_expr_binary(false))? else {
                    return self.follow_err_exp(lparen, Token::LeftParen.quoted());
                };
                self.matched(lparen, Token::LeftParen, Token::RightParen)?;
                Ok(Some(TestExpr::Paren(Box::new(ParenTest { lparen, x }))))
            }
            Token::LitWord if self.val == "]]" => Ok(None),
            _ => Ok(self.get_word()?.map(TestExpr::Word)),
        }
    }

    fn decl_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let variant = self.lit(self.pos, self.val.clone());
        self.next()?;
        let mut args = Vec::new();
        while !self.stop_token() && !self.peek_redir() {
            if self.has_valid_ident() {
                args.push(self.get_assign(false)?);
            } else if self.tok.is_lit()
                && self.eql_offs > 0
                && !self
                    .val
                    .get(..self.eql_offs as usize)
                    .is_some_and(|name| name.contains('{'))
            {
                return self.cur_err("invalid var name".into());
            } else if self.tok == Token::LitWord && valid_name(&self.val) {
                args.push(Assign {
                    naked: true,
                    name: self.get_lit()?,
                    ..Assign::default()
                });
            } else if let Some(w) = self.get_word()? {
                args.push(Assign {
                    naked: true,
                    value: Some(w),
                    ..Assign::default()
                });
            } else {
                return self.follow_err(self.pos, quote(&variant.value), "names or assignments");
            }
        }
        s.cmd = Some(Command::Decl(DeclClause { variant, args }));
        Ok(())
    }

    fn time_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let time = self.pos;
        self.next()?;
        let (_, posix_format) = self.got_rsrv("-p")?;
        let stmt = self.nested(|p| p.got_stmt_pipe(Stmt::at(p.pos), false))?;
        s.cmd = Some(Command::Time(Box::new(TimeClause {
            time,
            posix_format,
            stmt,
        })));
        Ok(())
    }

    fn coproc_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let coproc = self.pos;
        self.next()?;
        if is_bash_compound_command(self.tok, &self.val) {
            let stmt = self.nested(|p| p.got_stmt_pipe(Stmt::at(p.pos), false))?;
            s.cmd = Some(Command::Coproc(Box::new(CoprocClause {
                coproc,
                name: None,
                stmt,
            })));
            return Ok(());
        }
        let mut name = self.get_word()?;
        let mut stmt = self.nested(|p| p.got_stmt_pipe(Stmt::at(p.pos), false))?;
        if stmt.is_none() {
            let Some(n) = name.take() else {
                return self.pos_err(coproc, "coproc clause requires a command".into());
            };
            let mut st = Stmt::at(n.pos());
            st.cmd = Some(Command::Call(CallExpr {
                assigns: Vec::new(),
                args: vec![n],
            }));
            stmt = Some(st);
        } else if let Some(Command::Call(call)) = stmt.as_mut().and_then(|st| st.cmd.as_mut())
            && let Some(n) = name.take()
        {
            call.args.insert(0, n);
        }
        s.cmd = Some(Command::Coproc(Box::new(CoprocClause {
            coproc,
            name,
            stmt,
        })));
        Ok(())
    }

    fn let_clause(&mut self, s: &mut Stmt) -> PResult<()> {
        let let_pos = self.pos;
        let old = self.pre_nested(ARITHM_EXPR_LET);
        self.next()?;
        let mut exprs = Vec::new();
        while !self.stop_token() && !self.peek_redir() {
            let Some(x) = self.arithm_expr(true)? else {
                break;
            };
            exprs.push(x);
        }
        if exprs.is_empty() {
            return self.follow_err_exp(let_pos, quote("let"));
        }
        self.post_nested(old);
        s.cmd = Some(Command::Let(LetClause { let_pos, exprs }));
        Ok(())
    }

    fn bash_func_decl(&mut self, s: &mut Stmt) -> PResult<()> {
        let fpos = self.pos;
        self.next()?;
        let mut names = Vec::new();
        while self.tok == Token::LitWord && self.val != "{" {
            names.push(self.lit(self.pos, self.val.clone()));
            self.next()?;
        }
        let has_parens = self.got(Token::LeftParen)?;
        match names.len() {
            0 => {
                if has_parens || (self.tok == Token::LitWord && self.val == "{") {
                    self.check_lang(fpos, LANG_ZSH, "anonymous functions".into())?;
                } else {
                    return self.follow_err(fpos, quote("function"), "a name");
                }
            }
            1 => {}
            _ => self.check_lang(fpos, LANG_ZSH, "multi-name functions".into())?,
        }
        if has_parens {
            self.follow(fpos, "function foo(", Token::RightParen)?;
        }
        self.func_decl(s, fpos, true, has_parens, names.pop())
    }

    fn unexpected_in_call_expr<T>(&self, ce: &CallExpr) -> PResult<T> {
        if let Some(first) = ce.args.first() {
            let cmd = first.lit();
            if is_bash_compound_command(Token::LitWord, &cmd) {
                self.check_lang(
                    self.pos,
                    LANG_BASH_LIKE,
                    format!("the {} builtin", quote(&cmd)),
                )?;
            }
        }
        self.cur_err(format!(
            "a command can only contain words and redirects; encountered {}",
            self.tok.quoted()
        ))
    }

    fn call_expr(&mut self, s: &mut Stmt, w: Option<Word>, assign: bool) -> PResult<()> {
        let first_is_function = w.as_ref().is_some_and(|w| w.lit() == "function");
        let mut ce = CallExpr::default();
        if let Some(w) = w {
            ce.args.push(w);
        }
        if assign {
            ce.assigns.push(self.get_assign(true)?);
        }
        loop {
            match self.tok {
                Token::Eof
                | Token::Newl
                | Token::Semicolon
                | Token::And
                | Token::Or
                | Token::AndAnd
                | Token::OrOr
                | Token::OrAnd
                | Token::DblSemicolon
                | Token::SemiAnd
                | Token::DblSemiAnd => break,
                Token::LitWord => {
                    if ce.args.is_empty() && self.has_valid_ident() {
                        ce.assigns.push(self.get_assign(true)?);
                        continue;
                    }
                    if self.val == "{" && first_is_function {
                        self.check_lang(
                            self.pos,
                            LANG_BASH_LIKE,
                            "the \"function\" builtin".into(),
                        )?;
                    }
                    let w = word_one(WordPart::Lit(self.lit(self.pos, self.val.clone())));
                    self.next()?;
                    ce.args.push(w);
                }
                Token::Lit => {
                    if ce.args.is_empty() && self.has_valid_ident() {
                        ce.assigns.push(self.get_assign(true)?);
                        continue;
                    }
                    ce.args.push(self.word_any_number()?);
                }
                Token::BckQuote if self.backquote_end() => break,
                Token::BckQuote
                | Token::DollBrace
                | Token::DollDblParen
                | Token::DollParen
                | Token::Dollar
                | Token::CmdIn
                | Token::AssgnParen
                | Token::CmdOut
                | Token::SglQuote
                | Token::DollSglQuote
                | Token::DblQuote
                | Token::DollDblQuote
                | Token::DollBrack
                | Token::GlobQuest
                | Token::GlobStar
                | Token::GlobPlus
                | Token::GlobAt
                | Token::GlobExcl => ce.args.push(self.word_any_number()?),
                Token::DblLeftParen => {
                    return self.cur_err(format!(
                        "{} can only be used to open an arithmetic cmd",
                        self.tok.quoted()
                    ));
                }
                Token::LeftParen => return self.unexpected_in_call_expr(&ce),
                Token::RightParen => {
                    if self.quote == SUB_CMD {
                        break;
                    }
                    return self.unexpected_in_call_expr(&ce);
                }
                _ => {
                    if self.peek_redir() {
                        self.do_redirect(s)?;
                        continue;
                    }
                    return self.unexpected_in_call_expr(&ce);
                }
            }
        }
        if !ce.args.is_empty() {
            for a in &ce.assigns {
                if a.index.is_some() || a.array.is_some() {
                    return self.pos_err(a.pos(), "inline variables cannot be arrays".into());
                }
            }
        }
        s.cmd = Some(Command::Call(ce));
        Ok(())
    }

    fn func_decl(
        &mut self,
        s: &mut Stmt,
        position: Pos,
        rsrv_word: bool,
        parens: bool,
        name: Option<Lit>,
    ) -> PResult<()> {
        self.got(Token::Newl)?;
        let Some(body) = self.get_stmt(false, false, true)? else {
            return self.follow_err(position, quote("foo()"), "a statement");
        };
        s.cmd = Some(Command::Func(Box::new(FuncDecl {
            position,
            rsrv_word,
            parens,
            name,
            body,
        })));
        Ok(())
    }
}
