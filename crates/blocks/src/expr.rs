//! A small expression language that binds a block's argument to the plugin's parameters.
//!
//! An argument can be a number or an expression such as `"drive"`, `"tone * 2"` or `"sync(0.5) * 0.75"`.
//!
//! Names: any parameter id, any modulator id, `rate` (the sample rate), `bpm` and `pi`.
//! Operators: `+ - * / ^` and the comparisons `< > <= >=`, which give 1 or 0. Division by zero gives 0.
//! Functions:
//! - `min(a,b)`, `max(a,b)`, `clamp(x,lo,hi)`, `abs(x)`, `pow(a,b)`, `exp(x)`, `ln(x)`, `log2(x)`, `sqrt(x)`
//! - `sin(x)`, `cos(x)` in radians, `tanh(x)`, `floor(x)`, `round(x)`
//! - `db2lin(db)`, `lin2db(x)`
//! - `semitones(n)`, the ratio for `n` semitones, and `note(n)`, the frequency in Hz of MIDI note `n`
//! - `mix(a,b,t)`, which is `a + (b - a) * t`
//! - `if(c,a,b)`, which gives `a` when `c` is 0.5 or more and `b` otherwise
//! - `sync(beats)`, the length of that many quarter notes in milliseconds at the host's tempo

#[derive(Clone, Debug)]
enum Node {
    Num(f32),
    Var(usize),
    Rate,
    Bpm,
    Neg(Box<Node>),
    Bin(Op, Box<Node>, Box<Node>),
    Call(Fun, Vec<Node>),
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Pow,
    Lt,
    Gt,
    Le,
    Ge,
}

#[derive(Clone, Copy, Debug)]
enum Fun {
    Min,
    Max,
    Clamp,
    Abs,
    Pow,
    Exp,
    Ln,
    Log2,
    Sqrt,
    Sin,
    Cos,
    Tanh,
    Floor,
    Round,
    Db2Lin,
    Lin2Db,
    Semitones,
    Note,
    Mix,
    If,
    Sync,
}

impl Fun {
    fn named(name: &str) -> Option<(Fun, usize)> {
        Some(match name {
            "min" => (Fun::Min, 2),
            "max" => (Fun::Max, 2),
            "clamp" => (Fun::Clamp, 3),
            "abs" => (Fun::Abs, 1),
            "pow" => (Fun::Pow, 2),
            "exp" => (Fun::Exp, 1),
            "ln" => (Fun::Ln, 1),
            "log2" => (Fun::Log2, 1),
            "sqrt" => (Fun::Sqrt, 1),
            "sin" => (Fun::Sin, 1),
            "cos" => (Fun::Cos, 1),
            "tanh" => (Fun::Tanh, 1),
            "floor" => (Fun::Floor, 1),
            "round" => (Fun::Round, 1),
            "db2lin" => (Fun::Db2Lin, 1),
            "lin2db" => (Fun::Lin2Db, 1),
            "semitones" => (Fun::Semitones, 1),
            "note" => (Fun::Note, 1),
            "mix" => (Fun::Mix, 3),
            "if" => (Fun::If, 3),
            "sync" => (Fun::Sync, 1),
            _ => return None,
        })
    }
}

/// What an expression reads while it runs.
pub struct Env<'a> {
    /// The plugin's parameter values, then the modulators' values.
    pub vars: &'a [f32],
    pub rate: f32,
    pub bpm: f32,
}

/// A compiled expression. Evaluating it does not allocate.
#[derive(Clone, Debug)]
pub struct Expr(Node);

impl Expr {
    pub fn number(v: f32) -> Expr {
        Expr(Node::Num(v))
    }

    /// `lookup` turns a name into the index of its value in `Env::vars`.
    pub fn parse(src: &str, lookup: &dyn Fn(&str) -> Option<usize>) -> Result<Expr, String> {
        let mut p = Parser {
            s: src.chars().collect(),
            pos: 0,
            lookup,
        };
        p.skip();
        if p.pos >= p.s.len() {
            return Err("the expression is empty".to_string());
        }
        let node = p.cmp()?;
        p.skip();
        if p.pos < p.s.len() {
            return Err(format!("unexpected '{}' after the end of the expression '{}'", p.s[p.pos], src));
        }
        Ok(Expr(node))
    }

