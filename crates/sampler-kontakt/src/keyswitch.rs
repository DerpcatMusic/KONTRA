//! Articulation maps for keyswitched Kontakt instruments, from group start
//! criteria and from recognized KSP keyswitch patterns. What is found is
//! given default alternatives (CC 32 values, channels, programs, velocity
//! splits in key order); what hints at keyswitching but is not recognized is
//! reported, never guessed.
//!
//! Recognized in `on note` (with keys evaluated from `on init` defaults):
//! - `if (in_range($EVENT_NOTE, LOW, HIGH) ...)` whose branch assigns
//!   `$var := $EVENT_NOTE [- OFFSET]`: keys LOW..=HIGH, articulation `key - OFFSET`.
//! - `if ($EVENT_NOTE = KEY)` or `select ($EVENT_NOTE)` / `case KEY [to KEY]`
//!   branches that call `allow_group`/`disallow_group` or assign a constant.
//!
//! Only the `on init` assignments at the top level of the callback are
//! evaluated (the authored defaults); keys that depend on anything else are
//! reported. `set_key_type(.., $NI_KEY_TYPE_CONTROL)`, `set_keyrange` and
//! `set_key_name` are hints: they name keys, and alone they are reported.

use ni_file::kontakt::objects::StartCriteriaParams;
use sampler_ir as ir;
use std::collections::HashMap;

/// `$START_CRITERIA_ON_KEY`: the KSP constants enumerate the modes in stored
/// order (none, on key, on controller, cycle round robin, cycle random, slice);
/// installed round-robin groups store 3.
const START_ON_KEY: i32 = 1;
/// Default alternative controller: bank select LSB, otherwise rarely used.
pub const CONTROLLER: u8 = 32;

/// Keyswitches found in one script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    /// Switch keys in key order, each with a name when the script gives one.
    pub keys: Vec<(u8, Option<String>)>,
    /// The key selected before any switch is played, when evaluable.
    pub default: Option<u8>,
}

/// What a script says about keyswitching.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Detection {
    /// No keyswitch pattern and no hint.
    None,
    Found(Detected),
    /// Hints or patterns whose keys or logic could not be established.
    Unrecognized(String),
}

