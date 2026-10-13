use super::nodes::*;
use super::parser::*;
use super::tokens::{Token, quote};

type Level<'a> = fn(&mut Parser<'a>, bool) -> PResult<Option<ArithmExpr>>;

fn is_arith_name(x: &ArithmExpr) -> bool {
    let ArithmExpr::Word(w) = x else {
        return false;
    };
    if w.parts.len() != 1 {
        return false;
    }
    match &w.parts[0] {
        WordPart::Lit(l) => valid_name(&l.value),
        WordPart::ParamExp(pe) => pe.naked_index(),
        _ => false,
    }
}

fn binary(op: Token, x: ArithmExpr, y: ArithmExpr) -> ArithmExpr {
    ArithmExpr::Binary(Box::new(BinaryArithm { op, x, y }))
}

impl<'a> Parser<'a> {
    pub(super) fn arithm_expr(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.nested(|p| p.arithm_expr_binary(compact, Self::arithm_expr_assign, &[Token::Comma]))
    }

    fn arithm_expr_assign(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        let value = self.arithm_expr_ternary(compact)?;
        match self.tok {
            Token::AddAssgn
            | Token::SubAssgn
            | Token::MulAssgn
            | Token::QuoAssgn
            | Token::RemAssgn
            | Token::AndAssgn
            | Token::OrAssgn
            | Token::XorAssgn
            | Token::ShlAssgn
            | Token::ShrAssgn
            | Token::Assgn => {
                if compact && self.spaced {
                    return Ok(value);
                }
                let Some(value) = value.filter(is_arith_name) else {
                    return self.cur_err(format!("{} must follow a name", self.tok.quoted()));
                };
                let pos = self.pos;
                let tok = self.tok;
                self.next_arith_op(compact)?;
                let Some(y) = self.nested(|p| p.arithm_expr_assign(compact))? else {
                    return self.follow_err_exp(pos, tok.quoted());
                };
                Ok(Some(binary(tok, value, y)))
            }
            _ => Ok(value),
        }
    }

    fn arithm_expr_ternary(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        let value = self.arithm_expr_binary(
            compact,
            Self::arithm_expr_land,
            &[Token::OrOr, Token::DblCaret],
        )?;
        if self.tok != Token::Quest || (compact && self.spaced) {
            return Ok(value);
        }
        let Some(value) = value else {
            return self.cur_err(format!("{} must follow an expression", self.tok.quoted()));
        };
        let quest_pos = self.pos;
        self.next_arith_op(compact)?;
        if self.tok == Token::Colon {
            return self.follow_err_exp(quest_pos, Token::Quest.quoted());
        }
        let Some(true_expr) = self.arithm_expr(compact)? else {
            return self.follow_err_exp(quest_pos, Token::Quest.quoted());
        };
        if self.tok != Token::Colon {
            return self.pos_err(
                quest_pos,
                format!(
                    "ternary operator missing {} after {}",
                    Token::Colon.quoted(),
                    Token::Quest.quoted()
                ),
            );
        }
        let colon_pos = self.pos;
        self.next_arith_op(compact)?;
        let Some(false_expr) = self.nested(|p| p.arithm_expr_ternary(compact))? else {
            return self.follow_err_exp(colon_pos, Token::Colon.quoted());
        };
        Ok(Some(binary(
            Token::Quest,
            value,
            binary(Token::Colon, true_expr, false_expr),
        )))
    }

    fn arithm_expr_land(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(compact, Self::arithm_expr_bor, &[Token::AndAnd])
    }

    fn arithm_expr_bor(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(compact, Self::arithm_expr_bxor, &[Token::Or])
    }

    fn arithm_expr_bxor(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(compact, Self::arithm_expr_band, &[Token::Caret])
    }

    fn arithm_expr_band(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(compact, Self::arithm_expr_equality, &[Token::And])
    }

    fn arithm_expr_equality(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(
            compact,
            Self::arithm_expr_comparison,
            &[Token::Equal, Token::Nequal],
        )
    }

    fn arithm_expr_comparison(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(
            compact,
            Self::arithm_expr_shift,
            &[Token::RdrIn, Token::RdrOut, Token::Lequal, Token::Gequal],
        )
    }

