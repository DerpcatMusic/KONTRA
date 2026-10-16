//! Bounded owner-only projection. Raw reads, no authored Lua calls, collector or IO.
use super::{ScriptHost, Table, Value, parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
const GRAPH: usize = 262_144;
const STEPS: usize = 1_048_576;
const BYTES: usize = 16 << 20;
const SIZE: usize = 256 << 10;
const FIELDS: &[&str] = &["Bypass","BypassInsertFX","FXPostGain","Gain","Pan","Pitch","BaseNote","NoteTracking","LowKey","HighKey","LowVelocity","HighVelocity","Volume","Frequency","Freq","Mode","KeyTracking","SampleStart","SampleEnd","SamplePath","Source","Destination","Ratio","ConnectionMode","Inverted","Mapper","Dry","Wet","Output","Bus","PrePost","Enabled"];
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LiveGraphQuery { pub keygroup_node:u32, pub matrix_node:u32, pub player_nodes:Vec<u32> }
impl LiveGraphQuery {
    pub fn validate(&self)->Result<(), &'static str> {
        if self.player_nodes.is_empty() || self.player_nodes.len()>3 || self.keygroup_node==self.matrix_node
            || self.player_nodes.iter().enumerate().any(|(i,n)|*n==self.keygroup_node || *n==self.matrix_node || self.player_nodes[..i].contains(n)) { Err("invalid_query") } else { Ok(()) }
    }
}
#[derive(Debug)]
pub struct LiveGraphReceipt { pub value:Json }
#[derive(Default)]
struct Budget { nodes:usize, steps:usize, bytes:usize }
impl Budget {
    fn step(&mut self,n:usize)->Result<(), &'static str> { self.steps=self.steps.checked_add(n).ok_or("field_budget")?; if self.steps>STEPS {Err("field_budget")}else{Ok(())} }
    fn bytes(&mut self,n:usize)->Result<(), &'static str> { if n>65536 {return Err("scalar_size");} self.bytes=self.bytes.checked_add(n).ok_or("byte_budget")?; if self.bytes>BYTES {Err("byte_budget")}else{Ok(())} }
}
fn read<T:mlua::FromLua>(t:&Table,k:&str)->Result<T,&'static str>{t.raw_get(k).map_err(|_|"invalid_owner_table")}
fn id(t:&Table)->Result<usize,&'static str>{read(t,"__id")}
fn list(t:&Table,k:&str)->Result<Vec<Table>,&'static str>{
    let Some(t)=read::<Option<Table>>(t,k)? else{return Ok(Vec::new())};
    if t.raw_len()>8192{return Err("collection_budget")}
    (1..=t.raw_len()).map(|i|t.raw_get::<Table>(i).map_err(|_|"invalid_child_list")).collect()
}
#[cfg(feature="library-access")]
fn hash(b:&[u8])->Option<String>{use sha2::{Digest,Sha256};Some(format!("{:x}",Sha256::digest(b)))}
#[cfg(not(feature="library-access"))]
fn hash(_:&[u8])->Option<String>{None}
fn number(n:f64)->Json{if n.is_finite(){json!({"type":"number","value":n,"bits":n.to_bits()})}else{json!({"type":"nonfinite","available":false})}}
fn value(v:Value,b:&mut Budget)->Result<Json,&'static str>{Ok(match v{
    Value::Nil=>Json::Null,Value::Boolean(v)=>json!({"type":"boolean","value":v}),Value::Integer(v)=>number(v as f64),Value::Number(v)=>number(v),
    Value::String(s)=>{let bytes=s.as_bytes();b.bytes(bytes.len())?;json!({"type":"string_hash","utf8_bytes":bytes.len(),"sha256":hash(bytes.as_ref())})},
    _=>json!({"type":"unsupported","available":false})})}