/// Add the instrument's articulation map. `groups` holds each translated
/// group's location and start criteria.
pub(crate) fn translate(
    instrument: &mut ir::Instrument,
    groups: &[(String, ir::GroupRef, Vec<StartCriteriaParams>)],
) {
    let mut ranges: Vec<(u8, u8)> = Vec::new();
    let mut tagged: HashMap<usize, usize> = HashMap::new();
    for (at, group, items) in groups {
        match items.as_slice() {
            [] => {}
            [c] if c.mode == START_ON_KEY
                && (0..=c.key_max).contains(&c.key_min)
                && c.key_max <= 127 =>
            {
                let range = (c.key_min as u8, c.key_max as u8);
                let index = ranges.iter().position(|&r| r == range).unwrap_or_else(|| {
                    ranges.push(range);
                    ranges.len() - 1
                });
                tagged.insert(group.0, index);
            }
            _ => instrument.unsupported.push(ir::Unsupported {
                location: at.clone(),
                feature: "group start options (mode, next, cycle class)".into(),
                value: format!(
                    "{:?}",
                    items
                        .iter()
                        .map(|c| (c.mode, c.next_criteria, c.cycle_class))
                        .collect::<Vec<_>>()
                ),
                reason: ir::Reason::NotModeled,
            }),
        }
    }
    let overlapping = ranges
        .iter()
        .enumerate()
        .any(|(i, a)| ranges[..i].iter().any(|b| a.0 <= b.1 && b.0 <= a.1));
    if overlapping {
        instrument.unsupported.push(ir::Unsupported {
            location: "groups".into(),
            feature: "overlapping start-on-key ranges".into(),
            value: format!("{ranges:?}"),
            reason: ir::Reason::NotModeled,
        });
    } else if !ranges.is_empty() {
        // Ordered by key so the first range is the default (the lowest switch).
        let mut order: Vec<usize> = (0..ranges.len()).collect();
        order.sort_by_key(|&i| ranges[i]);
        let mut rank = vec![0; ranges.len()];
        for (r, &i) in order.iter().enumerate() {
            rank[i] = r;
        }
        instrument.articulations = order
            .iter()
            .enumerate()
            .map(|(r, &i)| ir::Articulation {
                name: key_name(ranges[i].0),
                switch_keys: (ranges[i].0..=ranges[i].1).collect(),
                default: r == 0,
                ..Default::default()
            })
            .collect();
        for zone in &mut instrument.zones {
            if let Some(&i) = zone.group.and_then(|g| tagged.get(&g.0)) {
                zone.articulation = Some(ir::ArticulationRef(rank[i]));
            }
        }
        instrument.switching.owner = ir::SwitchOwner::Native;
    }
    let mut owner: Option<String> = None;
    for behavior in &instrument.behaviors {
        if behavior.language != ir::Language::Ksp {
            continue;
        }
        let unsupported = |value: String| ir::Unsupported {
            location: behavior.name.clone(),
            feature: "keyswitch script".into(),
            value,
            reason: ir::Reason::NotModeled,
        };
        match detect(&behavior.source) {
            Detection::None => {}
            Detection::Unrecognized(why) => instrument.unsupported.push(unsupported(why)),
            Detection::Found(found) if !instrument.articulations.is_empty() => {
                let why = match &owner {
                    Some(first) => format!("{first} already owns switching"),
                    None => "group start options already switch".into(),
                };
                instrument
                    .unsupported
                    .push(unsupported(format!("keys {:?}: {why}", keys(&found))));
            }
            Detection::Found(found) => {
                owner = Some(behavior.name.clone());
                instrument.articulations = found
                    .keys
                    .iter()
                    .map(|(key, name)| ir::Articulation {
                        name: name.clone().unwrap_or_else(|| key_name(*key)),
                        switch_keys: vec![*key],
                        default: found.default == Some(*key),
                        ..Default::default()
                    })
                    .collect();
                instrument.switching.owner = ir::SwitchOwner::Behavior;
            }
        }
    }
    instrument.assign_alternatives(CONTROLLER);
}

fn keys(found: &Detected) -> Vec<u8> {
    found.keys.iter().map(|k| k.0).collect()
}

/// Kontakt's note name: key 60 is C3.
fn key_name(key: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(key % 12)],
        i32::from(key / 12) - 2
    )
}

// ---------------------------------------------------------------- detection