    fn arithm_expr_shift(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(
            compact,
            Self::arithm_expr_addition,
            &[Token::Hdoc, Token::AppOut],
        )
    }

    fn arithm_expr_addition(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(
            compact,
            Self::arithm_expr_multiplication,
            &[Token::Plus, Token::Minus],
        )
    }

    fn arithm_expr_multiplication(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        self.arithm_expr_binary(
            compact,
            Self::arithm_expr_power,
            &[Token::Star, Token::Slash, Token::Perc],
        )
    }

    fn arithm_expr_power(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        let value = self.arithm_expr_unary(compact)?;
        if self.tok != Token::Power || (compact && self.spaced) {
            return Ok(value);
        }
        let Some(value) = value else {
            return self.cur_err(format!("{} must follow an expression", self.tok.quoted()));
        };
        let op = self.tok;
        let pos = self.pos;
        self.next_arith_op(compact)?;
        let Some(y) = self.nested(|p| p.arithm_expr_power(compact))? else {
            return self.follow_err_exp(pos, op.quoted());
        };
        Ok(Some(binary(op, value, y)))
    }

    fn arithm_expr_unary(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        if !compact {
            self.got(Token::Newl)?;
        }
        match self.tok {
            Token::ExclMark | Token::Tilde | Token::Plus | Token::Minus => {
                let op_pos = self.pos;
                let op = self.tok;
                self.next_arith_op(compact)?;
                let Some(x) = self.nested(|p| p.arithm_expr_unary(compact))? else {
                    return self.follow_err_exp(op_pos, op.quoted());
                };
                Ok(Some(ArithmExpr::Unary(Box::new(UnaryArithm {
                    op_pos,
                    op,
                    post: false,
                    x,
                }))))
            }
            _ => self.arithm_expr_value(compact),
        }
    }

    fn arithm_expr_value(&mut self, compact: bool) -> PResult<Option<ArithmExpr>> {
        let x = match self.tok {
            Token::AddAdd | Token::SubSub => {
                let op_pos = self.pos;
                let op = self.tok;
                self.next_arith(compact)?;
                if self.tok != Token::LitWord {
                    return self.follow_err(op_pos, op.quoted(), "a literal");
                }
                let Some(x) = self.arithm_expr_value(compact)? else {
                    return self.follow_err(op_pos, op.quoted(), "a literal");
                };
                return Ok(Some(ArithmExpr::Unary(Box::new(UnaryArithm {
                    op_pos,
                    op,
                    post: false,
                    x,
                }))));
            }
            Token::LeftParen => {
                let lparen = self.pos;
                self.next_arith_op(compact)?;
                let x = self.follow_arithm(Token::LeftParen, lparen)?;
                let rparen = self.matched(lparen, Token::LeftParen, Token::RightParen)?;
                if self.quote == PARAM_EXP_ARITHM && self.tok == Token::LitWord {
                    self.check_lang(lparen, LANG_ZSH, "subscript flags".into())?;
                }
                ArithmExpr::Paren(Box::new(ParenArithm { lparen, rparen, x }))
            }
            Token::LeftBrack => {
                return self.cur_err(format!(
                    "{} must follow a name like a[i]",
                    self.tok.quoted()
                ));
            }
            Token::Colon => {
                return self.cur_err(format!(
                    "ternary operator missing {} before {}",
                    Token::Quest.quoted(),
                    Token::Colon.quoted()
                ));
            }
            Token::LitWord => {
                let Some(l) = self.get_lit()? else {
                    return Ok(None);
                };
                if self.tok != Token::LeftBrack {
                    ArithmExpr::Word(word_one(WordPart::Lit(l)))
                } else {
                    let index = self.either_index()?;
                    ArithmExpr::Word(word_one(WordPart::ParamExp(Box::new(ParamExp {
                        short: true,
                        param: Some(l),
                        index: Some(index),
                        ..ParamExp::default()
                    }))))
                }
            }
            Token::BckQuote if self.quote == ARITHM_EXPR_LET && self.open_bquotes > 0 => {
                return Ok(None);
            }
            _ => match self.get_word()? {
                Some(w) => ArithmExpr::Word(w),
                None => return Ok(None),
            },
        };
        if compact && self.spaced {
            return Ok(Some(x));
        }
        if !compact {
            self.got(Token::Newl)?;
        }
        if self.tok == Token::AddAdd || self.tok == Token::SubSub {
            if !is_arith_name(&x) {
                return self.cur_err(format!("{} must follow a name", self.tok.quoted()));
            }
            let u = UnaryArithm {
                op_pos: self.pos,
                op: self.tok,
                post: true,
                x,
            };
            self.next_arith(compact)?;
            return Ok(Some(ArithmExpr::Unary(Box::new(u))));
        }
        Ok(Some(x))
    }

