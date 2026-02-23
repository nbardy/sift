use crate::core::{Binding, Expr, SearchOpts, Weights};

#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("unexpected end of input")]
    UnexpectedEof,
    #[error("expected ')' at position {0}")]
    ExpectedClose(usize),
    #[error("unknown form: {0}")]
    UnknownForm(String),
    #[error("expected string literal at position {0}")]
    ExpectedString(usize),
    #[error("expected number at position {0}")]
    ExpectedNumber(usize),
    #[error("{0}")]
    Other(String),
}

// ── Tokenizer ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Open,
    Close,
    BracketOpen,
    BracketClose,
    Str(String),
    Num(f64),
    Sym(String),
    Keyword(String),
}

fn tokenize(input: &str) -> Result<Vec<Token>, ParseError> {
    let mut tokens = Vec::new();
    let bytes = input.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b';' => {
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
            }
            b'(' => { tokens.push(Token::Open); i += 1; }
            b')' => { tokens.push(Token::Close); i += 1; }
            b'[' => { tokens.push(Token::BracketOpen); i += 1; }
            b']' => { tokens.push(Token::BracketClose); i += 1; }
            b'"' => {
                i += 1;
                let start = i;
                let mut s = String::new();
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                        match bytes[i] {
                            b'n' => s.push('\n'),
                            b't' => s.push('\t'),
                            b'\\' => s.push('\\'),
                            b'"' => s.push('"'),
                            other => { s.push('\\'); s.push(other as char); }
                        }
                    } else {
                        s.push(bytes[i] as char);
                    }
                    i += 1;
                }
                if i >= bytes.len() {
                    return Err(ParseError::Other(format!("unterminated string starting at {start}")));
                }
                i += 1;
                tokens.push(Token::Str(s));
            }
            b':' => {
                i += 1;
                let start = i;
                while i < bytes.len() && is_sym_char(bytes[i]) {
                    i += 1;
                }
                let kw = std::str::from_utf8(&bytes[start..i]).unwrap().to_string();
                tokens.push(Token::Keyword(kw));
            }
            c if c == b'-' || c == b'.' || c.is_ascii_digit() => {
                let start = i;
                if c == b'-' && (i + 1 >= bytes.len() || !bytes[i + 1].is_ascii_digit()) {
                    i += 1;
                    tokens.push(Token::Sym("-".to_string()));
                } else {
                    if bytes[i] == b'-' { i += 1; }
                    while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                        i += 1;
                    }
                    let num_str = std::str::from_utf8(&bytes[start..i]).unwrap();
                    let n: f64 = num_str.parse().map_err(|_| ParseError::ExpectedNumber(start))?;
                    tokens.push(Token::Num(n));
                }
            }
            c if is_sym_char(c) => {
                let start = i;
                while i < bytes.len() && is_sym_char(bytes[i]) {
                    i += 1;
                }
                let sym = std::str::from_utf8(&bytes[start..i]).unwrap().to_string();
                tokens.push(Token::Sym(sym));
            }
            other => {
                return Err(ParseError::Other(format!("unexpected character '{}' at position {i}", other as char)));
            }
        }
    }

    Ok(tokens)
}

fn is_sym_char(c: u8) -> bool {
    matches!(c, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'-' | b'&' | b'|' | b'>' | b'*' | b'/' | b'.' | b'!' | b'?' | b'+')
}