/// Find the keyswitch keys of a KSP script.
pub fn detect(source: &str) -> Detection {
    let lines = statements(source);
    let init = callback(&lines, "init");
    let env = Env::from_init(init.unwrap_or(&[]));
    let note = callback(&lines, "note").unwrap_or(&[]);
    let mut keys: Vec<(u8, Option<String>)> = Vec::new();
    let mut default = None;
    let mut problems = Vec::new();
    let mut add = |key: i64, problems: &mut Vec<String>| match u8::try_from(key) {
        Ok(key) if key < 128 => {
            if !keys.iter().any(|k| k.0 == key) {
                keys.push((key, None));
            }
        }
        _ => problems.push(format!("key {key} out of range")),
    };
    let mut offset_of_range: Option<i64> = None; // articulation index = key - offset
    for (i, line) in note.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        let body = || branch(&note[i + 1..]);
        if let Some(rest) = lower
            .strip_prefix("if")
            .filter(|r| r.contains("in_range($event_note"))
        {
            let at = rest.find("in_range(").unwrap() + "in_range(".len();
            let args = arguments(&line[2 + at..]);
            let Some(var_offset) = body().iter().find_map(|l| note_assignment(l)) else {
                continue; // A playable-range check, not a switch.
            };
            match (
                args.get(1).and_then(|a| env.eval(a)),
                args.get(2).and_then(|a| env.eval(a)),
            ) {
                (Some(low), Some(high)) if low <= high && high - low < 128 => {
                    let (var, offset) = var_offset;
                    let Some(offset) = offset.map_or(Some(0), |o| env.eval(&o)) else {
                        problems.push(format!(
                            "switch offset in {line:?} depends on runtime state"
                        ));
                        continue;
                    };
                    for key in low..=high {
                        add(key, &mut problems);
                    }
                    if let Some(value) = env.scalars.get(&var) {
                        default = u8::try_from(value + offset).ok();
                    }
                    offset_of_range = Some(offset);
                }
                _ => problems.push(format!("switch range in {line:?} depends on runtime state")),
            }
        } else if let Some(cond) = lower
            .strip_prefix("if")
            .map(str::trim)
            .and_then(|c| c.strip_prefix('('))
            .and_then(|c| c.strip_suffix(')'))
            .and_then(|c| c.trim().strip_prefix("$event_note"))
            .and_then(|c| c.trim().strip_prefix('='))
        {
            if switches(body()) {
                let cond = &line[line.len() - 1 - cond.len()..line.len() - 1];
                match env.eval(cond) {
                    Some(key) => add(key, &mut problems),
                    None => {
                        problems.push(format!("switch key in {line:?} depends on runtime state"))
                    }
                }
            }
        } else if lower.starts_with("select") && lower.contains("$event_note") {
            let mut depth = 0;
            for (j, l) in note[i + 1..].iter().enumerate() {
                let l_lower = l.to_ascii_lowercase();
                depth += opens(&l_lower);
                if l_lower.starts_with("end select") && depth == 0 {
                    break;
                }
                depth -= closes(&l_lower);
                let Some(case) = l_lower.strip_prefix("case ").filter(|_| depth == 0) else {
                    continue;
                };
                let case = &l[l.len() - case.len()..];
                let rest = &note[i + 2 + j..];
                let end = rest
                    .iter()
                    .position(|r| {
                        let r = r.to_ascii_lowercase();
                        r.starts_with("case ") || r.starts_with("end select")
                    })
                    .unwrap_or(rest.len());
                if !switches(&rest[..end]) {
                    continue;
                }
                let (low, high) = match case.split_once(" to ") {
                    Some((a, b)) => (env.eval(a), env.eval(b)),
                    None => (env.eval(case), env.eval(case)),
                };
                match (low, high) {
                    (Some(low), Some(high)) if low <= high && high - low < 128 => {
                        for key in low..=high {
                            add(key, &mut problems);
                        }
                    }
                    _ => problems.push(format!("switch case {case:?} depends on runtime state")),
                }
            }
        }
    }
    let hinted = hints(&lines, &env);
    if !problems.is_empty() {
        return Detection::Unrecognized(problems.join("; "));
    }
    if keys.len() < 2 {
        return match (keys.first(), hinted) {
            (Some((key, _)), _) => {
                Detection::Unrecognized(format!("single switch key {key}: a toggle, not a set"))
            }
            (None, Some(hint)) => Detection::Unrecognized(format!(
                "keyswitch hints ({hint}) but no recognized switch in on note"
            )),
            (None, None) => Detection::None,
        };
    }
    keys.sort_by_key(|k| k.0);
    // Names: literal set_key_name(KEY, "..."), or an init string array that a
    // set_key_name call indexes, read by articulation index for range patterns.
    for (key, name) in &mut keys {
        *name = env.names.get(key).cloned().or_else(|| {
            let index = usize::try_from(i64::from(*key) - offset_of_range?).ok()?;
            env.name_array.as_ref()?.get(index)?.clone()
        });
    }
    if default.is_some_and(|d| !keys.iter().any(|k| k.0 == d)) {
        default = None;
    }
    Detection::Found(Detected { keys, default })
}

/// Comment-free statements, one per line, continuations joined.
fn statements(source: &str) -> Vec<String> {
    let mut text = String::with_capacity(source.len());
    let mut comment = 0usize;
    let mut quoted = false;
    for c in source.chars() {
        match c {
            '"' if comment == 0 => {
                quoted = !quoted;
                text.push(c);
            }
            '{' if !quoted => comment += 1,
            '}' if !quoted && comment > 0 => comment -= 1,
            '\r' => {}
            '\n' => {
                quoted = false;
                text.push('\n');
            }
            _ if comment == 0 => text.push(c),
            _ => {}
        }
    }
    let text = text.replace("...\n", " ");
    text.lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

fn callback<'a>(lines: &'a [String], name: &str) -> Option<&'a [String]> {
    let start = lines
        .iter()
        .position(|l| l.eq_ignore_ascii_case(&format!("on {name}")))?;
    let end = lines[start..]
        .iter()
        .position(|l| l.eq_ignore_ascii_case("end on"))?;
    Some(&lines[start + 1..start + end])
}

