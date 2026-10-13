use super::nodes::{Comment, Pos, Word, WordPart};
use super::parser::*;
use super::tokens::Token;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rune {
    Ch(char),
    Eof,
    EscNewl,
}

impl Rune {
    pub fn is(self, c: char) -> bool {
        self == Rune::Ch(c)
    }
}

const RUNE_SELF: u8 = 0x80;

fn reg_ops(r: Rune) -> bool {
    matches!(
        r,
        Rune::Ch(';' | '"' | '\'' | '(' | ')' | '$' | '|' | '&' | '>' | '<' | '`')
    )
}

fn param_ops(r: Rune) -> bool {
    matches!(
        r,
        Rune::Ch(
            '}' | '#'
                | '!'
                | ':'
                | '-'
                | '+'
                | '='
                | '?'
                | '%'
                | '['
                | ']'
                | '/'
                | '^'
                | ','
                | '@'
                | '*'
        )
    )
}

fn arithm_ops(r: Rune) -> bool {
    matches!(
        r,
        Rune::Ch(
            '+' | '-'
                | '!'
                | '~'
                | '*'
                | '/'
                | '%'
                | '('
                | ')'
                | '^'
                | '<'
                | '>'
                | ':'
                | '='
                | ','
                | '?'
                | '|'
                | '&'
                | '['
                | ']'
                | '#'
                | '.'
        )
    )
}

fn bquote_escaped(b: u8) -> bool {
    matches!(b, b'$' | b'`' | b'\\')
}

pub fn positional_rune_param(r: char) -> bool {
    r.is_ascii_digit()
}

pub fn single_rune_param(r: char) -> bool {
    matches!(r, '@' | '*' | '#' | '$' | '?' | '!' | '-') || positional_rune_param(r)
}

pub fn param_name_rune(r: char) -> bool {
    r.is_ascii_alphanumeric() || r == '_'
}

pub fn param_name_start_rune(r: char) -> bool {
    single_rune_param(r) || param_name_rune(r)
}