    fn next_arith(&mut self, compact: bool) -> PResult<bool> {
        self.next()?;
        if compact && self.spaced {
            return Ok(true);
        }
        if !compact {
            self.got(Token::Newl)?;
        }
        Ok(false)
    }

    fn next_arith_op(&mut self, compact: bool) -> PResult<()> {
        let pos = self.pos;
        let tok = self.tok;
        if self.next_arith(compact)? {
            return self.follow_err_exp(pos, tok.quoted());
        }
        Ok(())
    }

    fn arithm_expr_binary(
        &mut self,
        compact: bool,
        next_op: Level<'a>,
        operators: &[Token],
    ) -> PResult<Option<ArithmExpr>> {
        let mut value = next_op(self, compact)?;
        loop {
            let Some(&op) = operators.iter().find(|&&op| op == self.tok) else {
                return Ok(value);
            };
            if compact && self.spaced {
                return Ok(value);
            }
            let Some(x) = value else {
                return self.cur_err(format!("{} must follow an expression", self.tok.quoted()));
            };
            let pos = self.pos;
            self.next_arith_op(compact)?;
            let Some(y) = next_op(self, compact)? else {
                return self.follow_err_exp(pos, op.quoted());
            };
            value = Some(binary(op, x, y));
        }
    }

    pub(super) fn follow_arithm(&mut self, ftok: Token, fpos: Pos) -> PResult<ArithmExpr> {
        match self.arithm_expr(false)? {
            Some(x) => Ok(x),
            None => self.follow_err_exp(fpos, ftok.quoted()),
        }
    }

    fn peek_arithm_end(&self) -> bool {
        self.tok == Token::RightParen && self.r.is(')')
    }

    pub(super) fn arithm_matching_err<T>(&self, pos: Pos, left: Token, right: Token) -> PResult<T> {
        match self.tok {
            Token::Lit | Token::LitWord => self.cur_err(format!(
                "not a valid arithmetic operator: {}",
                quote(&self.val)
            )),
            Token::LeftBrack => self.cur_err(format!(
                "{} must follow a name like a[i]",
                Token::LeftBrack.quoted()
            )),
            Token::Colon => self.cur_err(format!(
                "ternary operator missing {} before {}",
                Token::Quest.quoted(),
                Token::Colon.quoted()
            )),
            Token::RightParen | Token::Eof => self.matching_err(pos, left, right),
            Token::Period => {
                self.check_lang(self.pos, LANG_ZSH, "floating point arithmetic".into())?;
                self.matching_err(pos, left, right)
            }
            _ => {
                if self.quote & ALL_ARITHM_EXPR != 0 {
                    return self.cur_err(format!(
                        "not a valid arithmetic operator: {}",
                        self.tok.quoted()
                    ));
                }
                self.matching_err(pos, left, right)
            }
        }
    }

    pub(super) fn matched_arithm(&mut self, lpos: Pos, left: Token, right: Token) -> PResult<()> {
        if !self.got(right)? {
            return self.arithm_matching_err(lpos, left, right);
        }
        Ok(())
    }

    pub(super) fn arithm_end(&mut self, ltok: Token, lpos: Pos, old: SaveState) -> PResult<Pos> {
        if !self.peek_arithm_end() {
            return self.arithm_matching_err(lpos, ltok, Token::DblRightParen);
        }
        self.rune();
        self.post_nested(old);
        let pos = self.pos;
        self.next()?;
        Ok(pos)
    }
}