fn opens(lower: &str) -> i32 {
    i32::from(
        ["if ", "if(", "while ", "while(", "select ", "select("]
            .iter()
            .any(|k| lower.starts_with(k)),
    )
}

fn closes(lower: &str) -> i32 {
    i32::from(
        ["end if", "end while", "end select"]
            .iter()
            .any(|k| lower.starts_with(k)),
    )
}

/// The then-branch of the block opened just before `rest`.
fn branch(rest: &[String]) -> &[String] {
    let mut depth = 0;
    for (i, line) in rest.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if depth == 0 && (lower.starts_with("else") || closes(&lower) == 1) {
            return &rest[..i];
        }
        depth += opens(&lower) - closes(&lower);
    }
    rest
}

/// Whether a branch selects groups or sets a selection variable to a constant.
fn switches(body: &[String]) -> bool {
    body.iter().any(|l| {
        let lower = l.to_ascii_lowercase();
        lower.starts_with("allow_group")
            || lower.starts_with("disallow_group")
            || lower.split_once(":=").is_some_and(|(left, right)| {
                left.trim().starts_with('$') && right.trim().parse::<i64>().is_ok()
            })
    })
}

/// `$var := $EVENT_NOTE [- OFFSET]`: the variable and the offset expression.
fn note_assignment(line: &str) -> Option<(String, Option<String>)> {
    let (left, right) = line.split_once(":=")?;
    let var = left.trim();
    if !var.starts_with('$') {
        return None;
    }
    let right = right.trim();
    let rest = right
        .get(..11)
        .filter(|r| r.eq_ignore_ascii_case("$event_note"))
        .map(|_| right[11..].trim())?;
    if rest.is_empty() {
        return Some((var.into(), None));
    }
    let offset = rest.strip_prefix('-')?.trim();
    Some((var.into(), Some(format!("({offset})"))))
}

/// Top-level comma-separated arguments up to the closing parenthesis.
fn arguments(text: &str) -> Vec<&str> {
    let mut args = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' if depth == 0 => {
                args.push(text[start..i].trim());
                return args;
            }
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                args.push(text[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
    }
    args
}

/// Declared control keys, as a readable hint, or `None` without hints.
fn hints(lines: &[String], env: &Env) -> Option<String> {
    let mut found = Vec::new();
    for line in lines {
        let lower = line.to_ascii_lowercase();
        if lower.contains("$ni_key_type_control") || lower.starts_with("set_keyrange") {
            found.push(line.as_str());
        }
    }
    if found.is_empty() {
        return None;
    }
    let evaluated: Vec<i64> = found
        .iter()
        .filter_map(|l| {
            let open = l.find('(')?;
            env.eval(arguments(&l[open + 1..]).first()?)
        })
        .collect();
    Some(if evaluated.is_empty() {
        format!("{} control-key declarations with runtime keys", found.len())
    } else {
        format!("control keys {evaluated:?}")
    })
}

// ---------------------------------------------------------------- evaluation

/// Values `on init` assigns at its top level; anything assigned in a nested
/// block is unknown.
#[derive(Default)]
struct Env {
    scalars: HashMap<String, i64>,
    arrays: HashMap<String, Vec<i64>>,
    strings: HashMap<String, Vec<Option<String>>>,
    /// Literal `set_key_name(KEY, "...")` names.
    names: HashMap<u8, String>,
    /// The string array a `set_key_name(.., !array[..])` call reads.
    name_array: Option<Vec<Option<String>>>,
}