// ── Parser ──────────────────────────────────────────────────────────

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Result<Token, ParseError> {
        if self.pos >= self.tokens.len() {
            return Err(ParseError::UnexpectedEof);
        }
        let tok = self.tokens[self.pos].clone();
        self.pos += 1;
        Ok(tok)
    }

    fn expect_close(&mut self) -> Result<(), ParseError> {
        match self.next()? {
            Token::Close => Ok(()),
            _ => Err(ParseError::ExpectedClose(self.pos - 1)),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        match self.peek().ok_or(ParseError::UnexpectedEof)? {
            Token::Open => self.parse_form(),
            Token::Sym(_) => {
                let Token::Sym(name) = self.next()? else { unreachable!() };
                Ok(Expr::Var(name))
            }
            _ => Err(ParseError::Other(format!("unexpected token at position {}", self.pos))),
        }
    }

    fn parse_form(&mut self) -> Result<Expr, ParseError> {
        self.next()?;
        let head = match self.next()? {
            Token::Sym(s) => s,
            other => return Err(ParseError::UnknownForm(format!("{other:?}"))),
        };

        let expr = match head.as_str() {
            "rg"     => self.parse_search(|q, o| Expr::Rg(q, o))?,
            "lex"    => self.parse_search(|q, o| Expr::Lex(q, o))?,
            "sem"    => self.parse_search(|q, o| Expr::Sem(q, o))?,
            "&"      => self.parse_variadic(Expr::And)?,
            "|"      => self.parse_variadic(Expr::Or)?,
            "mix"    => self.parse_mix()?,
            "-"      => self.parse_diff()?,
            "pipe"   => self.parse_pipe()?,
            "top"    => self.parse_top()?,
            ">"      => self.parse_threshold()?,
            "let"    => self.parse_let()?,
            other    => return Err(ParseError::UnknownForm(other.to_string())),
        };

        self.expect_close()?;
        Ok(expr)
    }

    fn parse_search(&mut self, ctor: fn(String, SearchOpts) -> Expr) -> Result<Expr, ParseError> {
        let query = match self.next()? {
            Token::Str(s) => s,
            _ => return Err(ParseError::ExpectedString(self.pos - 1)),
        };
        let opts = self.parse_search_opts()?;
        Ok(ctor(query, opts))
    }

    fn parse_search_opts(&mut self) -> Result<SearchOpts, ParseError> {
        let mut opts = SearchOpts::default();
        while let Some(Token::Keyword(_)) = self.peek() {
            let Token::Keyword(kw) = self.next()? else { unreachable!() };
            let val = match self.next()? {
                Token::Str(s) => s,
                _ => return Err(ParseError::ExpectedString(self.pos - 1)),
            };
            match kw.as_str() {
                "in"  => opts.scope = Some(val),
                "lang" => opts.lang = Some(val),
                "x"   => opts.exclude.push(val),
                "i"   => opts.include.push(val),
                other => return Err(ParseError::Other(format!("unknown keyword :{other}"))),
            }
        }
        Ok(opts)
    }

    fn parse_variadic(&mut self, ctor: fn(Vec<Expr>) -> Expr) -> Result<Expr, ParseError> {
        let mut children = Vec::new();
        while self.peek() != Some(&Token::Close) {
            children.push(self.parse_expr()?);
        }
        Ok(ctor(children))
    }

    fn parse_mix(&mut self) -> Result<Expr, ParseError> {
        let weights = if self.peek() == Some(&Token::BracketOpen) {
            self.next()?;
            let mut ws = Vec::new();
            while self.peek() != Some(&Token::BracketClose) {
                match self.next()? {
                    Token::Num(n) => ws.push(n),
                    _ => return Err(ParseError::ExpectedNumber(self.pos - 1)),
                }
            }
            self.next()?;
            Weights::Explicit(ws)
        } else {
            Weights::Equal
        };
        let mut children = Vec::new();
        while self.peek() != Some(&Token::Close) {
            children.push(self.parse_expr()?);
        }
        Ok(Expr::Mix(weights, children))
    }

    fn parse_diff(&mut self) -> Result<Expr, ParseError> {
        let left = self.parse_expr()?;
        let right = self.parse_expr()?;
        Ok(Expr::Diff(Box::new(left), Box::new(right)))
    }

    /// Parse pipe: (pipe source target) or (>> source target)
    fn parse_pipe(&mut self) -> Result<Expr, ParseError> {
        let source = self.parse_expr()?;
        let target = self.parse_expr()?;
        Ok(Expr::Pipe(Box::new(source), Box::new(target)))
    }

    fn parse_top(&mut self) -> Result<Expr, ParseError> {
        let k = match self.next()? {
            Token::Num(n) => n as usize,
            _ => return Err(ParseError::ExpectedNumber(self.pos - 1)),
        };
        let child = self.parse_expr()?;
        Ok(Expr::Top(k, Box::new(child)))
    }

    fn parse_threshold(&mut self) -> Result<Expr, ParseError> {
        let t = match self.next()? {
            Token::Num(n) => n,
            _ => return Err(ParseError::ExpectedNumber(self.pos - 1)),
        };
        let child = self.parse_expr()?;
        Ok(Expr::Threshold(t, Box::new(child)))
    }

    fn parse_let(&mut self) -> Result<Expr, ParseError> {
        match self.next()? {
            Token::BracketOpen => {}
            _ => return Err(ParseError::Other(format!("expected '[' in let at position {}", self.pos - 1))),
        }
        let mut bindings = Vec::new();
        while self.peek() != Some(&Token::BracketClose) {
            let name = match self.next()? {
                Token::Sym(s) => s,
                _ => return Err(ParseError::Other(format!("expected variable name at position {}", self.pos - 1))),
            };
            let value = self.parse_expr()?;
            bindings.push(Binding { name, value });
        }
        self.next()?;
        let body = self.parse_expr()?;
        Ok(Expr::Let(bindings, Box::new(body)))
    }
}

