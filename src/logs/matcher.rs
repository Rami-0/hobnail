//! A small regular-expression matcher for `log_match` alert rules, so the
//! server needs no `regex` crate (about 1 MB of binary). It answers only
//! "does this line match", in time linear in the line length (a Pike VM
//! without captures: no backtracking, so no pattern can hang the task).
//!
//! Supported: literals, `.`, `[...]` / `[^...]` with ranges and `\d \w \s`,
//! `\d \D \w \W \s \S \b \B`, escapes (`\.`, `\t`, `\n`, `\xHH`), `^ $`,
//! groups `( )` and `(?: )`, alternation `|`, `* + ?` and `{n}`, `{n,}`,
//! `{n,m}` (lazy `?` suffixes are accepted; they do not change a yes/no
//! answer), and a leading `(?i)` for case-insensitive matching.
//! Not supported (rejected with a message): backreferences, lookaround,
//! named groups, Unicode classes `\p{..}`.

const MAX_INSTS: usize = 20_000;
const MAX_REPEAT: u32 = 1000;

#[derive(Debug, Clone)]
pub struct Regex {
    prog: Vec<Inst>,
    icase: bool,
    /// Pattern starts with `^`: only try at position 0.
    anchored: bool,
}

#[derive(Debug, Clone)]
enum Inst {
    Char(char),
    Any,
    Class(Box<Class>),
    Split(usize, usize),
    Jmp(usize),
    Bol,
    Eol,
    WordB(bool),
    Match,
}

#[derive(Debug, Clone, Default)]
struct Class {
    neg: bool,
    ranges: Vec<(char, char)>,
    /// Shorthand classes inside brackets: (kind, negated).
    perl: Vec<(u8, bool)>,
}

impl Class {
    fn matches(&self, c: char, icase: bool) -> bool {
        let hit = |c: char| {
            self.ranges.iter().any(|&(a, b)| a <= c && c <= b)
                || self.perl.iter().any(|&(k, n)| perl(k, c) != n)
        };
        let mut m = hit(c);
        if !m && icase {
            let lo = c.to_lowercase().next().unwrap_or(c);
            let up = c.to_uppercase().next().unwrap_or(c);
            m = hit(lo) || hit(up);
        }
        m != self.neg
    }
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn perl(kind: u8, c: char) -> bool {
    match kind {
        b'd' => c.is_ascii_digit(),
        b'w' => is_word(c),
        _ => c.is_whitespace(),
    }
}

#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Class(Class),
    Bol,
    Eol,
    WordB(bool),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat(Box<Node>, u32, Option<u32>),
}

struct P<'a> {
    s: &'a [char],
    i: usize,
    depth: usize,
}