impl Env {
    fn from_init(init: &[String]) -> Self {
        let mut env = Self::default();
        let mut depth = 0;
        for line in init {
            let lower = line.to_ascii_lowercase();
            if closes(&lower) == 1 {
                depth -= 1;
                continue;
            }
            let nested = depth > 0;
            depth += opens(&lower);
            let declaration = lower.starts_with("declare ");
            let statement = if declaration {
                // Drop the keyword and modifiers before the variable.
                line.split_whitespace()
                    .skip_while(|w| !w.starts_with(['$', '%', '!', '@', '~', '?']))
                    .collect::<Vec<_>>()
                    .join(" ")
            } else {
                line.clone()
            };
            if lower.starts_with("set_key_name") {
                let args = arguments(&line[line.find('(').map_or(0, |i| i + 1)..]);
                if let [key, name] = args[..] {
                    if let (Some(key), Some(name)) = (env.eval(key), literal(name)) {
                        if let Ok(key) = u8::try_from(key) {
                            env.names.insert(key, name);
                        }
                    }
                }
                continue;
            }
            let (target, value) = match statement.split_once(":=") {
                Some((t, v)) => (t.trim(), Some(v.trim())),
                None => (statement.trim(), None),
            };
            let name: String = target
                .chars()
                .take_while(|c| !matches!(c, '[' | ' ' | '('))
                .collect();
            if name.len() < 2 {
                continue;
            }
            if nested {
                env.forget(&name);
                continue;
            }
            match (name.as_bytes()[0], value) {
                (b'$', Some(v)) => match env.eval(v) {
                    Some(v) => {
                        env.scalars.insert(name, v);
                    }
                    None => env.forget(&name),
                },
                // A declared scalar starts at zero (ui_slider included).
                (b'$', None) if declaration => {
                    env.scalars.insert(name, 0);
                }
                (b'%', Some(v)) if declaration => {
                    let list = v.trim().strip_prefix('(').and_then(|v| v.strip_suffix(')'));
                    match list.map(|l| l.split(',').map(|x| env.eval(x)).collect()) {
                        Some(Some(values)) => {
                            env.arrays.insert(name, values);
                        }
                        _ => env.forget(&name),
                    }
                }
                (b'%', Some(v)) => {
                    let index = target[name.len()..]
                        .strip_prefix('[')
                        .and_then(|t| t.strip_suffix(']'))
                        .and_then(|t| env.eval(t));
                    match (index, env.eval(v)) {
                        (Some(i), Some(v)) => match env.arrays.get_mut(&name) {
                            Some(a) if (0..a.len() as i64).contains(&i) => a[i as usize] = v,
                            _ => env.forget(&name),
                        },
                        _ => env.forget(&name),
                    }
                }
                (b'!', None) if declaration => {
                    let size = target[name.len()..]
                        .strip_prefix('[')
                        .and_then(|t| t.strip_suffix(']'))
                        .and_then(|t| env.eval(t))
                        .filter(|&n| (0..=4096).contains(&n));
                    if let Some(n) = size {
                        env.strings.insert(name, vec![None; n as usize]);
                    }
                }
                (b'!', Some(v)) => {
                    let index = target[name.len()..]
                        .strip_prefix('[')
                        .and_then(|t| t.strip_suffix(']'))
                        .and_then(|t| env.eval(t));
                    if let (Some(i), Some(text), Some(a)) =
                        (index, literal(v), env.strings.get_mut(&name))
                    {
                        if let Some(slot) = usize::try_from(i).ok().and_then(|i| a.get_mut(i)) {
                            *slot = Some(text);
                        }
                    }
                }
                _ => {}
            }
        }
        // Any `set_key_name(.., !array[..])` names keys by array index.
        env.name_array = init
            .iter()
            .filter(|l| l.to_ascii_lowercase().starts_with("set_key_name"))
            .find_map(|l| {
                let at = l.find(", !").or_else(|| l.find(",!"))?;
                let rest = l[at + 1..].trim_start();
                let name: String = rest.chars().take_while(|&c| c != '[').collect();
                env.strings.get(&name).cloned()
            });
        env
    }

    fn forget(&mut self, name: &str) {
        self.scalars.remove(name);
        self.arrays.remove(name);
    }

