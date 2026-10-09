//! Port v1 4bffbb18:src/uvi/host.rs native modules into the Luau owner.
use mlua::{AnyUserData, Function, MetaMethod, Table, UserData, UserDataMethods, Value};

pub(super) const CHORD_REC: &str = r#"
local kinds={
  [5]="sus2",[9]="m",[11]="maddb9",[13]="madd9",[17]="M",[19]="Maddb9",
  [21]="Madd9",[25]="Madd#9",[33]="sus4",[37]="sus2sus4",[41]="mbb5",[65]="5-",
  [73]="dim",[81]="Mb5",[129]="5",[133]="sus2",[137]="m",[139]="maddb9",
  [141]="madd9",[145]="M",[147]="Maddb9",[149]="Madd9",[153]="Madd#9",[161]="sus4",
  [165]="sus2sus4",[261]="sus2#5",[273]="aug",[525]="m6/9",[533]="6/9",[585]="dim7",
  [649]="m6",[653]="m6/9",[657]="6",[661]="6/9",[1029]="7sus2no5",[1033]="m7",
  [1035]="m7b9",[1037]="m9",[1041]="7",[1043]="7b9",[1045]="9",[1049]="7#9",
  [1057]="7sus4no5",[1061]="7sus2sus4no5",[1069]="m9/11",[1077]="11",[1097]="m7b5",[1101]="m9b5",
  [1105]="7b5",[1109]="9b5",[1157]="7sus2",[1161]="m7",[1163]="m7b9",[1165]="m9",
  [1169]="7",[1171]="7b9",[1173]="9",[1177]="7#9",[1185]="7sus4",[1189]="7sus2sus4",
  [1193]="m7/11",[1197]="m11",[1205]="11",[1225]="m7/#11",[1289]="m7#5",[1297]="7#5",
  [1301]="9#5",[1581]="m13",[1589]="13",[1709]="m13",[1717]="13",[2053]="M7sus2",
  [2057]="mM7",[2061]="mM9",[2065]="M7",[2067]="M7b9",[2069]="M9",[2073]="M7#9",
  [2081]="M7sus4no5",[2085]="M7sus2sus4no5",[2089]="mM7bb5",[2093]="mM11",[2101]="M11",[2121]="mM7b5",
  [2129]="M7b5",[2133]="M#11",[2181]="M7sus2",[2185]="mM7",[2189]="mM9",[2193]="M7",
  [2195]="M7b9",[2197]="M9",[2201]="M7#9",[2209]="M7sus4",[2213]="M7sus2sus4",[2221]="mM11",
  [2229]="M11",[2261]="M#11",[2313]="mM7#5",[2321]="M7#5",[2325]="M9#5",[2605]="mM13",
  [2613]="M13",[2733]="mM13",[2741]="M13",
}
ChordRec={}
function ChordRec.getChroma(root,notes)
  local present={}
  for _,pitch in ipairs(notes)do present[(pitch-root)%12]=true end
  local chroma={}
  for interval=0,11 do chroma[interval+1]=present[interval]and 1 or 0 end
  return chroma
end
function ChordRec.getChromaString(chroma)return table.concat(chroma)end
function ChordRec.chordKind(notes)
  local bass=notes[1]%12
  for _,note in ipairs(notes)do
    local root=note%12
    local chroma=ChordRec.getChroma(root,notes)
    local mask=0
    for interval=0,11 do mask=mask+chroma[interval+1]*2^interval end
    local kind=kinds[mask]
    if kind then return root,kind,bass end
  end
end
"#;

pub(super) struct AsyncUpdaterFactory;
struct AsyncUpdater;

// Native trigger waits in its caller and coalesces while busy, including callback
// reentry. The Lua closure can yield without crossing a Rust method call boundary.
// User values retain callbacks in Lua rather than permanent Rust reference slots.
impl UserData for AsyncUpdaterFactory {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Call,
            |lua, (factory, callback): (AnyUserData, Function)| {
                let environment = factory.user_value::<Table>()?;
                let trigger = lua
                    .load(
                        "local pending=false;return function(self,ms)\
                         if pending then return end;pending=true;wait(ms);\
                         self.callback();pending=false end",
                    )
                    .set_name("UVI AsyncUpdater")
                    .set_environment(environment)
                    .eval::<Function>()?;
                let state = lua.create_table()?;
                state.set("callback", callback)?;
                state.set("trigger", trigger)?;
                let updater = lua.create_userdata(AsyncUpdater)?;
                updater.set_user_value(state)?;
                Ok(updater)
            },
        );
    }
}

impl UserData for AsyncUpdater {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_function(
            MetaMethod::Index,
            |_, (updater, key): (AnyUserData, String)| {
                updater.user_value::<Table>()?.get::<Value>(key)
            },
        );
        methods.add_meta_function(
            MetaMethod::NewIndex,
            |_, (updater, key, callback): (AnyUserData, String, Function)| {
                if key != "callback" {
                    return Err(mlua::Error::runtime("Unknown UVI AsyncUpdater property"));
                }
                updater.user_value::<Table>()?.set(key, callback)
            },
        );
    }
}
