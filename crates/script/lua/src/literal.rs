//! A module that is only `return` and a constant table, built straight into
//! tables. A program's tree is megabytes of such a literal, and Luau's
//! compiler takes about 16× its size on the system heap to compile it (AO
//! Strings Pizzicato 2: 3.8 MB of source, +61 MB). Anything else is `None`,
//! and the caller compiles the module as usual.

use mlua::{Lua, Value};

/// `text` as the value it returns, when it is `return` and a constant: tables
/// of `[k]=v`, `name=v` and positional entries, strings, numbers, booleans
/// and nil.
pub(crate) fn read(lua: &Lua, text: &str) -> Option<Value> {
    let mut r = Reader {
        lua,
        bytes: text.as_bytes(),
        at: 0,
    };
    r.space();
    r.word("return")?;
    let value = r.value()?;
    r.space();
    (r.at == r.bytes.len()).then_some(value)
}

struct Reader<'a> {
    lua: &'a Lua,
    bytes: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn space(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.space();
        self.bytes.get(self.at).copied()
    }

    fn eat(&mut self, b: u8) -> bool {
        let hit = self.peek() == Some(b);
        self.at += usize::from(hit);
        hit
    }

    fn name(&mut self) -> Option<&str> {
        self.space();
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            self.at += 1;
        }
        let name = std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
        name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            .then_some(name)
    }

    fn word(&mut self, word: &str) -> Option<()> {
        (self.name()? == word).then_some(())
    }

    fn value(&mut self) -> Option<Value> {
        match self.peek()? {
            b'{' => self.table(),
            b'"' => self.string(),
            b'-' | b'.' | b'0'..=b'9' => self.number(),
            _ => match self.name()? {
                "true" => Some(Value::Boolean(true)),
                "false" => Some(Value::Boolean(false)),
                "nil" => Some(Value::Nil),
                _ => None,
            },
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.at;
        while self
            .bytes
            .get(self.at)
            .is_some_and(|b| b.is_ascii_digit() || b"+-.eE".contains(b))
        {
            self.at += 1;
        }
        let text = std::str::from_utf8(self.bytes.get(start..self.at)?).ok()?;
        text.parse().ok().map(Value::Number)
    }

    fn string(&mut self) -> Option<Value> {
        self.at += 1;
        let mut out = Vec::new();
        loop {
            match *self.bytes.get(self.at)? {
                b'"' => break,
                b'\\' => {
                    self.at += 1;
                    out.push(match *self.bytes.get(self.at)? {
                        b'n' => b'\n',
                        b @ (b'"' | b'\\') => b,
                        _ => return None,
                    });
                }
                b'\n' => return None,
                b => out.push(b),
            }
            self.at += 1;
        }
        self.at += 1;
        self.lua.create_string(out).ok().map(Value::String)
    }

    fn table(&mut self) -> Option<Value> {
        self.at += 1;
        let table = self.lua.create_table().ok()?;
        let mut next = 1;
        while !self.eat(b'}') {
            if self.eat(b'[') {
                let key = self.value()?;
                (self.eat(b']') && self.eat(b'=')).then_some(())?;
                table.raw_set(key, self.value()?).ok()?;
            } else {
                let start = self.at;
                let name = self.name().map(str::to_owned);
                let keyed = name.filter(|_| self.eat(b'='));
                match keyed {
                    Some(key) => table.raw_set(key, self.value()?).ok()?,
                    None => {
                        self.at = start;
                        table.raw_set(next, self.value()?).ok()?;
                        next += 1;
                    }
                }
            }
            if !self.eat(b',') && !self.eat(b';') {
                self.eat(b'}').then_some(())?;
                break;
            }
        }
        Some(Value::Table(table))
    }
}

#[cfg(test)]
mod tests {
    use mlua::{Lua, Value};

    #[test]
    fn a_constant_module_reads_as_luau_returns_it_and_anything_else_compiles() {
        let lua = Lua::new();
        let text = r#"return {params={["Name"]="A \"b\"\\c",["Gain"]="0.5"},group=3,
            links={2,1},layers={{params={}},},connections={{"@MIDI CC 1","Gain",-0.25,7,1e-3,false},},}"#;
        let read = super::read(&lua, text).expect("a constant");
        let compiled: Value = lua.load(text).call(()).unwrap();
        let show = "local function show(v)
            if type(v) ~= 'table' then return tostring(v) end
            local keys = {}
            for k in pairs(v) do table.insert(keys, k) end
            table.sort(keys, function(a, b) return tostring(a) < tostring(b) end)
            local out = {}
            for _, k in ipairs(keys) do table.insert(out, tostring(k) .. '=' .. show(v[k])) end
            return '{' .. table.concat(out, ',') .. '}'
        end
        return show";
        let show: mlua::Function = lua.load(show).call(()).unwrap();
        assert_eq!(
            show.call::<String>(read).unwrap(),
            show.call::<String>(compiled).unwrap()
        );
        for code in [
            "return {a=b}",
            "return {f=function() end}",
            "x = 1",
            "return {1} 2",
        ] {
            assert!(super::read(&lua, code).is_none(), "{code}");
        }
    }
}
