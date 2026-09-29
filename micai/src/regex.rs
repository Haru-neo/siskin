//! A tiny regular expression engine.
//!
//! Uses no external libraries. The same algorithm is mirrored exactly on the C side,
//! so `siskin run` and `siskin build` produce results identical down to the character.
//!
//! Supported: literal chars · `.` · `*` `+` `?` (append `?` for lazy matching) · `[a-z]` `[^...]`
//! · `\d \w \s \D \W \S` and escapes · `^` `$` · `|` · `(...)` · `(?:...)`

/// A character class, such as `[a-z0-9]`.
#[derive(Debug, Clone)]
pub struct Class {
    pub neg: bool,
    pub ranges: Vec<(u32, u32)>,
}

impl Class {
    fn has(&self, c: u32) -> bool {
        let inside = self.ranges.iter().any(|(lo, hi)| c >= *lo && c <= *hi);
        inside != self.neg
    }
}

#[derive(Debug, Clone)]
pub enum Inst {
    Char(u32),
    Any,
    Class(usize),
    Match,
    Jmp(usize),
    /// Two branches. The first one is tried first.
    Split(usize, usize),
    /// Records the start/end position of a group.
    Save(usize),
    Bol,
    Eol,
}

pub struct Prog {
    pub insts: Vec<Inst>,
    pub classes: Vec<Class>,
    pub ngroups: usize,
}

/// State while parsing a single pattern.
struct P<'a> {
    src: &'a [char],
    pos: usize,
    insts: Vec<Inst>,
    classes: Vec<Class>,
    ngroups: usize,
}

pub const MAX_GROUPS: usize = 10;

pub fn compile(pattern: &str) -> Result<Prog, String> {
    let src: Vec<char> = pattern.chars().collect();
    let mut p = P { src: &src, pos: 0, insts: Vec::new(), classes: Vec::new(), ngroups: 0 };
    p.insts.push(Inst::Save(0));
    p.alt()?;
    if p.pos < p.src.len() {
        return Err(tr!(format!("정규식 {}번째 글자 `{}` 를 읽을 수 없습니다", p.pos + 1, p.src[p.pos]), format!("cannot parse `{1}` at position {0} of the regex", p.pos + 1, p.src[p.pos])));
    }
    p.insts.push(Inst::Save(1));
    p.insts.push(Inst::Match);
    Ok(Prog { insts: p.insts, classes: p.classes, ngroups: p.ngroups })
}

impl<'a> P<'a> {
    fn at(&self) -> Option<char> {
        self.src.get(self.pos).copied()
    }

    /// `A|B|C`
    fn alt(&mut self) -> Result<(), String> {
        let start = self.insts.len();
        self.concat()?;
        while self.at() == Some('|') {
            self.pos += 1;
            // Push the left branch behind a Split; on failure, try the right branch.
            let body: Vec<Inst> = self.insts.drain(start..).collect();
            self.insts.push(Inst::Split(0, 0)); // Just reserve the slot for now
            let split_at = self.insts.len() - 1;
            let shift = self.insts.len() - start;
            for i in body {
                self.insts.push(shift_inst(i, shift));
            }
            self.insts.push(Inst::Jmp(0));
            let jmp_at = self.insts.len() - 1;
            let right = self.insts.len();
            self.concat()?;
            let end = self.insts.len();
            self.insts[split_at] = Inst::Split(split_at + 1, right);
            self.insts[jmp_at] = Inst::Jmp(end);
        }
        Ok(())
    }

    fn concat(&mut self) -> Result<(), String> {
        while let Some(c) = self.at() {
            if c == '|' || c == ')' {
                break;
            }
            self.repeat()?;
        }
        Ok(())
    }

