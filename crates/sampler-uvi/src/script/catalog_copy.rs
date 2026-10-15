//! One private VM entry copies fixed scalar declarations and current XML fields.
use mlua::{Function, Lua, Table};

pub(super) fn cloner(lua: &Lua) -> mlua::Result<Function> {
    let table = lua.globals().raw_get::<Table>("table")?;
    lua.load(
        r#"
        local clone, create = ...
        return function(schema, retained)
            local count = #schema
            local definitions = create(count + (retained and #retained or 0))
            for index = 1, #schema do
                local definition = clone(schema[index])
                definitions[index] = definition
                definitions[definition.name] = definition
            end
            if retained then
                for index = 1, #retained do
                    local template = retained[index]
                    if definitions[template.name] == nil then
                        local definition = clone(template)
                        count = count + 1
                        definition.id = count
                        definitions[count] = definition
                        definitions[definition.name] = definition
                    end
                end
            end
            return definitions
        end
    "#,
    )
    .set_name("@kontra/catalog-definitions")
    .call((
        table.raw_get::<Function>("clone")?,
        table.raw_get::<Function>("create")?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mlua::{Value, VmState};
    use std::{cell::Cell, rc::Rc};

    fn template(lua: &Lua, name: &str) -> Table {
        let t = lua.create_table().unwrap();
        t.raw_set("name", name).unwrap();
        t.raw_set("type", "string").unwrap();
        t.raw_set("provenance", "retained-xml").unwrap();
        t
    }

    #[test]
    fn copies_keep_numeric_named_aliases_and_private_templates_across_mutation() {
        let lua = Lua::new();
        let copy = cloner(&lua).unwrap();
        let schema = lua.create_table().unwrap();
        let retained = lua.create_table().unwrap();
        let gain = template(&lua, "Gain");
        gain.raw_set("id", 1).unwrap();
        gain.raw_set("default", 0.5).unwrap();
        schema.raw_push(gain.clone()).unwrap();
        let a = template(&lua, "Custom");
        let b = template(&lua, "Other");
        retained.raw_push(a.clone()).unwrap();
        retained.raw_push(template(&lua, "Gain")).unwrap();
        retained.raw_push(b).unwrap();
        lua.load("table.clone=function() error('authored clone hook') end; table.create=function() error('authored create hook') end").exec().unwrap();
        let first: Table = copy.call((schema.clone(), retained.clone())).unwrap();
        let second: Table = copy.call((schema, retained)).unwrap();
        assert_eq!(first.raw_len(), 3);
        assert_eq!(second.raw_len(), 3);
        for index in 1..=3 {
            let d = first.raw_get::<Table>(index).unwrap();
            let name = d.raw_get::<String>("name").unwrap();
            assert_eq!(
                d.to_pointer(),
                first.raw_get::<Table>(name.as_str()).unwrap().to_pointer()
            );
            assert_ne!(
                d.to_pointer(),
                second.raw_get::<Table>(name.as_str()).unwrap().to_pointer()
            );
            assert_eq!(d.raw_get::<usize>("id").unwrap(), index);
        }
        first
            .raw_get::<Table>("Custom")
            .unwrap()
            .raw_set("default", 99)
            .unwrap();
        assert!(
            second
                .raw_get::<Table>("Custom")
                .unwrap()
                .raw_get::<Value>("default")
                .unwrap()
                .is_nil()
        );
        assert!(a.raw_get::<Value>("id").unwrap().is_nil());
        assert_eq!(gain.raw_get::<f64>("default").unwrap(), 0.5);
        assert_eq!(
            second
                .raw_get::<Table>("Gain")
                .unwrap()
                .raw_get::<f64>("default")
                .unwrap(),
            0.5
        );
    }

    #[test]
    fn interrupted_copy_propagates_and_does_not_poison_a_later_fresh_copy() {
        let lua = Lua::new();
        let copy = cloner(&lua).unwrap();
        let schema = lua.create_table().unwrap();
        let retained = lua.create_table().unwrap();
        for index in 0..100 {
            retained
                .raw_push(template(&lua, &format!("Field{index}")))
                .unwrap();
        }
        let calls = Rc::new(Cell::new(0));
        let observed = calls.clone();
        lua.set_interrupt(move |_| {
            observed.set(observed.get() + 1);
            if observed.get() > 10 {
                Err(mlua::Error::runtime("fixture work budget exceeded"))
            } else {
                Ok(VmState::Continue)
            }
        });
        let result = copy.call::<Table>((schema.clone(), retained.clone()));
        assert!(result.is_err());
        assert!(calls.get() > 10);
        lua.remove_interrupt();
        let fresh: Table = copy.call((schema, retained.clone())).unwrap();
        assert_eq!(fresh.raw_len(), 100);
        assert!(
            retained
                .raw_get::<Table>(1)
                .unwrap()
                .raw_get::<Value>("id")
                .unwrap()
                .is_nil()
        );
        assert_eq!(
            fresh
                .raw_get::<Table>("Field99")
                .unwrap()
                .raw_get::<usize>("id")
                .unwrap(),
            100
        );
    }

    #[test]
    fn allocation_failure_leaves_templates_reusable() {
        let lua = Lua::new();
        let copy = cloner(&lua).unwrap();
        let schema = lua.create_table().unwrap();
        let retained = lua.create_table().unwrap();
        for index in 0..100 {
            retained
                .raw_push(template(&lua, &format!("Field{index}")))
                .unwrap();
        }
        lua.set_memory_limit(lua.used_memory() + 1024).unwrap();
        let error = copy
            .call::<Table>((schema.clone(), retained.clone()))
            .unwrap_err();
        assert!(error.to_string().contains("memory"), "{error}");
        lua.set_memory_limit(0).unwrap();
        let fresh: Table = copy.call((schema, retained.clone())).unwrap();
        assert_eq!(fresh.raw_len(), 100);
        assert!(
            retained
                .raw_get::<Table>(1)
                .unwrap()
                .raw_get::<Value>("id")
                .unwrap()
                .is_nil()
        );
        assert_eq!(
            fresh
                .raw_get::<Table>("Field99")
                .unwrap()
                .raw_get::<usize>("id")
                .unwrap(),
            100
        );
    }

    #[test]
    fn current_xml_inventory_and_values_are_read_again_for_each_copy() {
        use super::super::{Config, ScriptHost};
        let host = ScriptHost::new("<Program><Inserts><OnePole Custom='first' Freq='500'/><OnePole Custom='12'/></Inserts></Program>", (), Config::default()).unwrap();
        host.lua
            .load(
                r#"
            local a,b=Program.inserts[1],Program.inserts[2]
            first=a.parameterDefinitions
            second=b.parameterDefinitions
            assert(first.Custom.type=='string' and second.Custom.type=='float')
            assert(first[first.Custom.id]==first.Custom)
            assert(second[second.Custom.id]==second.Custom)
            first.Freq.default=999
            first.Custom.displayName='local mutation'
            assert(a:getParameter('Freq')==500 and b:getParameter('Freq')~=999)
        "#,
            )
            .exec()
            .unwrap();
        let program: Table = host.lua.globals().raw_get("Program").unwrap();
        let inserts: Table = program.raw_get("inserts").unwrap();
        let first: Table = inserts.raw_get(1).unwrap();
        let id: usize = first.raw_get("__id").unwrap();
        // Simulate a new retained XML snapshot at the native catalog boundary.
        // Do not cache an element's current field list or values by module kind.
        host.shared.params.borrow_mut()[id] = vec![
            ("Custom".into(), "17.5".into()),
            ("NewField".into(), "fresh".into()),
            ("Freq".into(), "600".into()),
        ];
        host.lua.load(r#"
            local a,b=Program.inserts[1],Program.inserts[2]
            a.parameterDefinitions=nil
            local fresh=a.parameterDefinitions
            assert(#fresh==6 and fresh.Custom.type=='float')
            assert(fresh.NewField.type=='string' and fresh.NewField.default==nil)
            assert(fresh.Custom.default==nil and fresh.Custom.displayName=='Custom')
            assert(fresh.Custom~=first.Custom and fresh.Freq~=first.Freq)
            assert(fresh[fresh.Custom.id]==fresh.Custom and fresh[fresh.NewField.id]==fresh.NewField)
            assert(a:getParameter('Custom')==17.5 and a:getParameter('Freq')==600)
            assert(b:getParameter('Custom')==12 and second.NewField==nil)
            assert(first.Custom.type=='string' and first.NewField==nil)
        "#).exec().unwrap();
        host.shared.params.borrow_mut()[id] = vec![("NewField".into(), "new value".into())];
        host.lua
            .load(
                r#"
            local a=Program.inserts[1]
            a.parameterDefinitions=nil
            local fresh=a.parameterDefinitions
            assert(#fresh==5 and fresh.Custom==nil and fresh.NewField.type=='string')
            assert(a:getParameter('NewField')=='new value')
            assert(a:getParameter('Freq')==fresh.Freq.default and fresh.Freq.default~=999)
        "#,
            )
            .exec()
            .unwrap();
        assert!(
            host.fault_counts().init.is_empty(),
            "{:?}",
            host.fault_counts()
        );
    }

    #[test]
    fn native_copy_obeys_live_host_work_and_recovers_without_a_partial_catalog() {
        use super::super::{Config, ScriptHost};
        let fields = (0..100)
            .map(|i| format!("Field{i}='{i}'"))
            .collect::<Vec<_>>()
            .join(" ");
        let host = ScriptHost::new(
            &format!("<Program><Inserts><OnePole {fields}/></Inserts></Program>"),
            (),
            Config::default(),
        )
        .unwrap();
        host.shared.arm(std::time::Duration::from_secs(20), 40);
        let error = host
            .lua
            .load("local definitions=Program.inserts[1].parameterDefinitions")
            .exec()
            .unwrap_err();
        assert!(host.shared.work_exhausted.get(), "{error}");
        assert_eq!(host.shared.remaining_work.get(), 0);
        host.shared.arm(std::time::Duration::from_secs(20), 10_000);
        host.lua
            .load(
                r#"
            local e=Program.inserts[1]
            assert(rawget(e,'parameterDefinitions')==nil)
            local definitions=e.parameterDefinitions
            assert(#definitions==104 and definitions.Field99.id==104)
            assert(definitions[definitions.Field99.id]==definitions.Field99)
            assert(definitions.Field99.provenance=='retained-xml')
        "#,
            )
            .exec()
            .unwrap();
        assert!(!host.shared.work_exhausted.get());
        assert!(host.shared.remaining_work.get() < 10_000);
    }
}