    pub fn eval(&self, env: &Env) -> f32 {
        eval(&self.0, env)
    }

    /// The value, if it does not depend on any parameter, modulator, sample rate or tempo.
    pub fn constant(&self) -> Option<f32> {
        if is_const(&self.0) {
            Some(eval(
                &self.0,
                &Env {
                    vars: &[],
                    rate: 48000.0,
                    bpm: 120.0,
                },
            ))
        } else {
            None
        }
    }
}

fn is_const(n: &Node) -> bool {
    match n {
        Node::Num(_) => true,
        Node::Var(_) | Node::Rate | Node::Bpm => false,
        Node::Neg(a) => is_const(a),
        Node::Bin(_, a, b) => is_const(a) && is_const(b),
        Node::Call(Fun::Sync, _) => false,
        Node::Call(_, args) => args.iter().all(is_const),
    }
}

fn eval(n: &Node, e: &Env) -> f32 {
    match n {
        Node::Num(v) => *v,
        Node::Var(i) => e.vars.get(*i).copied().unwrap_or(0.0),
        Node::Rate => e.rate,
        Node::Bpm => e.bpm,
        Node::Neg(a) => -eval(a, e),
        Node::Bin(op, a, b) => {
            let x = eval(a, e);
            let y = eval(b, e);
            match op {
                Op::Add => x + y,
                Op::Sub => x - y,
                Op::Mul => x * y,
                Op::Div => {
                    if y == 0.0 {
                        0.0
                    } else {
                        x / y
                    }
                }
                Op::Pow => x.powf(y),
                Op::Lt => (x < y) as u8 as f32,
                Op::Gt => (x > y) as u8 as f32,
                Op::Le => (x <= y) as u8 as f32,
                Op::Ge => (x >= y) as u8 as f32,
            }
        }
        Node::Call(f, args) => {
            let a = |i: usize| eval(&args[i], e);
            match f {
                Fun::Min => a(0).min(a(1)),
                Fun::Max => a(0).max(a(1)),
                Fun::Clamp => a(0).max(a(1)).min(a(2).max(a(1))),
                Fun::Abs => a(0).abs(),
                Fun::Pow => a(0).powf(a(1)),
                Fun::Exp => a(0).exp(),
                Fun::Ln => a(0).max(1e-30).ln(),
                Fun::Log2 => a(0).max(1e-30).log2(),
                Fun::Sqrt => a(0).max(0.0).sqrt(),
                Fun::Sin => a(0).sin(),
                Fun::Cos => a(0).cos(),
                Fun::Tanh => a(0).tanh(),
                Fun::Floor => a(0).floor(),
                Fun::Round => a(0).round(),
                Fun::Db2Lin => (a(0) * 0.115_129_25).exp(),
                Fun::Lin2Db => 20.0 * a(0).max(1e-9).log10(),
                Fun::Semitones => (a(0) / 12.0).exp2(),
                Fun::Note => 440.0 * ((a(0) - 69.0) / 12.0).exp2(),
                Fun::Mix => {
                    let (x, y, t) = (a(0), a(1), a(2));
                    x + (y - x) * t
                }
                Fun::If => {
                    if a(0) >= 0.5 {
                        a(1)
                    } else {
                        a(2)
                    }
                }
                Fun::Sync => 60000.0 / e.bpm.max(1.0) * a(0),
            }
        }
    }
}

