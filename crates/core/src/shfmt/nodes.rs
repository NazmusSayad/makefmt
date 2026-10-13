use std::cell::RefCell;
use std::rc::Rc;

use super::tokens::Token;

const OFFSET_MAX: i64 = u32::MAX as i64 - 11;
const LINE_MAX: u32 = (1 << 18) - 1;
const COL_MAX: u32 = (1 << 14) - 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pos {
    offs: u32,
    line: u32,
    col: u32,
}

impl Pos {
    pub fn new(offset: i64, line: i64, col: i64) -> Pos {
        Pos {
            offs: offset.min(OFFSET_MAX) as u32,
            line: if line > LINE_MAX as i64 {
                0
            } else {
                line as u32
            },
            col: if col > COL_MAX as i64 { 0 } else { col as u32 },
        }
    }

    pub fn line(self) -> u32 {
        self.line
    }

    pub fn col(self) -> u32 {
        self.col
    }

    pub fn is_valid(self) -> bool {
        self.line != 0 || self.col != 0
    }

    pub fn after(self, other: Pos) -> bool {
        self.is_valid() && self.offs > other.offs
    }

    pub fn add_col(self, n: i64) -> Pos {
        if !self.is_valid() {
            return self;
        }
        let offs = (self.offs as i64 + n).clamp(0, OFFSET_MAX) as u32;
        let mut col = self.col as i64;
        if col > 0 {
            col += n;
            if col < 1 || col > COL_MAX as i64 {
                col = 0;
            }
        }
        Pos {
            offs,
            line: self.line,
            col: col as u32,
        }
    }

    pub fn max(self, other: Pos) -> Pos {
        if other.after(self) {
            return other;
        }
        self
    }

    pub fn display(self) -> String {
        let line = if self.line > 0 {
            self.line.to_string()
        } else {
            "?".to_string()
        };
        let col = if self.col > 0 {
            self.col.to_string()
        } else {
            "?".to_string()
        };
        format!("{line}:{col}")
    }
}

#[derive(Clone, Debug)]
pub struct File {
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
}

pub fn stmts_end(stmts: &[Stmt], last: &[Comment]) -> Pos {
    if let Some(c) = last.last() {
        return c.end();
    }
    if let Some(s) = stmts.last() {
        let end = s.end();
        if let Some(c) = s.comments.last()
            && c.end().after(end)
        {
            return c.end();
        }
        return end;
    }
    Pos::default()
}

#[derive(Clone, Debug)]
pub struct Comment {
    pub hash: Pos,
    pub text: String,
}

impl Comment {
    pub fn pos(&self) -> Pos {
        self.hash
    }