impl Parser<'_> {
    pub(super) fn rune(&mut self) -> Rune {
        if self.r == Rune::Ch('\n') || self.r == Rune::EscNewl {
            self.line += 1;
            self.col = 0;
        }
        self.col += self.w as i64;
        let mut bquotes = 0;
        loop {
            if self.bsp >= self.bs.len() && self.fill() == 0 {
                if self.bs.is_empty() {
                    self.bsp = 1;
                }
                self.r = Rune::Eof;
                self.w = 1;
                return self.r;
            }
            let b = self.bs[self.bsp];
            if b < RUNE_SELF {
                self.bsp += 1;
                match b {
                    0 => {
                        self.col += 1;
                        continue;
                    }
                    b'\r' => {
                        if self.peek() == b'\n' {
                            self.col += 1;
                            continue;
                        }
                    }
                    b'\\' => {
                        if self.r == Rune::Ch('\\') {
                        } else if self.peek() == b'\n' {
                            self.bsp += 1;
                            self.w = 1;
                            self.r = Rune::EscNewl;
                            return self.r;
                        } else if self.peek_two() == (b'\r', b'\n') {
                            self.col += 1;
                            self.bsp += 2;
                            self.w = 2;
                            self.r = Rune::EscNewl;
                            return self.r;
                        }
                        self.read_eof = false;
                        if self.open_bquotes > 0
                            && self.bsp < self.bs.len()
                            && ((bquotes < self.open_bquotes && bquote_escaped(self.bs[self.bsp]))
                                || (bquotes < self.open_bquote_dbls && self.bs[self.bsp] == b'"'))
                        {
                            bquotes += 1;
                            self.col += 1;
                            continue;
                        }
                    }
                    _ => {}
                }
                if b == b'`' {
                    self.last_bquote_esc = bquotes;
                }
                if let Some(lit) = &mut self.lit_bs {
                    lit.push(b);
                }
                self.w = 1;
                self.r = Rune::Ch(b as char);
                return self.r;
            }
            let width = match b {
                0xF0.. => 4,
                0xE0.. => 3,
                _ => 2,
            };
            if self.bs.len() - self.bsp < width && self.fill() > 0 {
                continue;
            }
            let rest = &self.bs[self.bsp..];
            let width = width.min(rest.len());
            let c = std::str::from_utf8(&rest[..width])
                .ok()
                .and_then(|s| s.chars().next())
                .unwrap_or('\u{FFFD}');
            if let Some(lit) = &mut self.lit_bs {
                lit.extend_from_slice(&rest[..width]);
            }
            self.bsp += width;
            self.w = width;
            self.r = Rune::Ch(c);
            return self.r;
        }
    }

    fn fill(&mut self) -> usize {
        if self.read_eof || self.r == Rune::Eof {
            return 0;
        }
        self.offs += self.bsp;
        let left = self.bs.len() - self.bsp.min(self.bs.len());
        let read_from = self.offs + left;
        let mut n = 0;
        if !self.read_err {
            n = (self.src.len() - read_from).min(READ_BUF_SIZE - left);
            if n == 0 {
                self.read_err = true;
                self.read_eof = true;
            }
        }
        self.bs = &self.src[self.offs..read_from + n];
        self.bsp = 0;
        n
    }

    fn next_keep_spaces(&mut self) -> PResult<()> {
        let mut r = self.r;
        if self.quote != HDOC_BODY && self.quote != HDOC_BODY_TABS {
            while r == Rune::EscNewl {
                r = self.rune();
            }
        }
        self.pos = self.next_pos();
        match self.quote {
            RUNE_BY_RUNE => self.tok = Token::Illegal,
            DBL_QUOTES => match r {
                Rune::Ch('`' | '"' | '$') => self.tok = self.dq_token(r),
                _ => self.advance_lit_dquote(r),
            },
            HDOC_BODY | HDOC_BODY_TABS => match r {
                Rune::Ch('`' | '$') => self.tok = self.dq_token(r),
                _ => self.advance_lit_hdoc(r),
            },
            PARAM_EXP_REPL if r.is('/') => {
                self.rune();
                self.tok = Token::Slash;
            }
            PARAM_EXP_REPL | PARAM_EXP_EXP => match r {
                Rune::Ch('}') => self.tok = self.param_token(r),
                Rune::Ch('`' | '"' | '$' | '\'') => self.tok = self.reg_token(r),
                _ => self.advance_lit_other(r),
            },
            _ => {}
        }
        Ok(())
    }

    pub(super) fn next(&mut self) -> PResult<()> {
        if self.r == Rune::Eof {
            self.tok = Token::Eof;
            return Ok(());
        }
        self.spaced = false;
        if self.quote & ALL_KEEP_SPACES != 0 {
            return self.next_keep_spaces();
        }
        let mut r = self.r;
        while r == Rune::EscNewl {
            r = self.rune();
        }
        loop {
            loop {
                match r {
                    Rune::Eof => {
                        self.tok = Token::Eof;
                        return Ok(());
                    }
                    Rune::EscNewl => r = self.rune(),
                    Rune::Ch(' ' | '\t' | '\r') => {
                        self.spaced = true;
                        r = self.rune();
                    }
                    Rune::Ch('\n') => {
                        if self.tok == Token::Newl {
                            r = self.rune();
                            continue;
                        }
                        self.spaced = true;
                        self.tok = Token::Newl;
                        if self.quote != HDOC_WORD && self.heredocs.len() > self.buried_hdocs {
                            self.do_heredocs()?;
                        }
                        return Ok(());
                    }
                    _ => break,
                }
            }
            self.pos = self.next_pos();
            if self.quote & ALL_REG_TOKENS != 0 {
                match r {
                    Rune::Ch(';' | '"' | '\'' | '(' | ')' | '$' | '|' | '&' | '>' | '<' | '`') => {
                        self.tok = self.reg_token(r);
                    }
                    Rune::Ch('#') => {
                        if !self.spaced
                            && (self.quote == UNQUOTED_WORD_CONT || self.quote == TEST_EXPR)
                        {
                            self.advance_lit_none(r);
                            return Ok(());
                        }
                        r = self.rune();
                        self.new_lit(r);
                        loop {
                            match r {
                                Rune::Ch('\n') | Rune::Eof => break,
                                Rune::EscNewl => {
                                    if let Some(lit) = &mut self.lit_bs {
                                        lit.extend_from_slice(b"\\\n");
                                    }
                                    break;
                                }
                                Rune::Ch('`') if self.backquote_end() => break,
                                _ => {}
                            }
                            r = self.rune();
                        }
                        let text = self.end_lit();
                        self.acc_coms.push(Comment {
                            hash: self.pos,
                            text,
                        });
                        return self.next();
                    }
                    Rune::Ch('[') => {
                        if self.quote == ARRAY_ELEMS
                            && (self.spaced
                                || self.tok == Token::LeftParen
                                || self.tok == Token::Newl)
                        {
                            self.rune();
                            self.tok = Token::LeftBrack;
                        } else {
                            self.advance_lit_none(r);
                        }
                    }
                    Rune::Ch('=') => {
                        if self.peek() == b'(' {
                            self.rune();
                            self.rune();
                            self.tok = Token::AssgnParen;
                        } else if self.quote == ARRAY_ELEMS {
                            self.rune();
                            self.tok = Token::Assgn;
                        } else {
                            self.advance_lit_none(r);
                        }
                    }
                    Rune::Ch(c @ ('?' | '*' | '+' | '@' | '!')) => {
                        if self.extended_glob() {
                            self.tok = match c {
                                '?' => Token::GlobQuest,
                                '*' => Token::GlobStar,
                                '+' => Token::GlobPlus,
                                '@' => Token::GlobAt,
                                _ => Token::GlobExcl,
                            };
                            self.rune();
                            self.rune();
                        } else {
                            self.advance_lit_none(r);
                        }
                    }
                    _ => self.advance_lit_none(r),
                }
            } else if self.quote == PARAM_EXP_ARITHM && r.is('#') {
                self.advance_lit_other(r);
            } else if self.quote & ALL_ARITHM_EXPR != 0 && arithm_ops(r) {
                self.tok = self.arithm_token(r);
            } else if self.quote & ALL_PARAM_EXP != 0 && param_ops(r) {
                self.tok = self.param_token(r);
            } else if self.quote == TEST_EXPR_REGEXP {
                if !self.rx_first_part && self.spaced {
                    self.quote = TEST_EXPR;
                    continue;
                }
                self.rx_first_part = false;
                match r {
                    Rune::Ch(';' | '"' | '\'' | '$' | '&' | '>' | '<' | '`') => {
                        self.tok = self.reg_token(r);
                    }
                    Rune::Ch(')') => {
                        if self.rx_open_parens > 0 {
                            self.advance_lit_re(r);
                        } else {
                            self.tok = Token::RightParen;
                            self.quote = TEST_EXPR;
                            self.rune();
                        }
                    }
                    _ => self.advance_lit_re(r),
                }
            } else if reg_ops(r) {
                self.tok = self.reg_token(r);
            } else {
                self.advance_lit_other(r);
            }
            return Ok(());
        }
    }

    fn extended_glob(&mut self) -> bool {
        if self.val == "function" {
            return false;
        }
        if self.peek() == b'(' {
            let (_, p2) = self.peek_two();
            return p2 != b')';
        }
        false
    }

    pub(super) fn peek(&mut self) -> u8 {
        if self.bsp >= self.bs.len() {
            self.fill();
        }
        if self.bsp >= self.bs.len() {
            return RUNE_SELF;
        }
        self.bs[self.bsp]
    }

    pub(super) fn peek_two(&mut self) -> (u8, u8) {
        if self.bsp + 1 >= self.bs.len() {
            self.fill();
        }
        if self.bsp >= self.bs.len() {
            return (RUNE_SELF, RUNE_SELF);
        }
        if self.bsp + 1 >= self.bs.len() {
            return (self.bs[self.bsp], RUNE_SELF);
        }
        (self.bs[self.bsp], self.bs[self.bsp + 1])
    }

    pub(super) fn reg_token(&mut self, r: Rune) -> Token {
        let Rune::Ch(c) = r else {
            return Token::Illegal;
        };
        match c {
            '\'' => {
                self.rune();
                Token::SglQuote
            }
            '"' => {
                self.rune();
                Token::DblQuote
            }
            '`' => Token::BckQuote,
            '&' => match self.rune() {
                Rune::Ch('&') => {
                    self.rune();
                    Token::AndAnd
                }
                Rune::Ch('>') => match self.rune() {
                    Rune::Ch('|') => {
                        self.rune();
                        Token::RdrAllClob
                    }
                    Rune::Ch('>') => match self.rune() {
                        Rune::Ch('|') => {
                            self.rune();
                            Token::AppAllClob
                        }
                        _ => Token::AppAll,
                    },
                    _ => Token::RdrAll,
                },
                _ => Token::And,
            },
            '|' => match self.rune() {
                Rune::Ch('|') => {
                    self.rune();
                    Token::OrOr
                }
                Rune::Ch('&') if self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH) => {
                    self.rune();
                    Token::OrAnd
                }
                _ => Token::Or,
            },
            '$' => match self.rune() {
                Rune::Ch('\'') if self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH) => {
                    self.rune();
                    Token::DollSglQuote
                }
                Rune::Ch('"') if self.lang_in(LANG_BASH_LIKE | LANG_MKSH) => {
                    self.rune();
                    Token::DollDblQuote
                }
                Rune::Ch('{') => {
                    self.rune();
                    Token::DollBrace
                }
                Rune::Ch('[') if self.lang_in(LANG_BASH_LIKE) => {
                    self.rune();
                    Token::DollBrack
                }
                Rune::Ch('(') => {
                    if self.rune().is('(') {
                        self.rune();
                        return Token::DollDblParen;
                    }
                    Token::DollParen
                }
                _ => Token::Dollar,
            },
            '(' => {
                if self.rune().is('(')
                    && self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH)
                    && self.quote != TEST_EXPR
                {
                    self.rune();
                    return Token::DblLeftParen;
                }
                Token::LeftParen
            }
            ')' => {
                self.rune();
                Token::RightParen
            }
            ';' => match self.rune() {
                Rune::Ch(';') => {
                    if self.rune().is('&') && self.lang_in(LANG_BASH_LIKE) {
                        self.rune();
                        return Token::DblSemiAnd;
                    }
                    Token::DblSemicolon
                }
                Rune::Ch('&') if self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH) => {
                    self.rune();
                    Token::SemiAnd
                }
                _ => Token::Semicolon,
            },
            '<' => match self.rune() {
                Rune::Ch('<') => match self.rune() {
                    Rune::Ch('-') => {
                        self.rune();
                        Token::DashHdoc
                    }
                    Rune::Ch('<') => {
                        self.rune();
                        Token::WordHdoc
                    }
                    _ => Token::Hdoc,
                },
                Rune::Ch('>') => {
                    self.rune();
                    Token::RdrInOut
                }
                Rune::Ch('&') => {
                    self.rune();
                    Token::DplIn
                }
                Rune::Ch('(') if self.lang_in(LANG_BASH_LIKE | LANG_ZSH) => {
                    self.rune();
                    Token::CmdIn
                }
                _ => Token::RdrIn,
            },
            '>' => match self.rune() {
                Rune::Ch('>') => match self.rune() {
                    Rune::Ch('|') => {
                        self.rune();
                        Token::AppClob
                    }
                    _ => Token::AppOut,
                },
                Rune::Ch('&') => {
                    self.rune();
                    Token::DplOut
                }
                Rune::Ch('|') => {
                    self.rune();
                    Token::RdrClob
                }
                Rune::Ch('(') if self.lang_in(LANG_BASH_LIKE | LANG_ZSH) => {
                    self.rune();
                    Token::CmdOut
                }
                _ => Token::RdrOut,
            },
            _ => Token::Illegal,
        }
    }

    pub(super) fn dq_token(&mut self, r: Rune) -> Token {
        match r {
            Rune::Ch('"') => {
                self.rune();
                Token::DblQuote
            }
            Rune::Ch('`') => Token::BckQuote,
            _ => match self.rune() {
                Rune::Ch('{') => {
                    self.rune();
                    Token::DollBrace
                }
                Rune::Ch('[') if self.lang_in(LANG_BASH_LIKE) => {
                    self.rune();
                    Token::DollBrack
                }
                Rune::Ch('(') => {
                    if self.rune().is('(') {
                        self.rune();
                        return Token::DollDblParen;
                    }
                    Token::DollParen
                }
                _ => Token::Dollar,
            },
        }
    }

    pub(super) fn param_token(&mut self, r: Rune) -> Token {
        let c = match r {
            Rune::Ch(c) => c,
            Rune::Eof => return Token::Eof,
            Rune::EscNewl => return Token::Illegal,
        };
        match c {
            '}' => {
                self.rune();
                Token::RightBrace
            }
            ':' => {
                let tok = match self.rune() {
                    Rune::Ch('+') => Token::ColPlus,
                    Rune::Ch('-') => Token::ColMinus,
                    Rune::Ch('?') => Token::ColQuest,
                    Rune::Ch('=') => Token::ColAssgn,
                    Rune::Ch('#') => Token::ColHash,
                    Rune::Ch('|') => Token::ColPipe,
                    Rune::Ch('*') => Token::ColStar,
                    _ => return Token::Colon,
                };
                self.rune();
                tok
            }
            '+' => {
                self.rune();
                Token::Plus
            }
            '-' => {
                self.rune();
                Token::Minus
            }
            '?' => {
                self.rune();
                Token::Quest
            }
            '=' => {
                self.rune();
                Token::Assgn
            }
            '%' => self.doubled('%', Token::Perc, Token::DblPerc),
            '#' => self.doubled('#', Token::Hash, Token::DblHash),
            '!' => {
                self.rune();
                Token::ExclMark
            }
            ']' => {
                self.rune();
                Token::RightBrack
            }
            '/' => self.doubled('/', Token::Slash, Token::DblSlash),
            '^' => self.doubled('^', Token::Caret, Token::DblCaret),
            ',' => self.doubled(',', Token::Comma, Token::DblComma),
            '@' => {
                self.rune();
                Token::At
            }
            '*' => {
                self.rune();
                Token::Star
            }
            _ => Token::Illegal,
        }
    }

    fn doubled(&mut self, c: char, single: Token, double: Token) -> Token {
        if self.rune().is(c) {
            self.rune();
            return double;
        }
        single
    }

    fn arithm_token(&mut self, r: Rune) -> Token {
        let Rune::Ch(c) = r else {
            return Token::Illegal;
        };
        match c {
            '!' => self.doubled('=', Token::ExclMark, Token::Nequal),
            '=' => self.doubled('=', Token::Assgn, Token::Equal),
            '~' => {
                self.rune();
                Token::Tilde
            }
            '(' => {
                self.rune();
                Token::LeftParen
            }
            ')' => {
                self.rune();
                Token::RightParen
            }
            '&' => match self.rune() {
                Rune::Ch('&') => {
                    self.rune();
                    Token::AndAnd
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::AndAssgn
                }
                _ => Token::And,
            },
            '|' => match self.rune() {
                Rune::Ch('|') => {
                    self.rune();
                    Token::OrOr
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::OrAssgn
                }
                _ => Token::Or,
            },
            '<' => match self.rune() {
                Rune::Ch('<') => self.doubled('=', Token::Hdoc, Token::ShlAssgn),
                Rune::Ch('=') => {
                    self.rune();
                    Token::Lequal
                }
                _ => Token::RdrIn,
            },
            '>' => match self.rune() {
                Rune::Ch('>') => self.doubled('=', Token::AppOut, Token::ShrAssgn),
                Rune::Ch('=') => {
                    self.rune();
                    Token::Gequal
                }
                _ => Token::RdrOut,
            },
            '+' => match self.rune() {
                Rune::Ch('+') => {
                    self.rune();
                    Token::AddAdd
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::AddAssgn
                }
                _ => Token::Plus,
            },
            '-' => match self.rune() {
                Rune::Ch('-') => {
                    self.rune();
                    Token::SubSub
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::SubAssgn
                }
                _ => Token::Minus,
            },
            '%' => self.doubled('=', Token::Perc, Token::RemAssgn),
            '*' => match self.rune() {
                Rune::Ch('*') => {
                    self.rune();
                    Token::Power
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::MulAssgn
                }
                _ => Token::Star,
            },
            '/' => self.doubled('=', Token::Slash, Token::QuoAssgn),
            '^' => match self.rune() {
                Rune::Ch('^') => {
                    self.rune();
                    Token::DblCaret
                }
                Rune::Ch('=') => {
                    self.rune();
                    Token::XorAssgn
                }
                _ => Token::Caret,
            },
            '[' => {
                self.rune();
                Token::LeftBrack
            }
            ']' => {
                self.rune();
                Token::RightBrack
            }
            ',' => {
                self.rune();
                Token::Comma
            }
            '?' => {
                self.rune();
                Token::Quest
            }
            ':' => {
                self.rune();
                Token::Colon
            }
            '#' => {
                self.rune();
                Token::Hash
            }
            '.' => {
                self.rune();
                Token::Period
            }
            _ => Token::Illegal,
        }
    }

    pub(super) fn new_lit(&mut self, r: Rune) {
        self.lit_bs = Some(match r {
            Rune::Ch(c) if c.is_ascii() => vec![c as u8],
            Rune::Eof | Rune::EscNewl => Vec::new(),
            Rune::Ch(c) => self.bs[self.bsp - c.len_utf8()..self.bsp].to_vec(),
        });
    }

    pub(super) fn end_lit(&mut self) -> String {
        let mut lit = self.lit_bs.take().unwrap_or_default();
        if self.r != Rune::Eof && self.r != Rune::EscNewl {
            lit.truncate(lit.len().saturating_sub(self.w));
        }
        String::from_utf8_lossy(&lit).into_owned()
    }

    fn is_lit_redir(&self) -> bool {
        let Some(lit_bs) = &self.lit_bs else {
            return false;
        };
        let lit = &lit_bs[..lit_bs.len().saturating_sub(1)];
        if lit.len() >= 2 && lit[0] == b'{' && lit[lit.len() - 1] == b'}' {
            let mut name = &lit[1..lit.len() - 1];
            if self.lang_in(LANG_BASH_LIKE)
                && name.last() == Some(&b']')
                && let Some(i) = name.iter().position(|&b| b == b'[')
                && i > 0
                && i + 2 < name.len()
            {
                name = &name[..i];
            }
            return valid_name(&String::from_utf8_lossy(name));
        }
        number_literal(&String::from_utf8_lossy(lit))
    }

    pub(super) fn advance_lit_other(&mut self, mut r: Rune) {
        let mut tok = Token::LitWord;
        self.new_lit(r);
        while r != Rune::Eof {
            let stop = match r {
                Rune::Ch('\\') => {
                    self.rune();
                    false
                }
                Rune::Ch('\'' | '"' | '`' | '$') => {
                    tok = Token::Lit;
                    true
                }
                Rune::Ch('}') => self.quote & ALL_PARAM_EXP != 0,
                Rune::Ch('/') => self.quote != PARAM_EXP_EXP,
                Rune::Ch(':' | '=' | '%' | '^' | ',' | '?' | '!' | '~' | '*') => {
                    self.quote & ALL_ARITHM_EXPR != 0
                }
                Rune::Ch('.') => self.quote & ALL_ARITHM_EXPR != 0,
                Rune::Ch('[' | ']')
                    if self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH)
                        && self.quote & ALL_ARITHM_EXPR != 0 =>
                {
                    true
                }
                Rune::Ch(
                    '[' | ']' | '+' | '-' | ' ' | '\t' | ';' | '&' | '>' | '<' | '|' | '(' | ')'
                    | '\n' | '\r',
                ) => self.quote & ALL_KEEP_SPACES == 0,
                _ => false,
            };
            if stop {
                break;
            }
            r = self.rune();
        }
        self.tok = tok;
        self.val = self.end_lit();
    }

    pub(super) fn advance_lit_none(&mut self, mut r: Rune) {
        self.eql_offs = -1;
        let mut tok = Token::LitWord;
        self.new_lit(r);
        while r != Rune::Eof {
            match r {
                Rune::Ch(' ' | '\t' | '\n' | '\r' | '&' | '|' | ';' | ')' | '(') => break,
                Rune::Ch('\\') => {
                    self.rune();
                }
                Rune::Ch('>' | '<') => {
                    if self.peek() == b'(' {
                        tok = Token::Lit;
                    } else if self.is_lit_redir() {
                        tok = Token::LitRedir;
                    }
                    break;
                }
                Rune::Ch('`') => {
                    if self.quote != SUB_CMD_BCKQUO {
                        tok = Token::Lit;
                    }
                    break;
                }
                Rune::Ch('"' | '\'' | '$') => {
                    tok = Token::Lit;
                    break;
                }
                Rune::Ch('?' | '*' | '+' | '@' | '!') => {
                    if self.extended_glob() {
                        tok = Token::Lit;
                        break;
                    }
                }
                Rune::Ch('=') => {
                    if self.eql_offs < 0 {
                        self.eql_offs = self.lit_len() as isize - 1;
                    }
                }
                Rune::Ch('}') => {
                    if self.quote == SUB_CMD_BRACES && self.lit_len() == 1 {
                        self.rune();
                        break;
                    }
                }
                Rune::Ch('[') => {
                    let first = self.lit_bs.as_ref().and_then(|l| l.first().copied());
                    if !(first == Some(b'{') && self.lang_in(LANG_BASH_LIKE))
                        && self.lang_in(LANG_BASH_LIKE | LANG_MKSH | LANG_ZSH)
                        && self.lit_len() > 1
                        && first != Some(b'[')
                    {
                        tok = Token::Lit;
                        break;
                    }
                }
                _ => {}
            }
            r = self.rune();
        }
        self.tok = tok;
        self.val = self.end_lit();
    }

    fn lit_len(&self) -> usize {
        self.lit_bs.as_ref().map(Vec::len).unwrap_or(0)
    }

    fn advance_lit_dquote(&mut self, mut r: Rune) {
        let mut tok = Token::LitWord;
        self.new_lit(r);
        while r != Rune::Eof {
            match r {
                Rune::Ch('"') => break,
                Rune::Ch('\\') => {
                    self.rune();
                }
                Rune::EscNewl | Rune::Ch('`' | '$') => {
                    tok = Token::Lit;
                    break;
                }
                _ => {}
            }
            r = self.rune();
        }
        self.tok = tok;
        self.val = self.end_lit();
    }

    fn advance_lit_hdoc(&mut self, mut r: Rune) {
        let mut last_tok = self.tok;
        while r == Rune::EscNewl {
            r = self.rune();
            last_tok = Token::Lit;
        }
        self.pos = self.next_pos();

        self.tok = Token::Lit;
        self.new_lit(r);
        while self.quote == HDOC_BODY_TABS && r.is('\t') {
            r = self.rune();
        }
        let mut l_start = self.lit_len() as isize - 1;
        let stop = self
            .hdoc_stops
            .last()
            .cloned()
            .flatten()
            .unwrap_or_default();
        loop {
            match r {
                Rune::EscNewl | Rune::Ch('$') => {
                    self.val = self.end_lit();
                    return;
                }
                Rune::Ch('\\') => {
                    self.rune();
                }
                Rune::Ch('`') | Rune::Ch('\n') | Rune::Eof => {
                    if r.is('`') && !self.backquote_end() {
                        self.val = self.end_lit();
                        return;
                    }
                    if l_start == 0 && last_tok == Token::Lit {
                    } else if l_start >= 0 {
                        let lit = self.lit_bs.as_deref().unwrap_or_default();
                        let mut line = &lit[(l_start as usize).min(lit.len())..];
                        if r != Rune::Eof && !line.is_empty() {
                            line = &line[..line.len() - 1];
                        }
                        if line == stop.as_slice() {
                            self.tok = Token::LitWord;
                            let mut val = self.end_lit();
                            val.truncate(l_start as usize);
                            self.val = val;
                            if self.val.is_empty() {
                                self.tok = Token::Newl;
                            }
                            if let Some(last) = self.hdoc_stops.last_mut() {
                                *last = None;
                            }
                            return;
                        }
                    }
                    if !r.is('\n') {
                        return;
                    }
                    while self.quote == HDOC_BODY_TABS && self.peek() == b'\t' {
                        self.rune();
                    }
                    l_start = self.lit_len() as isize;
                }
                _ => {}
            }
            r = self.rune();
        }
    }

    pub(super) fn quoted_hdoc_word(&mut self) -> Option<Word> {
        let mut r = self.r;
        self.new_lit(r);
        let pos = self.next_pos();
        let stop = self
            .hdoc_stops
            .last()
            .cloned()
            .flatten()
            .unwrap_or_default();
        loop {
            if r == Rune::Eof {
                return None;
            }
            while self.quote == HDOC_BODY_TABS && r.is('\t') {
                r = self.rune();
            }
            let l_start = self.lit_len() as isize - 1;
            loop {
                match r {
                    Rune::Eof | Rune::Ch('\n') => break,
                    Rune::Ch('`') => {
                        if self.backquote_end() {
                            break;
                        }
                    }
                    Rune::EscNewl => {
                        if let Some(lit) = &mut self.lit_bs {
                            lit.extend_from_slice(b"\\\n");
                        }
                        break;
                    }
                    _ => {}
                }
                r = self.rune();
            }
            if l_start >= 0 {
                let lit = self.lit_bs.as_deref().unwrap_or_default();
                let mut line = &lit[(l_start as usize).min(lit.len())..];
                if r != Rune::Eof && !line.is_empty() {
                    line = &line[..line.len() - 1];
                }
                if line == stop.as_slice() {
                    if let Some(last) = self.hdoc_stops.last_mut() {
                        *last = None;
                    }
                    let mut val = self.end_lit();
                    val.truncate(l_start as usize);
                    if val.is_empty() {
                        return None;
                    }
                    let lit = self.lit(pos, val);
                    return Some(word_one(WordPart::Lit(lit)));
                }
            }
            r = self.rune();
        }
    }

    fn advance_lit_re(&mut self, mut r: Rune) {
        self.new_lit(r);
        loop {
            match r {
                Rune::Ch('\\') => {
                    self.rune();
                }
                Rune::Ch('(') => self.rx_open_parens += 1,
                Rune::Ch(')') => {
                    self.rx_open_parens -= 1;
                    if self.rx_open_parens < 0 {
                        self.tok = Token::LitWord;
                        self.val = self.end_lit();
                        self.quote = TEST_EXPR;
                        return;
                    }
                }
                Rune::Ch(' ' | '\t' | '\r' | '\n' | ';' | '&' | '>' | '<') => {
                    if self.rx_open_parens <= 0 {
                        self.tok = Token::LitWord;
                        self.val = self.end_lit();
                        self.quote = TEST_EXPR;
                        return;
                    }
                }
                Rune::Ch('"' | '\'' | '$' | '`') => {
                    self.tok = Token::Lit;
                    self.val = self.end_lit();
                    return;
                }
                Rune::Eof => {
                    self.tok = Token::LitWord;
                    self.val = self.end_lit();
                    self.quote = NO_STATE;
                    return;
                }
                _ => {}
            }
            r = self.rune();
        }
    }

    pub(super) fn next_pos(&self) -> Pos {
        let offset = self.offs as i64 + self.bsp as i64 - self.w as i64;
        Pos::new(offset, self.line, self.col)
    }
}