struct Parser<'a> {
    s: Vec<char>,
    pos: usize,
    lookup: &'a dyn Fn(&str) -> Option<usize>,
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.pos < self.s.len() && self.s[self.pos].is_whitespace() {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<char> {
        self.skip();
        self.s.get(self.pos).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        if self.peek() == Some(c) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    fn cmp(&mut self) -> Result<Node, String> {
        let left = self.add()?;
        let op = match (self.peek(), self.s.get(self.pos + 1).copied()) {
            (Some('<'), Some('=')) => {
                self.pos += 2;
                Op::Le
            }
            (Some('>'), Some('=')) => {
                self.pos += 2;
                Op::Ge
            }
            (Some('<'), _) => {
                self.pos += 1;
                Op::Lt
            }
            (Some('>'), _) => {
                self.pos += 1;
                Op::Gt
            }
            _ => return Ok(left),
        };
        let right = self.add()?;
        Ok(Node::Bin(op, Box::new(left), Box::new(right)))
    }

    fn add(&mut self) -> Result<Node, String> {
        let mut left = self.mul()?;
        loop {
            let op = match self.peek() {
                Some('+') => Op::Add,
                Some('-') => Op::Sub,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.mul()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn mul(&mut self) -> Result<Node, String> {
        let mut left = self.unary()?;
        loop {
            let op = match self.peek() {
                Some('*') => Op::Mul,
                Some('/') => Op::Div,
                _ => return Ok(left),
            };
            self.pos += 1;
            let right = self.unary()?;
            left = Node::Bin(op, Box::new(left), Box::new(right));
        }
    }

    fn unary(&mut self) -> Result<Node, String> {
        if self.eat('-') {
            return Ok(Node::Neg(Box::new(self.unary()?)));
        }
        if self.eat('+') {
            return self.unary();
        }
        let base = self.atom()?;
        if self.eat('^') {
            let exp = self.unary()?;
            return Ok(Node::Bin(Op::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('(') => {
                self.pos += 1;
                let inner = self.cmp()?;
                if !self.eat(')') {
                    return Err("a '(' is never closed".to_string());
                }
                Ok(inner)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => self.number(),
            Some(c) if c.is_ascii_alphabetic() || c == '_' => self.name(),
            Some(c) => Err(format!("expected a number or a name but found '{}'", c)),
            None => Err("the expression ends where a number or a name should be".to_string()),
        }
    }

    fn number(&mut self) -> Result<Node, String> {
        let start = self.pos;
        while self.pos < self.s.len() && (self.s[self.pos].is_ascii_digit() || self.s[self.pos] == '.') {
            self.pos += 1;
        }
        if self.pos < self.s.len() && (self.s[self.pos] == 'e' || self.s[self.pos] == 'E') {
            let save = self.pos;
            self.pos += 1;
            if self.pos < self.s.len() && (self.s[self.pos] == '+' || self.s[self.pos] == '-') {
                self.pos += 1;
            }
            if self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                while self.pos < self.s.len() && self.s[self.pos].is_ascii_digit() {
                    self.pos += 1;
                }
            } else {
                self.pos = save;
            }
        }
        let text: String = self.s[start..self.pos].iter().collect();
        text.parse::<f32>()
            .map(Node::Num)
            .map_err(|_| format!("'{}' is not a number", text))
    }

    fn name(&mut self) -> Result<Node, String> {
        let start = self.pos;
        while self.pos < self.s.len() && (self.s[self.pos].is_ascii_alphanumeric() || self.s[self.pos] == '_') {
            self.pos += 1;
        }
        let name: String = self.s[start..self.pos].iter().collect();
        if self.peek() == Some('(') {
            let Some((fun, arity)) = Fun::named(&name) else {
                return Err(format!("there is no function named '{}'", name));
            };
            self.pos += 1;
            let mut args = Vec::new();
            if !self.eat(')') {
                loop {
                    args.push(self.cmp()?);
                    if self.eat(',') {
                        continue;
                    }
                    if self.eat(')') {
                        break;
                    }
                    return Err(format!("the call to '{}' needs a ',' or a ')'", name));
                }
            }
            if args.len() != arity {
                return Err(format!("'{}' takes {} value(s) but was given {}", name, arity, args.len()));
            }
            return Ok(Node::Call(fun, args));
        }
        match name.as_str() {
            "rate" => Ok(Node::Rate),
            "bpm" => Ok(Node::Bpm),
            "pi" => Ok(Node::Num(std::f32::consts::PI)),
            _ => match (self.lookup)(&name) {
                Some(i) => Ok(Node::Var(i)),
                None => Err(format!("'{}' is not a parameter, a modulator or a built-in name", name)),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str, vars: &[f32]) -> f32 {
        let names = ["a", "b"];
        let e = Expr::parse(src, &|n| names.iter().position(|x| *x == n)).unwrap();
        e.eval(&Env {
            vars,
            rate: 48000.0,
            bpm: 120.0,
        })
    }

    #[test]
    fn arithmetic_follows_the_usual_precedence() {
        assert_eq!(run("1 + 2 * 3", &[]), 7.0);
        assert_eq!(run("(1 + 2) * 3", &[]), 9.0);
        assert_eq!(run("10 - 4 - 3", &[]), 3.0);
        assert_eq!(run("8 / 4 / 2", &[]), 1.0);
        assert_eq!(run("2 ^ 3 ^ 2", &[]), 512.0, "powers group to the right");
        assert_eq!(run("-2 ^ 2", &[]), -4.0, "the power binds tighter than the minus");
        assert_eq!(run("2 ^ -1", &[]), 0.5);
        assert_eq!(run("1.5e2 + 0.25", &[]), 150.25);
    }

    #[test]
    fn names_read_parameters_and_the_built_ins() {
        assert_eq!(run("a * 2 + b", &[3.0, 4.0]), 10.0);
        assert_eq!(run("rate / 2", &[]), 24000.0);
        assert_eq!(run("bpm", &[]), 120.0);
        assert!((run("pi", &[]) - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn functions_and_comparisons_work() {
        assert_eq!(run("min(a, b)", &[3.0, 4.0]), 3.0);
        assert_eq!(run("max(a, b)", &[3.0, 4.0]), 4.0);
        assert_eq!(run("clamp(a, 0, 1)", &[5.0]), 1.0);
        assert_eq!(run("clamp(a, 0, 1)", &[-5.0]), 0.0);
        assert!((run("db2lin(-6.0206)", &[]) - 0.5).abs() < 1e-4);
        assert!((run("lin2db(0.5)", &[]) + 6.0206).abs() < 1e-3);
        assert!((run("note(69)", &[]) - 440.0).abs() < 1e-3);
        assert!((run("note(81)", &[]) - 880.0).abs() < 1e-2);
        assert!((run("semitones(12)", &[]) - 2.0).abs() < 1e-5);
        assert_eq!(run("mix(2, 6, 0.25)", &[]), 3.0);
        assert_eq!(run("if(a > 0.5, 10, 20)", &[1.0]), 10.0);
        assert_eq!(run("if(a > 0.5, 10, 20)", &[0.0]), 20.0);
        assert_eq!(run("a <= b", &[2.0, 2.0]), 1.0);
        assert_eq!(run("a >= b", &[1.0, 2.0]), 0.0);
        assert_eq!(run("sync(1)", &[]), 500.0, "a quarter note at 120 bpm is 500 ms");
        assert_eq!(run("sync(0.5)", &[]), 250.0);
    }

    #[test]
    fn division_by_zero_and_logs_of_zero_stay_finite() {
        assert_eq!(run("1 / 0", &[]), 0.0);
        assert!(run("ln(0)", &[]).is_finite());
        assert!(run("lin2db(0)", &[]).is_finite());
        assert!(run("sqrt(0 - 4)", &[]).is_finite());
    }

    #[test]
    fn mistakes_come_with_a_message_that_says_what_is_wrong() {
        let names: [&str; 1] = ["a"];
        let err = |s: &str| Expr::parse(s, &|n| names.iter().position(|x| *x == n)).unwrap_err();
        assert!(err("zzz + 1").contains("'zzz' is not a parameter"));
        assert!(err("min(1)").contains("takes 2"));
        assert!(err("nope(1)").contains("no function named 'nope'"));
        assert!(err("(1 + 2").contains("never closed"));
        assert!(err("1 + ").contains("ends where"));
        assert!(err("1 2").contains("unexpected"));
        assert!(err("   ").contains("empty"));
    }

    #[test]
    fn constants_are_recognised() {
        let none = |_: &str| None;
        assert_eq!(Expr::parse("2 * 3", &none).unwrap().constant(), Some(6.0));
        assert_eq!(Expr::parse("rate", &none).unwrap().constant(), None);
        assert_eq!(Expr::parse("sync(1)", &none).unwrap().constant(), None);
        let e = Expr::parse("a", &|n| (n == "a").then_some(0)).unwrap();
        assert_eq!(e.constant(), None);
    }
}