impl P<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn next(&mut self) -> Option<char> {
        let c = self.peek();
        self.i += 1;
        c
    }

    fn alt(&mut self) -> Result<Node, String> {
        let mut alts = vec![self.concat()?];
        while self.peek() == Some('|') {
            self.i += 1;
            alts.push(self.concat()?);
        }
        Ok(if alts.len() == 1 {
            alts.pop().unwrap()
        } else {
            Node::Alt(alts)
        })
    }

    fn concat(&mut self) -> Result<Node, String> {
        let mut v = vec![];
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            let atom = self.atom()?;
            v.push(self.quant(atom)?);
        }
        Ok(Node::Concat(v))
    }

    fn quant(&mut self, atom: Node) -> Result<Node, String> {
        let mut node = atom;
        loop {
            let (min, max) = match self.peek() {
                Some('*') => (0, None),
                Some('+') => (1, None),
                Some('?') => (0, Some(1)),
                Some('{') => match self.braces()? {
                    Some(r) => {
                        self.i -= 1; // braces() consumed it all; undo the step below
                        r
                    }
                    None => return Ok(node),
                },
                _ => return Ok(node),
            };
            self.i += 1;
            if matches!(node, Node::Bol | Node::Eol | Node::WordB(_)) {
                return Err("a quantifier needs something to repeat".into());
            }
            // Lazy / possessive suffix: same yes/no answer.
            if matches!(self.peek(), Some('?') | Some('+')) {
                self.i += 1;
            }
            node = Node::Repeat(Box::new(node), min, max);
        }
    }

    /// `{n}`, `{n,}`, `{n,m}`; a `{` that is not a valid repeat is a literal
    /// (returns None and leaves the position alone).
    fn braces(&mut self) -> Result<Option<(u32, Option<u32>)>, String> {
        let start = self.i;
        self.i += 1;
        let num = |p: &mut Self| {
            let st = p.i;
            while p.peek().is_some_and(|c| c.is_ascii_digit()) {
                p.i += 1;
            }
            p.s[st..p.i].iter().collect::<String>().parse::<u32>().ok()
        };
        let Some(min) = num(self) else {
            self.i = start;
            return Ok(None);
        };
        let max = if self.peek() == Some(',') {
            self.i += 1;
            if self.peek() == Some('}') {
                None
            } else {
                match num(self) {
                    Some(m) => Some(m),
                    None => {
                        self.i = start;
                        return Ok(None);
                    }
                }
            }
        } else {
            Some(min)
        };
        if self.peek() != Some('}') {
            self.i = start;
            return Ok(None);
        }
        self.i += 1;
        if max.is_some_and(|m| m < min) {
            return Err(format!("bad repeat {{{min},{}}}", max.unwrap_or(0)));
        }
        if min > MAX_REPEAT || max.unwrap_or(0) > MAX_REPEAT {
            return Err(format!(
                "repeat counts above {MAX_REPEAT} are not supported"
            ));
        }
        Ok(Some((min, max)))
    }

    fn atom(&mut self) -> Result<Node, String> {
        let c = self.next().ok_or("unexpected end")?;
        Ok(match c {
            '.' => Node::Any,
            '^' => Node::Bol,
            '$' => Node::Eol,
            '(' => {
                self.depth += 1;
                if self.depth > 100 {
                    return Err("groups nested too deeply".into());
                }
                if self.peek() == Some('?') {
                    self.i += 1;
                    match self.next() {
                        Some(':') => {}
                        Some('=') | Some('!') | Some('<') => {
                            return Err("lookaround is not supported".into())
                        }
                        Some('P') => return Err("named groups are not supported".into()),
                        _ => return Err("unsupported group syntax (?…)".into()),
                    }
                }
                let n = self.alt()?;
                if self.next() != Some(')') {
                    return Err("missing )".into());
                }
                self.depth -= 1;
                n
            }
            ')' => return Err("unmatched )".into()),
            '*' | '+' | '?' => return Err(format!("nothing to repeat before {c}")),
            '[' => Node::Class(self.class()?),
            '\\' => self.escape(false)?,
            c => Node::Char(c),
        })
    }

    fn escape(&mut self, in_class: bool) -> Result<Node, String> {
        let c = self.next().ok_or("pattern ends with \\")?;
        let shorthand = |k: u8, n: bool| {
            Node::Class(Class {
                perl: vec![(k, n)],
                ..Default::default()
            })
        };
        Ok(match c {
            'd' => shorthand(b'd', false),
            'D' => shorthand(b'd', true),
            'w' => shorthand(b'w', false),
            'W' => shorthand(b'w', true),
            's' => shorthand(b's', false),
            'S' => shorthand(b's', true),
            'b' if !in_class => Node::WordB(true),
            'B' if !in_class => Node::WordB(false),
            'n' => Node::Char('\n'),
            't' => Node::Char('\t'),
            'r' => Node::Char('\r'),
            'x' => {
                let h: String = [self.next(), self.next()].iter().flatten().collect();
                let v = u32::from_str_radix(&h, 16).map_err(|_| "bad \\x escape")?;
                Node::Char(char::from_u32(v).ok_or("bad \\x escape")?)
            }
            '1'..='9' => return Err("backreferences are not supported".into()),
            'p' | 'P' => return Err("Unicode classes (\\p) are not supported".into()),
            c if c.is_ascii_alphanumeric() => return Err(format!("unknown escape \\{c}")),
            c => Node::Char(c),
        })
    }

    fn class(&mut self) -> Result<Class, String> {
        let mut cl = Class::default();
        if self.peek() == Some('^') {
            self.i += 1;
            cl.neg = true;
        }
        let mut first = true;
        loop {
            let c = self.next().ok_or("missing ]")?;
            if c == ']' && !first {
                break;
            }
            first = false;
            let lo = if c == '\\' {
                match self.escape(true)? {
                    Node::Char(c) => c,
                    Node::Class(inner) => {
                        cl.perl.extend(inner.perl);
                        continue;
                    }
                    _ => return Err("bad escape in []".into()),
                }
            } else {
                c
            };
            if self.peek() == Some('-') && self.s.get(self.i + 1).is_some_and(|&c| c != ']') {
                self.i += 1;
                let mut hi = self.next().ok_or("missing ]")?;
                if hi == '\\' {
                    match self.escape(true)? {
                        Node::Char(c) => hi = c,
                        _ => return Err("bad range in []".into()),
                    }
                }
                if hi < lo {
                    return Err(format!("bad range {lo}-{hi}"));
                }
                cl.ranges.push((lo, hi));
            } else {
                cl.ranges.push((lo, lo));
            }
        }
        Ok(cl)
    }
}