pub fn test_unary_op(val: &str) -> Token {
    match val {
        "!" => Token::ExclMark,
        "-e" | "-a" => Token::TsExists,
        "-f" => Token::TsRegFile,
        "-d" => Token::TsDirect,
        "-c" => Token::TsCharSp,
        "-b" => Token::TsBlckSp,
        "-p" => Token::TsNmPipe,
        "-S" => Token::TsSocket,
        "-L" | "-h" => Token::TsSmbLink,
        "-k" => Token::TsSticky,
        "-g" => Token::TsGIDSet,
        "-u" => Token::TsUIDSet,
        "-G" => Token::TsGrpOwn,
        "-O" => Token::TsUsrOwn,
        "-N" => Token::TsModif,
        "-r" => Token::TsRead,
        "-w" => Token::TsWrite,
        "-x" => Token::TsExec,
        "-s" => Token::TsNoEmpty,
        "-t" => Token::TsFdTerm,
        "-z" => Token::TsEmpStr,
        "-n" => Token::TsNempStr,
        "-o" => Token::TsOptSet,
        "-v" => Token::TsVarSet,
        "-R" => Token::TsRefVar,
        _ => Token::Illegal,
    }
}

pub fn test_binary_op(val: &str) -> Token {
    match val {
        "=" => Token::Assgn,
        "==" => Token::Equal,
        "!=" => Token::Nequal,
        "=~" => Token::TsReMatch,
        "-nt" => Token::TsNewer,
        "-ot" => Token::TsOlder,
        "-ef" => Token::TsDevIno,
        "-eq" => Token::TsEql,
        "-ne" => Token::TsNeq,
        "-le" => Token::TsLeq,
        "-ge" => Token::TsGeq,
        "-lt" => Token::TsLss,
        "-gt" => Token::TsGtr,
        _ => Token::Illegal,
    }
}