    fn repeat(&mut self) -> Result<(), String> {
        let start = self.insts.len();
        self.atom()?;
        loop {
            let c = match self.at() {
                Some(c) if c == '*' || c == '+' || c == '?' => c,
                _ => break,
            };
            self.pos += 1;
            // If immediately followed by `?`, match as little as possible (lazy).
            let lazy = self.at() == Some('?');
            if lazy {
                self.pos += 1;
            }
            let body: Vec<Inst> = self.insts.drain(start..).collect();
            let blen = body.len();
            match c {
                '*' => {
                    // split -> body -> jmp(split)
                    self.insts.push(Inst::Split(0, 0));
                    let split_at = self.insts.len() - 1;
                    let shift = self.insts.len() - start;
                    for i in body {
                        self.insts.push(shift_inst(i, shift));
                    }
                    self.insts.push(Inst::Jmp(split_at));
                    let after = self.insts.len();
                    self.insts[split_at] = if lazy {
                        Inst::Split(after, split_at + 1)
                    } else {
                        Inst::Split(split_at + 1, after)
                    };
                }
                '+' => {
                    // body -> split(body, after)
                    for i in body {
                        self.insts.push(i);
                    }
                    let split_at = self.insts.len();
                    self.insts.push(Inst::Split(0, 0));
                    let after = self.insts.len();
                    self.insts[split_at] = if lazy {
                        Inst::Split(after, start)
                    } else {
                        Inst::Split(start, after)
                    };
                    let _ = blen;
                }
                _ => {
                    // '?' : split -> body
                    self.insts.push(Inst::Split(0, 0));
                    let split_at = self.insts.len() - 1;
                    let shift = self.insts.len() - start;
                    for i in body {
                        self.insts.push(shift_inst(i, shift));
                    }
                    let after = self.insts.len();
                    self.insts[split_at] = if lazy {
                        Inst::Split(after, split_at + 1)
                    } else {
                        Inst::Split(split_at + 1, after)
                    };
                }
            }
        }
        Ok(())
    }

    fn atom(&mut self) -> Result<(), String> {
        let c = match self.at() {
            Some(c) => c,
            None => return Err(tr!("정규식이 갑자기 끝났습니다", "unexpected end of regex").into()),
        };
        self.pos += 1;
        match c {
            '(' => {
                let capture = !(self.at() == Some('?') && self.src.get(self.pos + 1) == Some(&':'));
                if !capture {
                    self.pos += 2;
                }
                let slot = if capture {
                    self.ngroups += 1;
                    if self.ngroups >= MAX_GROUPS {
                        return Err(tr!(format!("괄호는 {}개까지만 쓸 수 있습니다", MAX_GROUPS - 1), format!("at most {} groups are allowed", MAX_GROUPS - 1)));
                    }
                    let g = self.ngroups;
                    self.insts.push(Inst::Save(g * 2));
                    Some(g)
                } else {
                    None
                };
                self.alt()?;
                if self.at() != Some(')') {
                    return Err(tr!("`)` 가 없습니다", "missing `)`").into());
                }
                self.pos += 1;
                if let Some(g) = slot {
                    self.insts.push(Inst::Save(g * 2 + 1));
                }
                Ok(())
            }
            '[' => {
                let cl = self.class()?;
                self.classes.push(cl);
                self.insts.push(Inst::Class(self.classes.len() - 1));
                Ok(())
            }
            '.' => {
                self.insts.push(Inst::Any);
                Ok(())
            }
            '^' => {
                self.insts.push(Inst::Bol);
                Ok(())
            }
            '$' => {
                self.insts.push(Inst::Eol);
                Ok(())
            }
            '\\' => {
                let e = match self.at() {
                    Some(e) => e,
                    None => return Err(tr!("`\\` 뒤에 글자가 없습니다", "missing character after `\\`").into()),
                };
                self.pos += 1;
                match esc_class(e) {
                    Some(cl) => {
                        self.classes.push(cl);
                        self.insts.push(Inst::Class(self.classes.len() - 1));
                    }
                    None => self.insts.push(Inst::Char(esc_char(e) as u32)),
                }
                Ok(())
            }
            ')' => Err(tr!("짝이 없는 `)` 입니다", "unmatched `)`").into()),
            '*' | '+' => Err(tr!(format!("`{}` 앞에 반복할 것이 없습니다", c), format!("nothing to repeat before `{}`", c))),
            _ => {
                self.insts.push(Inst::Char(c as u32));
                Ok(())
            }
        }
    }

    fn class(&mut self) -> Result<Class, String> {
        let mut neg = false;
        if self.at() == Some('^') {
            neg = true;
            self.pos += 1;
        }
        let mut ranges: Vec<(u32, u32)> = Vec::new();
        let mut first = true;
        loop {
            let c = match self.at() {
                Some(c) => c,
                None => return Err(tr!("`]` 가 없습니다", "missing `]`").into()),
            };
            if c == ']' && !first {
                self.pos += 1;
                break;
            }
            first = false;
            self.pos += 1;
            if c == '\\' {
                let e = match self.at() {
                    Some(e) => e,
                    None => return Err(tr!("`\\` 뒤에 글자가 없습니다", "missing character after `\\`").into()),
                };
                self.pos += 1;
                match esc_class(e) {
                    Some(cl) => {
                        // A class inside a class, like `[\d]`, merges its ranges as-is.
                        for r in cl.ranges {
                            ranges.push(r);
                        }
                        continue;
                    }
                    None => {
                        let lo = esc_char(e) as u32;
                        ranges.push((lo, lo));
                        continue;
                    }
                }
            }
            // Is it of the form a-z?
            if self.at() == Some('-') && self.src.get(self.pos + 1).map_or(false, |x| *x != ']') {
                self.pos += 1;
                let hi = self.src[self.pos];
                self.pos += 1;
                ranges.push((c as u32, hi as u32));
            } else {
                ranges.push((c as u32, c as u32));
            }
        }
        Ok(Class { neg, ranges })
    }
}