fn scalar(h:&ScriptHost,t:&Table,i:usize,name:&str,b:&mut Budget)->Result<Json,&'static str>{
    let kinds=h.shared.kinds.borrow();let kind=kinds.get(i).ok_or("invalid_model_id")?;
    let mut def=None;for d in parameters::definitions(kind){b.step(1)?;if d.name==name{def=Some(d);break}}
    let params=h.shared.params.borrow();let mut retained=None;
    for(k,v)in params.get(i).ok_or("invalid_model_id")?{b.step(1)?;if k==name{b.bytes(v.len())?;retained=Some(v.as_str());break}}
    let overlay=match read::<Option<Table>>(t,"__set")?{Some(o)=>value(o.raw_get(name).map_err(|_|"invalid_overlay")?,b)?,None=>Json::Null};
    let retained=match retained{None=>Json::Null,Some(s)if def.is_some_and(|d|d.kind=="boolean")=>json!({"type":"boolean","value":s=="1"||s=="true"}),Some(s)=>match s.parse::<f64>(){Ok(n)=>number(n),Err(_)=>json!({"type":"string_hash","utf8_bytes":s.len(),"sha256":hash(s.as_bytes())})}};
    let default=def.map(|d|if d.kind=="boolean"{json!({"type":"boolean","value":d.default!=0.})}else{number(d.default)}).unwrap_or(Json::Null);
    let(current,provenance)=if !overlay.is_null(){(overlay.clone(),"overlay")}else if !retained.is_null(){(retained.clone(),"retained_params")}else if !default.is_null(){(default.clone(),"catalog_default")}else{(Json::Null,"unavailable")};
    let live=h.shared.live.borrow().get(i).is_some_and(|v|v.contains(&name));
    let scope=h.shared.scopes.borrow().get(i).copied().flatten();
    let route=if name=="Bypass"&&live{"special_insert_bypass"}else if live{"declared_engine_lane"}
        else if scope.is_some()&&matches!(name,"Gain"|"Pan"){"special_scope_parameter"}
        else if matches!(scope,Some(super::Scope::Oscillator(_)))&&name=="Pitch"{"special_scope_parameter"}
        else if matches!(scope,Some(super::Scope::Program))&&name=="Polyphony"{"special_scope_parameter"}
        else{"no_declared_runtime_route"};
    let binding=if live{h.shared.nodes.borrow().get(i).and_then(|(n,_)|*n).and_then(|n|crate::engine_parameters::binding(n,kind,name))}else{None};
    let binding=binding.map(|v|{let control_id=v.control.0;json!({"parameter":v.address.parameter,"group":v.address.group,"slot":v.address.slot,"generic":v.address.generic,"control_id":control_id.to_string(),"law":v.law})});
    Ok(json!({"current":current,"provenance":provenance,"overlay":overlay,"retained_params":retained,"catalog_default":default,"catalog_type":def.map(|d|d.kind),"declared_live":live,"runtime_route":route,"potential_binding":binding,"binding_status":if binding.is_some(){"prepared_check"}else{"unbound_or_special_route"}}))
}
struct Entry{table:Table,parent:Option<usize>,ordinal:usize}
impl ScriptHost{
    pub fn observe_live_graph(&self,q:&LiveGraphQuery)->LiveGraphReceipt{
        let mut b=Budget::default();let mut v=self.live_graph_inner(q,&mut b).unwrap_or_else(|r|json!({"schema_version":1,"status":"refused","reason":r,"query":q,"complete":false}));
        v["work"]=json!({"nodes":b.nodes,"field_steps":b.steps,"scalar_bytes":b.bytes});
        struct Counter(usize);impl std::io::Write for Counter{
            fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.0=self.0.checked_add(b.len()).ok_or(std::io::ErrorKind::OutOfMemory)?;if self.0>SIZE{return Err(std::io::ErrorKind::OutOfMemory.into())}Ok(b.len())}
            fn flush(&mut self)->std::io::Result<()>{Ok(())}}
        if serde_json::to_writer(&mut Counter(0),&v).is_err(){v=json!({"schema_version":1,"status":"refused","reason":"receipt_size","complete":false})}
        LiveGraphReceipt{value:v}
    }
    fn live_graph_inner(&self,q:&LiveGraphQuery,b:&mut Budget)->Result<Json,&'static str>{
        q.validate()?;let root:Table=self.lua.globals().raw_get("Program").map_err(|_|"program_unavailable")?;
        let mut pending=vec![(root,None,0usize,1usize)];let mut all=BTreeMap::new();let mut physical=BTreeMap::new();
        let nodes=self.shared.nodes.borrow();let kinds=self.shared.kinds.borrow();
        while let Some((table,parent,ordinal,depth))=pending.pop(){
            b.nodes+=1;b.step(1)?;if b.nodes>GRAPH||depth>192{return Err("graph_budget")}
            let i=id(&table)?;let &(node,_)=nodes.get(i).ok_or("invalid_model_id")?;
            let kind:String=read(&table,"type")?;b.bytes(kind.len())?;if kinds.get(i)!=Some(&kind){return Err("mutated_kind")}
            if let Some(p)=parent{if id(&read::<Table>(&table,"parent")?)?!=p{return Err("inconsistent_parent")}}
            if all.insert(i,Entry{table:table.clone(),parent,ordinal}).is_some(){return Err("duplicate_or_cycle")}
            if let Some(n)=node{if physical.insert(n,i).is_some(){return Err("duplicate_physical_node")}}
            let children=list(&table,"children")?;
            if b.nodes.checked_add(pending.len()).and_then(|n|n.checked_add(children.len())).is_none_or(|n|n>GRAPH){return Err("graph_budget")}
            for(o,c)in children.into_iter().enumerate().rev(){pending.push((c,Some(i),o,depth+1))}
        }
        let lookup=|n:u32|physical.get(&(n as usize)).copied().ok_or("node_missing");
        let key=lookup(q.keygroup_node)?;let matrix=lookup(q.matrix_node)?;
        if kinds[key]!="Keygroup"||kinds[matrix]!="GainMatrix"{return Err("wrong_required_kind")}
        let mut selected=BTreeSet::new();let mut ancestors=Vec::new();let mut next=Some(key);
        while let Some(i)=next{if ancestors.len()==16||ancestors.contains(&i){return Err("ancestor_budget_or_cycle")}ancestors.push(i);selected.insert(i);next=all[&i].parent}
        let first=list(&all[&key].table,"inserts")?.into_iter().next().ok_or("keygroup_insert_missing")?;
        if id(&first)?!=matrix {return Err("required_matrix_not_first_keygroup_insert")}
        let mut players=Vec::new();
        for t in list(&all[&key].table,"oscillators")?{let i=id(&t)?;let n=nodes.get(i).and_then(|(n,_)|*n).ok_or("nonphysical_player")?;if kinds.get(i).is_none_or(|k|k!="SamplePlayer"){return Err("unsupported_oscillator_wrapper")}players.push(n as u32)}
        if players.len()!=q.player_nodes.len()||q.player_nodes.iter().any(|n|!players.contains(n)){return Err("player_membership_mismatch")}
        for n in &q.player_nodes{let i=lookup(*n)?;if all[&i].parent!=Some(key){return Err("player_scope_mismatch")}selected.insert(i);}
        let mut seeds=Vec::new();
        for &i in &selected{for collection in ["inserts","auxs","sends","connections"]{for(o,t)in list(&all[&i].table,collection)?.into_iter().enumerate(){
            let j=id(&t)?;if !all.contains_key(&j)||all[&j].parent!=Some(i){return Err("collection_identity_mismatch")}
            let _ = o; seeds.push(j);
        }}}
        while let Some(i)=seeds.pop(){if !selected.insert(i){continue}if selected.len()>128{return Err("selected_node_budget")}let e=all.get(&i).ok_or("collection_identity_mismatch")?;for t in list(&e.table,"children")?{seeds.push(id(&t)?);}}
        if !selected.contains(&matrix){return Err("matrix_not_on_selected_path")}
        let synthetic=read::<Option<Table>>(&all[ancestors.last().ok_or("ancestor_missing")?].table,"parent")?;
        let mut rows=Vec::new();let mut matrices=Vec::new();let mut inserts=Vec::new();let mut routes=Vec::new();
        for i in selected{let e=&all[&i];let mut collections=serde_json::Map::new();
            for collection in ["inserts","auxs","sends","connections","oscillators"] {
                let mut ids=Vec::new();
                for (ordinal,t) in list(&e.table,collection)?.into_iter().enumerate() {
                    let j=id(&t)?;if !all.contains_key(&j)||all[&j].parent!=Some(i){return Err("collection_identity_mismatch")}
                    ids.push(nodes[j].0);
                    let row=json!({"owner_node":nodes[i].0,"collection":collection,"ordinal":ordinal,"node":nodes[j].0,"kind":kinds[j]});
                    if collection=="inserts" {if inserts.len()==32{return Err("insert_budget")}inserts.push(row);}
                    else if matches!(collection,"auxs"|"sends") {if routes.len()==16{return Err("route_budget")}routes.push(row);}
                }
                if !ids.is_empty(){collections.insert(collection.into(),json!(ids));}
            }
            let mut fields=serde_json::Map::new();for &name in FIELDS{let v=scalar(self,&e.table,i,name,b)?;if v["catalog_type"]=="boolean" && v["current"]["type"]!="boolean" {return Err("invalid_catalog_boolean")}fields.insert(name.into(),if v["current"].is_null(){json!({"current":Json::Null,"provenance":"unavailable"})}else{v});}
            if kinds[i]=="GainMatrix"{if matrices.len()==8{return Err("matrix_budget")}let mut cells=Vec::new();for output in 1..=12{let mut row=Vec::new();for input in 1..=12{let cell=scalar(self,&e.table,i,&format!("Gain_{input}_{output}"),b)?;
                    if cell["current"]["type"]!="number" || cell["current"]["value"].as_f64().is_none_or(|n|!n.is_finite()){return Err("matrix_nonnumeric_or_nonfinite")}
                    row.push(cell)}cells.push(row)}matrices.push(json!({"node":nodes[i].0,"cells":cells,"primary_width":6,"width":12,"ordering":"output-major; Gain_input_output","scope":"owner_model_not_DSP_matrix"}));}
            rows.push(json!({"node":nodes[i].0,"model_index":i,"kind":kinds[i],"parent":e.parent.and_then(|p|nodes[p].0),"child_ordinal":e.ordinal,"collections":collections,"fields":fields,"raw_bypass_property":value(e.table.raw_get("bypass").map_err(|_|"invalid_raw_bypass")?,b)?}));
        }
        if let Some(t)=synthetic{let i=id(&t)?;if nodes.get(i).is_none_or(|(n,_)|n.is_some())||kinds.get(i).is_none_or(|k|k!="Part"){return Err("invalid_synthetic_ancestor")}let mut f=serde_json::Map::new();for n in ["Bypass","BypassInsertFX","Gain","Pan"]{f.insert(n.into(),scalar(self,&t,i,n,b)?);}rows.push(json!({"node":Json::Null,"model_index":i,"kind":"Part","synthetic":true,"fields":f}));}
        if rows.len()>128{return Err("selected_node_budget")}
        Ok(json!({"schema_version":1,"status":"observed","complete":true,"query":q,"owner_time_ms":self.shared.now.get(),"nodes":rows,"matrices":matrices,"ancestors":ancestors.iter().map(|i|nodes[*i].0).collect::<Vec<_>>(),"ordered_inserts":inserts,"routes":routes,"oscillator_order":players,"fx_post_gain_support":"not_implemented; keygroup chain currently pre_amplitude","scope":"Current owner tables/getter model, not native scope or instantaneous DSP ramps"}))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    struct Empty;impl super::super::Files for Empty{fn script(&self,_:&str)->Option<String>{None}}
    fn fixture()->(ScriptHost,LiveGraphQuery){fixture_with_init(None)}
    fn fixture_with_init(init:Option<&str>)->(ScriptHost,LiveGraphQuery){
        let xml="<Program><Layers><Layer><Keygroups><Keygroup FXPostGain='0'><Inserts><GainMatrix Gain_3_1='-0.25' Gain_1_3='0.75' Gain_12_7='0.4'/></Inserts><Oscillators><SamplePlayer/><SamplePlayer Bypass='1'/><SamplePlayer Bypass='1'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>";
        let xml=match init {Some(source)=>xml.replace("</Program>",&format!("<EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>")),None=>xml.to_owned()};
        let d=roxmltree::Document::parse(&xml).unwrap();let n=|k:&str|d.descendants().find(|n|n.has_tag_name(k)).unwrap().id().get_usize() as u32;
        let q=LiveGraphQuery{keygroup_node:n("Keygroup"),matrix_node:n("GainMatrix"),player_nodes:d.descendants().filter(|n|n.has_tag_name("SamplePlayer")).map(|n|n.id().get_usize()as u32).collect()};
        (ScriptHost::new(&xml,Empty,super::super::Config::default()).unwrap(),q)
    }
    #[test]fn signed_direction_outer_cells_default_and_unbound(){let(h,q)=fixture();let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");let c=&v["matrices"][0]["cells"];assert_eq!(c[0][2]["current"]["value"],-0.25);assert_eq!(c[2][0]["current"]["value"],0.75);assert_eq!(c[6][11]["current"]["value"],0.4);assert_eq!(c[5][5]["current"]["value"],1.);assert_eq!(c[0][2]["declared_live"],false);assert_eq!(v["oscillator_order"].as_array().unwrap().len(),3);let k=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.keygroup_node).unwrap();assert_eq!(k["fields"]["FXPostGain"]["current"]["type"],"boolean");assert_eq!(k["fields"]["FXPostGain"]["current"]["value"],false);}
    #[test]fn real_initializer_fold_is_retained_without_inventing_live_lane(){
        let(h,q)=fixture_with_init(Some("Program.layers[1].keygroups[1].inserts[1]:setParameter('Gain_3_1', -0.6)"));
        let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");
        let c=&v["matrices"][0]["cells"][0][2];assert_eq!(c["current"]["value"],-0.6);
        assert_eq!(c["retained_params"]["value"],-0.6);assert_eq!(c["declared_live"],false);
        assert!(h.insert_overrides().iter().any(|(n,k,_)|*n==q.matrix_node as usize&&k=="Gain_3_1"));
    }
    #[test]fn overlay_is_distinct_from_params_and_constructor_bypass(){let(h,q)=fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();let l=list(&p,"layers").unwrap().remove(0);let k=list(&l,"keygroups").unwrap().remove(0);let m=list(&k,"inserts").unwrap().remove(0);let o=h.lua.create_table().unwrap();o.raw_set("Gain_3_1",0.9).unwrap();o.raw_set("Bypass",true).unwrap();m.raw_set("__set",o).unwrap();let v=h.observe_live_graph(&q).value;let c=&v["matrices"][0]["cells"][0][2];assert_eq!(c["current"]["value"],0.9);assert_eq!(c["retained_params"]["value"],-0.25);let m=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.matrix_node).unwrap();assert_eq!(m["fields"]["Bypass"]["current"]["value"],true);assert_eq!(m["raw_bypass_property"]["value"],false);}
    #[test]fn missing_duplicate_cycle_and_limits_refuse(){let(h,mut q)=fixture();q.matrix_node=u32::MAX;assert_eq!(h.observe_live_graph(&q).value["reason"],"node_missing");q.player_nodes.push(q.player_nodes[0]);assert!(q.validate().is_err());let(h,q)=fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();let c:Table=p.raw_get("children").unwrap();c.raw_set(2,p.clone()).unwrap();assert_eq!(h.observe_live_graph(&q).value["status"],"refused");assert!(Budget::default().step(STEPS+1).is_err());assert!(Budget::default().bytes(65537).is_err());assert!(Budget{bytes:BYTES,..Budget::default()}.bytes(1).is_err());}    #[test]fn unknown_does_not_become_zero_and_malformed_owner_is_refused(){
        let(h,q)=fixture();let v=h.observe_live_graph(&q).value;
        let k=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.keygroup_node).unwrap();
        assert!(k["fields"]["Source"]["current"].is_null());
        let p:Table=h.lua.globals().raw_get("Program").unwrap();p.raw_set("type","not-the-original-kind").unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"mutated_kind");
        let(h,q)=fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();let c:Table=p.raw_get("children").unwrap();
        for i in 1..=8193{c.raw_set(i,p.clone()).unwrap();}
        assert_eq!(h.observe_live_graph(&q).value["reason"],"collection_budget");
    }
    #[test]fn invalid_matrix_scalar_refuses_instead_of_coercing_or_zeroing(){
        let(h,q)=fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();let l=list(&p,"layers").unwrap().remove(0);
        let k=list(&l,"keygroups").unwrap().remove(0);let m=list(&k,"inserts").unwrap().remove(0);let o=h.lua.create_table().unwrap();
        o.raw_set("Gain_3_1",true).unwrap();m.raw_set("__set",o.clone()).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"matrix_nonnumeric_or_nonfinite");
        o.raw_set("Gain_3_1",f64::NAN).unwrap();assert_eq!(h.observe_live_graph(&q).value["reason"],"matrix_nonnumeric_or_nonfinite");
    }

}