    fn eval(&self, text: &str) -> Option<i64> {
        let tokens = tokenize(text)?;
        let mut parser = Parser {
            tokens: &tokens,
            at: 0,
            env: self,
        };
        let value = parser.sum()?;
        (parser.at == tokens.len()).then_some(value)
    }
}

fn literal(text: &str) -> Option<String> {
    let text = text.trim();
    Some(text.strip_prefix('"')?.strip_suffix('"')?.to_string())
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(i64),
    Name(String),
    Op(char),
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else if c.is_ascii_digit() {
            let mut n = 0i64;
            while let Some(d) = chars.peek().and_then(|c| c.to_digit(10)) {
                n = n.checked_mul(10)?.checked_add(i64::from(d))?;
                chars.next();
            }
            tokens.push(Token::Number(n));
        } else if matches!(c, '$' | '%' | '_') || c.is_ascii_alphabetic() {
            let mut name = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphanumeric() || matches!(c, '$' | '%' | '_' | '.') {
                    name.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            tokens.push(Token::Name(name));
        } else if matches!(c, '+' | '-' | '*' | '/' | '(' | ')' | '[' | ']') {
            tokens.push(Token::Op(c));
            chars.next();
        } else {
            return None;
        }
    }
    Some(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    env: &'a Env,
}

impl Parser<'_> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn eat(&mut self, op: char) -> bool {
        let found = self.peek() == Some(&Token::Op(op));
        self.at += usize::from(found);
        found
    }

    fn sum(&mut self) -> Option<i64> {
        let mut value = self.product()?;
        loop {
            if self.eat('+') {
                value = value.checked_add(self.product()?)?;
            } else if self.eat('-') {
                value = value.checked_sub(self.product()?)?;
            } else {
                return Some(value);
            }
        }
    }

    fn product(&mut self) -> Option<i64> {
        let mut value = self.unary()?;
        loop {
            if self.eat('*') {
                value = value.checked_mul(self.unary()?)?;
            } else if self.eat('/') {
                value = value.checked_div(self.unary()?)?;
            } else if matches!(self.peek(), Some(Token::Name(n)) if n.eq_ignore_ascii_case("mod")) {
                self.at += 1;
                value = value.checked_rem(self.unary()?)?;
            } else {
                return Some(value);
            }
        }
    }

    fn unary(&mut self) -> Option<i64> {
        if self.eat('-') {
            return self.unary()?.checked_neg();
        }
        if self.eat('(') {
            let value = self.sum()?;
            return self.eat(')').then_some(value);
        }
        match self.peek()?.clone() {
            Token::Number(n) => {
                self.at += 1;
                Some(n)
            }
            Token::Name(name) if name.starts_with('$') => {
                self.at += 1;
                self.env.scalars.get(&name).copied()
            }
            Token::Name(name) if name.starts_with('%') => {
                self.at += 1;
                if !self.eat('[') {
                    return None;
                }
                let index = self.sum()?;
                if !self.eat(']') {
                    return None;
                }
                let array = self.env.arrays.get(&name)?;
                array.get(usize::try_from(index).ok()?).copied()
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(source: &str) -> Detected {
        match detect(source) {
            Detection::Found(found) => found,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn range_with_offset_variable_and_init_arrays() {
        // The shape of Afflatus' keyswitch script, trimmed.
        let source = r#"
on init
    declare %KS_keys[4] := (0, 12, 24, 96)
    declare ui_slider $KS_Base (0,3)
    $KS_Base := 2
    declare const $num := 3
    declare !names[$num]
    !names[0] := "Sustain"
    !names[1] := "Staccato"
    !names[2] := "Trill"
    declare $articulation := 1
    make_persistent($articulation)
    if ($x = 1)
        $num2 := 5
    end if
    set_key_name(%KS_keys[$KS_Base], !names[0])
end on
on note
    ignore_event($EVENT_ID) { swallow }
    if (in_range($EVENT_NOTE,%KS_keys[$KS_Base],%KS_keys[$KS_Base] + $num-1) ...
        and not($EVENT_NOTE = 0))
        $articulation := $EVENT_NOTE - %KS_keys[$KS_Base]
        exit
    end if
    if (not(in_range($EVENT_NOTE,$range_min,$range_max)))
        exit
    end if
end on
"#;
        let found = found(source);
        assert_eq!(
            found.keys,
            [
                (24, Some("Sustain".into())),
                (25, Some("Staccato".into())),
                (26, Some("Trill".into()))
            ]
        );
        assert_eq!(found.default, Some(25));
    }

    #[test]
    fn equality_and_select_patterns_with_group_selection() {
        let source = "on init\n declare $art\nend on\non note\n\
            if ($EVENT_NOTE = 36)\n disallow_group($ALL_GROUPS)\n allow_group(0)\n end if\n\
            if ($EVENT_NOTE = 37)\n $art := 1\n end if\n\
            select ($EVENT_NOTE)\n case 38 to 39\n disallow_group($ALL_GROUPS)\n\
            case 60\n play_note(60, 100, 0, -1)\n end select\nend on\n";
        let found = found(source);
        assert_eq!(
            found.keys.iter().map(|k| k.0).collect::<Vec<_>>(),
            [36, 37, 38, 39]
        );
        assert_eq!(found.default, None);
    }

    #[test]
    fn runtime_keys_and_bare_hints_are_reported_not_guessed() {
        let runtime = "on init\nend on\non note\n\
            if (in_range($EVENT_NOTE, $ks_low, $ks_high))\n $ks := $EVENT_NOTE - $ks_low\n end if\nend on";
        assert!(matches!(detect(runtime), Detection::Unrecognized(why) if why.contains("runtime")));
        let hinted = "on init\n declare $base := 24\n set_key_type($base, $NI_KEY_TYPE_CONTROL)\nend on\n\
            on note\n if ($EVENT_NOTE = $base)\n call toggle\n end if\nend on";
        assert!(
            matches!(detect(hinted), Detection::Unrecognized(why) if why.contains("control keys [24]"))
        );
        let toggle =
            "on init\nend on\non note\n if ($EVENT_NOTE = 24)\n $legato := 1\n end if\nend on";
        assert!(matches!(detect(toggle), Detection::Unrecognized(why) if why.contains("single")));
        let plain = "on init\n declare $x := 1\nend on\non note\n if (in_range($EVENT_NOTE, 0, 10))\n exit\n end if\nend on";
        assert_eq!(detect(plain), Detection::None);
    }

    #[test]
    fn start_on_key_groups_become_native_articulations() {
        let criteria = |mode, low, high| StartCriteriaParams {
            mode,
            next_criteria: 0,
            key_min: low,
            key_max: high,
            controller: 0,
            cc_min: 0,
            cc_max: 0,
            cycle_class: 0,
            slice_zone_idx: 0,
            slice_zone_slice_idx: 0,
            sequencer_only: false,
        };
        let mut ir = ir::Instrument::default();
        ir.groups = vec![ir::Group::default(); 4];
        ir.zones = (0..4)
            .map(|g| ir::Zone {
                group: Some(ir::GroupRef(g)),
                ..ir::Zone::new(ir::AssetRef(0))
            })
            .collect();
        let groups = vec![
            ("g0".into(), ir::GroupRef(0), vec![criteria(1, 25, 25)]),
            ("g1".into(), ir::GroupRef(1), vec![criteria(1, 24, 24)]),
            ("g2".into(), ir::GroupRef(2), vec![]),
            ("g3".into(), ir::GroupRef(3), vec![criteria(3, 24, 24)]),
        ];
        translate(&mut ir, &groups);
        let tags: Vec<_> = ir
            .zones
            .iter()
            .map(|z| z.articulation.map(|a| a.0))
            .collect();
        assert_eq!(tags, [Some(1), Some(0), None, None]);
        assert_eq!(ir.articulations[0].switch_keys, [24]);
        assert!(ir.articulations[0].default);
        assert_eq!(ir.articulations[1].alternatives.program, Some(1));
        assert_eq!(ir.switching.owner, ir::SwitchOwner::Native);
        assert_eq!(ir.unsupported.len(), 1); // the round-robin group
    }
}