struct Compiler {
    prog: Vec<Inst>,
}

impl Compiler {
    fn emit(&mut self, i: Inst) -> Result<usize, String> {
        if self.prog.len() >= MAX_INSTS {
            return Err("pattern is too complex".into());
        }
        self.prog.push(i);
        Ok(self.prog.len() - 1)
    }

    fn node(&mut self, n: &Node) -> Result<(), String> {
        match n {
            Node::Char(c) => {
                self.emit(Inst::Char(*c))?;
            }
            Node::Any => {
                self.emit(Inst::Any)?;
            }
            Node::Class(c) => {
                self.emit(Inst::Class(Box::new(c.clone())))?;
            }
            Node::Bol => {
                self.emit(Inst::Bol)?;
            }
            Node::Eol => {
                self.emit(Inst::Eol)?;
            }
            Node::WordB(b) => {
                self.emit(Inst::WordB(*b))?;
            }
            Node::Concat(v) => {
                for x in v {
                    self.node(x)?;
                }
            }
            Node::Alt(v) => {
                let mut jumps = vec![];
                for (k, x) in v.iter().enumerate() {
                    if k + 1 < v.len() {
                        let split = self.emit(Inst::Split(0, 0))?;
                        self.node(x)?;
                        jumps.push(self.emit(Inst::Jmp(0))?);
                        let next = self.prog.len();
                        self.prog[split] = Inst::Split(split + 1, next);
                    } else {
                        self.node(x)?;
                    }
                }
                let end = self.prog.len();
                for j in jumps {
                    self.prog[j] = Inst::Jmp(end);
                }
            }
            Node::Repeat(x, min, max) => {
                for _ in 0..*min {
                    self.node(x)?;
                }
                match max {
                    None => {
                        // x* : L: split(body, end); body; jmp L
                        let split = self.emit(Inst::Split(0, 0))?;
                        self.node(x)?;
                        self.emit(Inst::Jmp(split))?;
                        let end = self.prog.len();
                        self.prog[split] = Inst::Split(split + 1, end);
                    }
                    Some(max) => {
                        let mut splits = vec![];
                        for _ in *min..*max {
                            splits.push(self.emit(Inst::Split(0, 0))?);
                            self.node(x)?;
                        }
                        let end = self.prog.len();
                        for s in splits {
                            self.prog[s] = Inst::Split(s + 1, end);
                        }
                    }
                }
            }
        }
        Ok(())
    }
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self, String> {
        let (icase, body) = match pattern.strip_prefix("(?i)") {
            Some(rest) => (true, rest),
            None => (false, pattern),
        };
        let chars: Vec<char> = body.chars().collect();
        let mut p = P {
            s: &chars,
            i: 0,
            depth: 0,
        };
        let ast = p.alt()?;
        if p.i < chars.len() {
            return Err(if chars[p.i] == ')' {
                "unmatched )".into()
            } else {
                format!("unexpected {:?}", chars[p.i])
            });
        }
        let anchored = matches!(&ast, Node::Concat(v) if matches!(v.first(), Some(Node::Bol)));
        let mut c = Compiler { prog: vec![] };
        c.node(&ast)?;
        c.emit(Inst::Match)?;
        let mut prog = c.prog;
        if icase {
            for i in &mut prog {
                if let Inst::Char(ch) = i {
                    *ch = fold(*ch);
                }
            }
        }
        Ok(Self {
            prog,
            icase,
            anchored,
        })
    }

    pub fn is_match(&self, text: &str) -> bool {
        let n = self.prog.len();
        let mut clist = Threads::new(n);
        let mut nlist = Threads::new(n);
        let mut prev: Option<char> = None;
        let mut it = text.chars().peekable();
        let mut at_start = true;
        loop {
            let cur = it.peek().copied();
            if at_start || !self.anchored {
                self.add(&mut clist, 0, prev, cur);
                at_start = false;
            }
            if clist.list.is_empty() && (self.anchored || cur.is_none()) {
                return false;
            }
            for &pc in &clist.list {
                match &self.prog[pc] {
                    Inst::Match => return true,
                    Inst::Char(ch) => {
                        if let Some(c) = cur {
                            let c = if self.icase { fold(c) } else { c };
                            if c == *ch {
                                self.add(&mut nlist, pc + 1, cur, peek2(&it));
                            }
                        }
                    }
                    Inst::Any if cur.is_some_and(|c| c != '\n') => {
                        self.add(&mut nlist, pc + 1, cur, peek2(&it));
                    }
                    Inst::Class(cl) if cur.is_some_and(|c| cl.matches(c, self.icase)) => {
                        self.add(&mut nlist, pc + 1, cur, peek2(&it));
                    }
                    _ => {}
                }
            }
            if cur.is_none() {
                return false;
            }
            it.next();
            prev = cur;
            std::mem::swap(&mut clist, &mut nlist);
            nlist.clear();
        }
    }