// ── Public API ──────────────────────────────────────────────────────

pub fn parse(input: &str) -> Result<Expr, ParseError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(tokens);
    let expr = parser.parse_expr()?;
    if parser.pos < parser.tokens.len() {
        return Err(ParseError::Other(format!("trailing tokens at position {}", parser.pos)));
    }
    Ok(expr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_rg() {
        let expr = parse(r#"(rg "TODO")"#).unwrap();
        assert_eq!(expr, Expr::Rg("TODO".into(), SearchOpts::default()));
    }

    #[test]
    fn parse_rg_with_opts() {
        let expr = parse(r#"(rg "pattern" :lang "rust" :x "*test*")"#).unwrap();
        assert_eq!(
            expr,
            Expr::Rg("pattern".into(), SearchOpts {
                lang: Some("rust".into()),
                exclude: vec!["*test*".into()],
                ..Default::default()
            })
        );
    }

    #[test]
    fn parse_intersection() {
        let expr = parse(r#"(& (rg "async") (rg "tokio"))"#).unwrap();
        assert_eq!(expr, Expr::And(vec![
            Expr::Rg("async".into(), SearchOpts::default()),
            Expr::Rg("tokio".into(), SearchOpts::default()),
        ]));
    }

    #[test]
    fn parse_diff() {
        let expr = parse(r#"(- (rg "fn") (rg "test"))"#).unwrap();
        assert_eq!(expr, Expr::Diff(
            Box::new(Expr::Rg("fn".into(), SearchOpts::default())),
            Box::new(Expr::Rg("test".into(), SearchOpts::default())),
        ));
    }

    #[test]
    fn parse_pipe() {
        let expr = parse(r#"(pipe (rg "auth") (rg "TODO"))"#).unwrap();
        assert_eq!(expr, Expr::Pipe(
            Box::new(Expr::Rg("auth".into(), SearchOpts::default())),
            Box::new(Expr::Rg("TODO".into(), SearchOpts::default())),
        ));
    }

    #[test]
    fn parse_top() {
        let expr = parse(r#"(top 5 (rg "TODO"))"#).unwrap();
        assert_eq!(expr, Expr::Top(5, Box::new(Expr::Rg("TODO".into(), SearchOpts::default()))));
    }

    #[test]
    fn parse_mix_equal() {
        let expr = parse(r#"(mix (rg "x") (rg "y"))"#).unwrap();
        assert_eq!(expr, Expr::Mix(Weights::Equal, vec![
            Expr::Rg("x".into(), SearchOpts::default()),
            Expr::Rg("y".into(), SearchOpts::default()),
        ]));
    }

    #[test]
    fn parse_mix_weighted() {
        let expr = parse(r#"(mix [0.6 0.4] (rg "x") (rg "y"))"#).unwrap();
        assert_eq!(expr, Expr::Mix(Weights::Explicit(vec![0.6, 0.4]), vec![
            Expr::Rg("x".into(), SearchOpts::default()),
            Expr::Rg("y".into(), SearchOpts::default()),
        ]));
    }

    #[test]
    fn parse_let_binding() {
        let expr = parse(r#"(let [x (rg "foo")] (top 5 x))"#).unwrap();
        assert_eq!(expr, Expr::Let(
            vec![Binding { name: "x".into(), value: Expr::Rg("foo".into(), SearchOpts::default()) }],
            Box::new(Expr::Top(5, Box::new(Expr::Var("x".into())))),
        ));
    }

    #[test]
    fn parse_nested() {
        let expr = parse(r#"(top 10 (mix [0.6 0.4] (sem "retry with exponential backoff") (rg "retry|backoff")))"#).unwrap();
        assert_eq!(expr, Expr::Top(10, Box::new(Expr::Mix(
            Weights::Explicit(vec![0.6, 0.4]),
            vec![
                Expr::Sem("retry with exponential backoff".into(), SearchOpts::default()),
                Expr::Rg("retry|backoff".into(), SearchOpts::default()),
            ]
        ))));
    }

    #[test]
    fn parse_comments() {
        let expr = parse(";; find TODOs\n(rg \"TODO\")").unwrap();
        assert_eq!(expr, Expr::Rg("TODO".into(), SearchOpts::default()));
    }

    #[test]
    fn parse_threshold() {
        let expr = parse(r#"(> 0.5 (rg "auth"))"#).unwrap();
        assert_eq!(expr, Expr::Threshold(0.5, Box::new(Expr::Rg("auth".into(), SearchOpts::default()))));
    }

}