fn shift_inst(i: Inst, by: usize) -> Inst {
    match i {
        Inst::Jmp(a) => Inst::Jmp(a + by),
        Inst::Split(a, b) => Inst::Split(a + by, b + by),
        other => other,
    }
}

fn esc_char(e: char) -> char {
    match e {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        other => other,
    }
}

fn esc_class(e: char) -> Option<Class> {
    let digits = vec![('0' as u32, '9' as u32)];
    let words = vec![
        ('a' as u32, 'z' as u32),
        ('A' as u32, 'Z' as u32),
        ('0' as u32, '9' as u32),
        ('_' as u32, '_' as u32),
    ];
    let spaces = vec![
        (' ' as u32, ' ' as u32),
        ('\t' as u32, '\t' as u32),
        ('\n' as u32, '\n' as u32),
        ('\r' as u32, '\r' as u32),
    ];
    match e {
        'd' => Some(Class { neg: false, ranges: digits }),
        'D' => Some(Class { neg: true, ranges: digits }),
        'w' => Some(Class { neg: false, ranges: words }),
        'W' => Some(Class { neg: true, ranges: words }),
        's' => Some(Class { neg: false, ranges: spaces }),
        'S' => Some(Class { neg: true, ranges: spaces }),
        _ => None,
    }
}

/// Backtracking matcher. The number of steps per run is capped so that
/// even pathological patterns terminate.
const MAX_STEPS: usize = 2_000_000;

/// Finds a match starting at position `start`. On success, returns the group positions.
pub fn match_at(prog: &Prog, input: &[char], start: usize) -> Option<Vec<isize>> {
    let nslots = MAX_GROUPS * 2;
    let mut saves: Vec<isize> = vec![-1; nslots];
    let mut stack: Vec<(usize, usize, Vec<isize>)> = Vec::new();
    let mut pc = 0usize;
    let mut sp = start;
    let mut steps = 0usize;

    loop {
        steps += 1;
        if steps > MAX_STEPS {
            return None;
        }
        let mut ok = true;
        match prog.insts[pc] {
            Inst::Char(c) => {
                if sp < input.len() && input[sp] as u32 == c {
                    sp += 1;
                    pc += 1;
                } else {
                    ok = false;
                }
            }
            Inst::Any => {
                if sp < input.len() {
                    sp += 1;
                    pc += 1;
                } else {
                    ok = false;
                }
            }
            Inst::Class(ci) => {
                if sp < input.len() && prog.classes[ci].has(input[sp] as u32) {
                    sp += 1;
                    pc += 1;
                } else {
                    ok = false;
                }
            }
            Inst::Bol => {
                if sp == 0 {
                    pc += 1;
                } else {
                    ok = false;
                }
            }
            Inst::Eol => {
                if sp == input.len() {
                    pc += 1;
                } else {
                    ok = false;
                }
            }
            Inst::Save(slot) => {
                // On backtrack, the copy saved by Split is restored,
                // so it is fine to just record it here.
                saves[slot] = sp as isize;
                pc += 1;
            }
            Inst::Jmp(a) => pc = a,
            Inst::Split(a, b) => {
                stack.push((b, sp, saves.clone()));
                pc = a;
            }
            Inst::Match => return Some(saves),
        }
        if !ok {
            match stack.pop() {
                Some((npc, nsp, nsaves)) => {
                    pc = npc;
                    sp = nsp;
                    saves = nsaves;
                }
                None => return None,
            }
        }
    }
}

/// Finds the first match anywhere in the string.
pub fn search(prog: &Prog, input: &[char], from: usize) -> Option<Vec<isize>> {
    let mut at = from;
    loop {
        if let Some(s) = match_at(prog, input, at) {
            return Some(s);
        }
        if at >= input.len() {
            return None;
        }
        at += 1;
    }
}