    /// Follow empty transitions from `pc` at a position between `prev` and `next`.
    fn add(&self, t: &mut Threads, pc: usize, prev: Option<char>, next: Option<char>) {
        let mut stack = vec![pc];
        while let Some(pc) = stack.pop() {
            if !t.insert(pc) {
                continue;
            }
            match &self.prog[pc] {
                Inst::Jmp(x) => stack.push(*x),
                Inst::Split(a, b) => {
                    stack.push(*b);
                    stack.push(*a);
                }
                Inst::Bol => {
                    if prev.is_none() {
                        stack.push(pc + 1)
                    }
                }
                Inst::Eol => {
                    if next.is_none() {
                        stack.push(pc + 1)
                    }
                }
                Inst::WordB(want) => {
                    let b = prev.is_some_and(is_word) != next.is_some_and(is_word);
                    if b == *want {
                        stack.push(pc + 1)
                    }
                }
                _ => {}
            }
        }
    }
}

fn peek2(it: &std::iter::Peekable<std::str::Chars<'_>>) -> Option<char> {
    let mut c = it.clone();
    c.next();
    c.next()
}

fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c.to_lowercase().next().unwrap_or(c)
    }
}

/// Sparse set of program counters (insertion order kept: leftmost-first).
struct Threads {
    list: Vec<usize>,
    seen: Vec<bool>,
}

impl Threads {
    fn new(n: usize) -> Self {
        Self {
            list: Vec::with_capacity(16),
            seen: vec![false; n],
        }
    }

    fn insert(&mut self, pc: usize) -> bool {
        if self.seen[pc] {
            return false;
        }
        self.seen[pc] = true;
        self.list.push(pc);
        true
    }

    fn clear(&mut self) {
        for &pc in &self.list {
            self.seen[pc] = false;
        }
        self.list.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(p: &str, t: &str) -> bool {
        Regex::new(p).unwrap().is_match(t)
    }

    #[test]
    fn basics() {
        assert!(m("error", "an error here"));
        assert!(!m("error", "an Error here"));
        assert!(m("(?i)error", "an ERROR here"));
        assert!(m("^ERR", "ERR x"));
        assert!(!m("^ERR", "x ERR"));
        assert!(m("done$", "all done"));
        assert!(!m("done$", "done twice"));
        assert!(m("a.c", "xabcx"));
        assert!(m("colou?r", "color"));
        assert!(m("colou?r", "colour"));
        assert!(m("ab+c", "abbbc"));
        assert!(!m("ab+c", "ac"));
        assert!(m("ab*c", "ac"));
        assert!(m("(foo|bar)baz", "xbarbaz"));
        assert!(m("timeout|refused", "connection refused"));
        assert!(m("^$", ""));
        assert!(m("", "anything"));
    }

    #[test]
    fn classes_and_escapes() {
        assert!(m(r"status=5\d\d", "GET / status=503"));
        assert!(!m(r"status=5\d\d", "status=404"));
        assert!(m("[A-Z]{3}-[0-9]+", "id ABC-42"));
        assert!(m("[^a-z ]", "abc 1"));
        assert!(!m("[^a-z ]", "abc def"));
        assert!(m(r"\bfail\b", "it did fail."));
        assert!(!m(r"\bfail\b", "failure"));
        assert!(m(r"a\.b", "a.b"));
        assert!(!m(r"a\.b", "axb"));
        assert!(m(r"[\d.]+ms", "took 12.5ms"));
        assert!(m(r"\x41", "A"));
        assert!(m("a{2,3}", "caab"));
        assert!(!m("^a{2,3}$", "aaaa"));
        assert!(m("^a{2,}$", "aaaa"));
        assert!(m("x{", "x{"), "a lone brace is a literal");
        assert!(m("(?i)[a-c]x", "BX"));
        assert!(m(".*?end", "the end"));
    }

    #[test]
    fn errors_are_messages() {
        for p in [
            "(", "a)", "[abc", "*a", r"\1", "(?=x)", r"\p{L}", "a{5,2}", r"\q", "^*",
        ] {
            assert!(Regex::new(p).is_err(), "{p}");
        }
        assert!(Regex::new("a{1001}").is_err());
    }

    #[test]
    fn no_catastrophic_backtracking() {
        let r = Regex::new("(a+)+$").unwrap();
        let text = "a".repeat(5000) + "b";
        let t = std::time::Instant::now();
        assert!(!r.is_match(&text));
        assert!(t.elapsed().as_millis() < 500);
    }
}