    pub fn end(&self) -> Pos {
        self.hash.add_col(1 + self.text.len() as i64)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Stmt {
    pub comments: Vec<Comment>,
    pub cmd: Option<Command>,
    pub position: Pos,
    pub semicolon: Pos,
    pub negated: bool,
    pub background: bool,
    pub redirs: Vec<Redirect>,
}

impl Stmt {
    pub fn at(position: Pos) -> Stmt {
        Stmt {
            position,
            ..Stmt::default()
        }
    }

    pub fn pos(&self) -> Pos {
        self.position
    }

    pub fn end(&self) -> Pos {
        if self.semicolon.is_valid() {
            return self.semicolon.add_col(1);
        }
        let mut end = self.position;
        if self.negated {
            end = end.add_col(1);
        }
        if let Some(cmd) = &self.cmd {
            end = cmd.end();
        }
        if let Some(r) = self.redirs.last() {
            end = end.max(r.end());
        }
        end
    }
}

#[derive(Clone, Debug)]
pub enum Command {
    Call(CallExpr),
    If(Box<IfClause>),
    While(WhileClause),
    For(Box<ForClause>),
    Case(CaseClause),
    Block(Block),
    Subshell(Subshell),
    Binary(Box<BinaryCmd>),
    Func(Box<FuncDecl>),
    Arithm(ArithmCmd),
    Test(TestClause),
    Decl(DeclClause),
    Let(LetClause),
    Time(Box<TimeClause>),
    Coproc(Box<CoprocClause>),
}

impl Command {
    pub fn pos(&self) -> Pos {
        match self {
            Command::Call(c) => c.pos(),
            Command::If(c) => c.position,
            Command::While(c) => c.while_pos,
            Command::For(c) => c.for_pos,
            Command::Case(c) => c.case,
            Command::Block(c) => c.lbrace,
            Command::Subshell(c) => c.lparen,
            Command::Binary(c) => c.x.pos(),
            Command::Func(c) => c.position,
            Command::Arithm(c) => c.left,
            Command::Test(c) => c.left,
            Command::Decl(c) => c.variant.pos(),
            Command::Let(c) => c.let_pos,
            Command::Time(c) => c.time,
            Command::Coproc(c) => c.coproc,
        }
    }

    pub fn end(&self) -> Pos {
        match self {
            Command::Call(c) => c.end(),
            Command::If(c) => c.fi_pos.add_col(2),
            Command::While(c) => c.done_pos.add_col(4),
            Command::For(c) => {
                if c.braces {
                    c.done_pos.add_col(1)
                } else {
                    c.done_pos.add_col(4)
                }
            }
            Command::Case(c) => c.esac.add_col(4),
            Command::Block(c) => c.rbrace.add_col(1),
            Command::Subshell(c) => c.rparen.add_col(1),
            Command::Binary(c) => c.y.end(),
            Command::Func(c) => c.body.end(),
            Command::Arithm(c) => c.right.add_col(2),
            Command::Test(c) => c.right.add_col(2),
            Command::Decl(c) => match c.args.last() {
                Some(a) => a.end(),
                None => c.variant.end(),
            },
            Command::Let(c) => c.exprs.last().map(ArithmExpr::end).unwrap_or_default(),
            Command::Time(c) => match &c.stmt {
                Some(s) => s.end(),
                None => c.time.add_col(4),
            },
            Command::Coproc(c) => match &c.stmt {
                Some(s) => s.end(),
                None => Pos::default(),
            },
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Assign {
    pub append: bool,
    pub naked: bool,
    pub name: Option<Lit>,
    pub index: Option<ArithmExpr>,
    pub value: Option<Word>,
    pub array: Option<ArrayExpr>,
}

impl Assign {
    pub fn pos(&self) -> Pos {
        match &self.name {
            Some(name) => name.pos(),
            None => self.value.as_ref().map(Word::pos).unwrap_or_default(),
        }
    }

    pub fn end(&self) -> Pos {
        if let Some(value) = &self.value {
            return value.end();
        }
        if let Some(array) = &self.array {
            return array.rparen.add_col(1);
        }
        if let Some(index) = &self.index {
            if self.naked {
                return index.end().add_col(1);
            }
            return index.end().add_col(2);
        }
        let name_end = self.name.as_ref().map(Lit::end).unwrap_or_default();
        if self.naked {
            return name_end;
        }
        name_end.add_col(1)
    }
}

#[derive(Clone, Debug)]
pub struct Redirect {
    pub op_pos: Pos,
    pub op: Token,
    pub n: Option<Lit>,
    pub word: Word,
    pub hdoc: Rc<RefCell<Option<Word>>>,
}

impl Redirect {
    pub fn pos(&self) -> Pos {
        match &self.n {
            Some(n) => n.pos(),
            None => self.op_pos,
        }
    }

    pub fn end(&self) -> Pos {
        if let Some(hdoc) = &*self.hdoc.borrow() {
            return hdoc.end();
        }
        self.word.end()
    }
}

#[derive(Clone, Debug, Default)]
pub struct CallExpr {
    pub assigns: Vec<Assign>,
    pub args: Vec<Word>,
}

impl CallExpr {
    pub fn pos(&self) -> Pos {
        match self.assigns.first() {
            Some(a) => a.pos(),
            None => self.args.first().map(Word::pos).unwrap_or_default(),
        }
    }

    pub fn end(&self) -> Pos {
        match self.args.last() {
            Some(w) => w.end(),
            None => self.assigns.last().map(Assign::end).unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Subshell {
    pub lparen: Pos,
    pub rparen: Pos,
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub lbrace: Pos,
    pub rbrace: Pos,
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug, Default)]
pub struct IfClause {
    pub position: Pos,
    pub then_pos: Pos,
    pub fi_pos: Pos,
    pub cond: Vec<Stmt>,
    pub cond_last: Vec<Comment>,
    pub then: Vec<Stmt>,
    pub then_last: Vec<Comment>,
    pub else_: Option<Box<IfClause>>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub struct WhileClause {
    pub while_pos: Pos,
    pub do_pos: Pos,
    pub done_pos: Pos,
    pub until: bool,
    pub cond: Vec<Stmt>,
    pub cond_last: Vec<Comment>,
    pub do_: Vec<Stmt>,
    pub do_last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub struct ForClause {
    pub for_pos: Pos,
    pub do_pos: Pos,
    pub done_pos: Pos,
    pub select: bool,
    pub braces: bool,
    pub loop_: Loop,
    pub do_: Vec<Stmt>,
    pub do_last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub enum Loop {
    WordIter(WordIter),
    CStyle(CStyleLoop),
}

#[derive(Clone, Debug)]
pub struct WordIter {
    pub name: Lit,
    pub in_pos: Pos,
    pub items: Vec<Word>,
}

#[derive(Clone, Debug)]
pub struct CStyleLoop {
    pub init: Option<ArithmExpr>,
    pub cond: Option<ArithmExpr>,
    pub post: Option<ArithmExpr>,
}

#[derive(Clone, Debug)]
pub struct BinaryCmd {
    pub op_pos: Pos,
    pub op: Token,
    pub x: Stmt,
    pub y: Stmt,
}

#[derive(Clone, Debug)]
pub struct FuncDecl {
    pub position: Pos,
    pub rsrv_word: bool,
    pub parens: bool,
    pub name: Option<Lit>,
    pub body: Stmt,
}

#[derive(Clone, Debug)]
pub struct Word {
    pub parts: Vec<WordPart>,
}

impl Word {
    pub fn pos(&self) -> Pos {
        self.parts.first().map(WordPart::pos).unwrap_or_default()
    }

    pub fn end(&self) -> Pos {
        self.parts.last().map(WordPart::end).unwrap_or_default()
    }

    pub fn lit(&self) -> String {
        let mut out = String::new();
        for part in &self.parts {
            match part {
                WordPart::Lit(l) => out.push_str(&l.value),
                _ => return String::new(),
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
pub enum WordPart {
    Lit(Lit),
    SglQuoted(SglQuoted),
    DblQuoted(DblQuoted),
    ParamExp(Box<ParamExp>),
    CmdSubst(CmdSubst),
    ArithmExp(Box<ArithmExp>),
    ProcSubst(ProcSubst),
    ExtGlob(ExtGlob),
}

impl WordPart {
    pub fn pos(&self) -> Pos {
        match self {
            WordPart::Lit(l) => l.value_pos,
            WordPart::SglQuoted(q) => q.left,
            WordPart::DblQuoted(q) => q.left,
            WordPart::ParamExp(p) => p.pos(),
            WordPart::CmdSubst(c) => c.left,
            WordPart::ArithmExp(a) => a.left,
            WordPart::ProcSubst(s) => s.op_pos,
            WordPart::ExtGlob(e) => e.op_pos,
        }
    }

    pub fn end(&self) -> Pos {
        match self {
            WordPart::Lit(l) => l.value_end,
            WordPart::SglQuoted(q) => q.right.add_col(1),
            WordPart::DblQuoted(q) => q.right.add_col(1),
            WordPart::ParamExp(p) => p.end(),
            WordPart::CmdSubst(c) => c.right.add_col(1),
            WordPart::ArithmExp(a) => {
                if a.bracket {
                    a.right.add_col(1)
                } else {
                    a.right.add_col(2)
                }
            }
            WordPart::ProcSubst(s) => s.rparen.add_col(1),
            WordPart::ExtGlob(e) => e.pattern.end().add_col(1),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Lit {
    pub value_pos: Pos,
    pub value_end: Pos,
    pub value: String,
}

impl Lit {
    pub fn pos(&self) -> Pos {
        self.value_pos
    }

    pub fn end(&self) -> Pos {
        self.value_end
    }
}

#[derive(Clone, Debug)]
pub struct SglQuoted {
    pub left: Pos,
    pub right: Pos,
    pub dollar: bool,
    pub value: String,
}

#[derive(Clone, Debug)]
pub struct DblQuoted {
    pub left: Pos,
    pub right: Pos,
    pub dollar: bool,
    pub parts: Vec<WordPart>,
}

#[derive(Clone, Debug, Default)]
pub struct CmdSubst {
    pub left: Pos,
    pub right: Pos,
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
    pub backquotes: bool,
    pub temp_file: bool,
    pub reply_var: bool,
}

#[derive(Clone, Debug, Default)]
pub struct ParamExp {
    pub dollar: Pos,
    pub rbrace: Pos,
    pub short: bool,
    pub excl: bool,
    pub length: bool,
    pub param: Option<Lit>,
    pub index: Option<ArithmExpr>,
    pub slice: Option<Box<Slice>>,
    pub repl: Option<Box<Replace>>,
    pub names: Option<Token>,
    pub exp: Option<Box<Expansion>>,
}

impl ParamExp {
    pub fn pos(&self) -> Pos {
        if self.dollar.is_valid() {
            return self.dollar;
        }
        self.param.as_ref().map(Lit::pos).unwrap_or_default()
    }

    pub fn end(&self) -> Pos {
        if !self.short {
            return self.rbrace.add_col(1);
        }
        if let Some(index) = &self.index {
            return index.end().add_col(1);
        }
        self.param.as_ref().map(Lit::end).unwrap_or_default()
    }

    pub fn naked_index(&self) -> bool {
        self.short && self.index.is_some() && !self.dollar.is_valid()
    }
}

#[derive(Clone, Debug)]
pub struct Slice {
    pub offset: Option<ArithmExpr>,
    pub length: Option<ArithmExpr>,
}

#[derive(Clone, Debug)]
pub struct Replace {
    pub all: bool,
    pub orig: Option<Word>,
    pub with: Option<Word>,
}

#[derive(Clone, Debug)]
pub struct Expansion {
    pub op: Token,
    pub word: Option<Word>,
}

#[derive(Clone, Debug)]
pub struct ArithmExp {
    pub left: Pos,
    pub right: Pos,
    pub bracket: bool,
    pub x: ArithmExpr,
}

#[derive(Clone, Debug)]
pub struct ArithmCmd {
    pub left: Pos,
    pub right: Pos,
    pub x: ArithmExpr,
}

#[derive(Clone, Debug)]
pub enum ArithmExpr {
    Binary(Box<BinaryArithm>),
    Unary(Box<UnaryArithm>),
    Paren(Box<ParenArithm>),
    Word(Word),
}

impl ArithmExpr {
    pub fn pos(&self) -> Pos {
        match self {
            ArithmExpr::Binary(b) => b.x.pos(),
            ArithmExpr::Unary(u) => {
                if u.post {
                    u.x.pos()
                } else {
                    u.op_pos
                }
            }
            ArithmExpr::Paren(p) => p.lparen,
            ArithmExpr::Word(w) => w.pos(),
        }
    }

    pub fn end(&self) -> Pos {
        match self {
            ArithmExpr::Binary(b) => b.y.end(),
            ArithmExpr::Unary(u) => {
                if u.post {
                    u.op_pos.add_col(2)
                } else {
                    u.x.end()
                }
            }
            ArithmExpr::Paren(p) => p.rparen.add_col(1),
            ArithmExpr::Word(w) => w.end(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BinaryArithm {
    pub op: Token,
    pub x: ArithmExpr,
    pub y: ArithmExpr,
}

#[derive(Clone, Debug)]
pub struct UnaryArithm {
    pub op_pos: Pos,
    pub op: Token,
    pub post: bool,
    pub x: ArithmExpr,
}

#[derive(Clone, Debug)]
pub struct ParenArithm {
    pub lparen: Pos,
    pub rparen: Pos,
    pub x: ArithmExpr,
}

#[derive(Clone, Debug)]
pub struct CaseClause {
    pub case: Pos,
    pub in_: Pos,
    pub esac: Pos,
    pub word: Word,
    pub items: Vec<CaseItem>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub struct CaseItem {
    pub op: Token,
    pub op_pos: Pos,
    pub comments: Vec<Comment>,
    pub patterns: Vec<Word>,
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
}

impl CaseItem {
    pub fn pos(&self) -> Pos {
        self.patterns.first().map(Word::pos).unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub struct TestClause {
    pub left: Pos,
    pub right: Pos,
    pub x: TestExpr,
}

#[derive(Clone, Debug)]
pub enum TestExpr {
    Binary(Box<BinaryTest>),
    Unary(Box<UnaryTest>),
    Paren(Box<ParenTest>),
    Word(Word),
}

impl TestExpr {
    pub fn pos(&self) -> Pos {
        match self {
            TestExpr::Binary(b) => b.x.pos(),
            TestExpr::Unary(u) => u.op_pos,
            TestExpr::Paren(p) => p.lparen,
            TestExpr::Word(w) => w.pos(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct BinaryTest {
    pub op: Token,
    pub x: TestExpr,
    pub y: TestExpr,
}

#[derive(Clone, Debug)]
pub struct UnaryTest {
    pub op_pos: Pos,
    pub op: Token,
    pub x: TestExpr,
}

#[derive(Clone, Debug)]
pub struct ParenTest {
    pub lparen: Pos,
    pub x: TestExpr,
}

#[derive(Clone, Debug)]
pub struct DeclClause {
    pub variant: Lit,
    pub args: Vec<Assign>,
}

#[derive(Clone, Debug)]
pub struct ArrayExpr {
    pub rparen: Pos,
    pub elems: Vec<ArrayElem>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug, Default)]
pub struct ArrayElem {
    pub index: Option<ArithmExpr>,
    pub value: Option<Word>,
    pub comments: Vec<Comment>,
}

impl ArrayElem {
    pub fn pos(&self) -> Pos {
        match &self.index {
            Some(index) => index.pos(),
            None => self.value.as_ref().map(Word::pos).unwrap_or_default(),
        }
    }

    pub fn end(&self) -> Pos {
        match &self.value {
            Some(value) => value.end(),
            None => self
                .index
                .as_ref()
                .map(|index| index.end().add_col(2))
                .unwrap_or_default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExtGlob {
    pub op_pos: Pos,
    pub op: Token,
    pub pattern: Lit,
}

#[derive(Clone, Debug)]
pub struct ProcSubst {
    pub op_pos: Pos,
    pub rparen: Pos,
    pub op: Token,
    pub stmts: Vec<Stmt>,
    pub last: Vec<Comment>,
}

#[derive(Clone, Debug)]
pub struct TimeClause {
    pub time: Pos,
    pub posix_format: bool,
    pub stmt: Option<Stmt>,
}

#[derive(Clone, Debug)]
pub struct CoprocClause {
    pub coproc: Pos,
    pub name: Option<Word>,
    pub stmt: Option<Stmt>,
}

#[derive(Clone, Debug)]
pub struct LetClause {
    pub let_pos: Pos,
    pub exprs: Vec<ArithmExpr>,
}
