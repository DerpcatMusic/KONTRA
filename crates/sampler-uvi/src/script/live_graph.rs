//! Bounded owner-only projection. Raw reads, no authored Lua calls, collector or IO.
use super::{ScriptHost, Table, Value, parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};
use std::collections::{BTreeMap, BTreeSet};
const GRAPH: usize = 262_144;
const STEPS: usize = 1_048_576;
const BYTES: usize = 16 << 20;
const SIZE: usize = 256 << 10;
const FIELDS: &[&str] = &["Bypass","BypassInsertFX","FXPostGain","Gain","Pan","Pitch","BaseNote","NoteTracking","LowKey","HighKey","LowVelocity","HighVelocity","Volume","Frequency","Freq","Mode","KeyTracking","SampleStart","SampleEnd","SamplePath","Source","Destination","Ratio","ConnectionMode","Inverted","Mapper","Dry","Wet","Output","Bus","PrePost","PreFader","Enabled"];
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
struct Budget { nodes:usize, steps:usize, bytes:usize, exhausted:bool }
impl Budget {
    fn step(&mut self,n:usize)->Result<(), &'static str> {
        match self.steps.checked_add(n) {
            Some(next) if next<=STEPS=>{self.steps=next;Ok(())},
            _=>{self.exhausted=true;Err("field_budget")},
        }
    }
    fn bytes(&mut self,n:usize)->Result<(), &'static str> {
        if n>65536{return Err("scalar_size")}
        match self.bytes.checked_add(n) {
            Some(next) if next<=BYTES=>{self.bytes=next;Ok(())},
            _=>Err("byte_budget"),
        }
    }
}
fn read<T:mlua::FromLua>(t:&Table,k:&str,b:&mut Budget)->Result<T,&'static str>{
    b.step(1)?;t.raw_get(k).map_err(|_|"invalid_owner_table")
}
fn id(t:&Table,b:&mut Budget)->Result<usize,&'static str>{read(t,"__id",b)}
fn same_table(a:&Table,z:&Table,b:&mut Budget)->Result<bool,&'static str>{
    b.step(1)?;Ok(a.to_pointer()==z.to_pointer())
}
fn list(t:&Table,k:&str,b:&mut Budget)->Result<Vec<Table>,&'static str>{
    let Some(t)=read::<Option<Table>>(t,k,b)? else{return Ok(Vec::new())};
    b.step(1)?;let len=t.raw_len();if len>8192{return Err("collection_budget")}
    // Reserve every extraction before allocation. Duplicate comparisons are charged
    // individually before performing them; a late size refusal cannot replace this.
    b.step(len)?;let mut out:Vec<Table>=Vec::with_capacity(len);let mut seen=Vec::with_capacity(len);
    for index in 1..=len {
        let child=t.raw_get::<Table>(index).map_err(|_|"invalid_child_list")?;
        let model=id(&child,b)?;b.step(1)?;let pointer=child.to_pointer();
        for &(prior_model,prior_pointer) in &seen {
            b.step(2)?;
            if model==prior_model || pointer==prior_pointer{return Err("duplicate_collection_member")}
        }
        b.step(1)?;seen.push((model,pointer));out.push(child);
    }
    Ok(out)
}
#[cfg(feature="library-access")]
fn hash(b:&[u8])->Option<String>{use sha2::{Digest,Sha256};Some(format!("{:x}",Sha256::digest(b)))}
#[cfg(not(feature="library-access"))]
fn hash(_:&[u8])->Option<String>{None}
fn number(n:f64)->Json{if n.is_finite(){json!({"type":"number","value":n,"bits":n.to_bits()})}else{json!({"type":"nonfinite","available":false})}}
fn value(v:Value,b:&mut Budget)->Result<Json,&'static str>{b.step(1)?;Ok(match v{
    Value::Nil=>Json::Null,Value::Boolean(v)=>json!({"type":"boolean","value":v}),Value::Integer(v)=>number(v as f64),Value::Number(v)=>number(v),
    Value::String(s)=>{let bytes=s.as_bytes();b.bytes(bytes.len())?;json!({"type":"string_hash","utf8_bytes":bytes.len(),"sha256":hash(bytes.as_ref())})},
    _=>json!({"type":"unsupported","available":false})})}
fn scalar(h:&ScriptHost,t:&Table,i:usize,name:&str,b:&mut Budget)->Result<Json,&'static str>{
    b.step(1)?;let kinds=h.shared.kinds.borrow();let kind=kinds.get(i).ok_or("invalid_model_id")?;
    let mut def=None;for d in parameters::definitions(kind){b.step(1)?;if d.name==name{def=Some(d);break}}
    let params=h.shared.params.borrow();let mut retained=None;
    for(k,v)in params.get(i).ok_or("invalid_model_id")?{b.step(1)?;if k==name{b.bytes(v.len())?;retained=Some(v.as_str());break}}
    let overlay=match read::<Option<Table>>(t,"__set",b)?{
        Some(o)=>{
            b.step(1)?;
            // The real getter uses overlay[name]. Never substitute raw fallback
            // for an overlay that may run __index (even if this cell is present).
            if o.metatable().is_some(){return Err("overlay_metatable_unsupported")}
            value(read::<Value>(&o,name,b)?,b)?
        },None=>Json::Null,
    };
    let retained=match retained{None=>Json::Null,Some(s)if def.is_some_and(|d|d.kind=="boolean")=>json!({"type":"boolean","value":s=="1"||s=="true"}),Some(s)=>match s.parse::<f64>(){Ok(n)=>number(n),Err(_)=>json!({"type":"string_hash","utf8_bytes":s.len(),"sha256":hash(s.as_bytes())})}};
    let default=def.map(|d|if d.kind=="boolean"{json!({"type":"boolean","value":d.default!=0.})}else{number(d.default)}).unwrap_or(Json::Null);
    let(current,provenance)=if !overlay.is_null(){(overlay.clone(),"overlay")}else if !retained.is_null(){(retained.clone(),"retained_params")}else if !default.is_null(){(default.clone(),"catalog_default")}else{(Json::Null,"unavailable")};
    b.step(3)?;let live={let live=h.shared.live.borrow();let mut found=false;
        if let Some(names)=live.get(i){for candidate in names{b.step(1)?;if *candidate==name{found=true;break}}}found};
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
fn canonical_member(t:&Table,parent:usize,all:&BTreeMap<usize,Entry>,b:&mut Budget)->Result<usize,&'static str>{
    let i=id(t,b)?;b.step(2)?;let e=all.get(&i).ok_or("collection_identity_mismatch")?;
    if e.parent!=Some(parent)||!same_table(t,&e.table,b)?{return Err("collection_identity_mismatch")}
    let actual:Table=read(t,"parent",b)?;b.step(1)?;let expected=&all.get(&parent).ok_or("inconsistent_parent")?.table;
    if !same_table(&actual,expected,b)?{return Err("inconsistent_parent")}
    Ok(i)
}
// Check the selected synthesis path in the same plural collections that ordinary
// callers use. Only existing Program/Layer/Keygroup containers are supported;
// an unknown wrapper is not inferred from the independent children traversal.
fn selected_ancestry(
    ancestors:&[usize],all:&BTreeMap<usize,Entry>,kinds:&[String],b:&mut Budget,
)->Result<(),&'static str>{
    b.step(1)?;let root=*ancestors.last().ok_or("ancestor_missing")?;
    if kinds.get(root).is_none_or(|k|k!="Program"){return Err("unsupported_selected_ancestry")}
    for edge in ancestors.windows(2){
        b.step(3)?;let(child,parent)=(edge[0],edge[1]);
        if !matches!(kinds[parent].as_str(),"Program"|"Layer"|"Keygroup"){
            return Err("unsupported_selected_ancestry");
        }
        let collection=match kinds[child].as_str(){
            "Layer"=>"layers","Keygroup"=>"keygroups",
            _=>return Err("unsupported_selected_ancestry"),
        };
        let mut found=false;
        for entry in list(&all[&parent].table,collection,b)?{
            let member=id(&entry,b)?;b.step(1)?;
            if member==child{
                if canonical_member(&entry,parent,all,b)?!=child{return Err("collection_identity_mismatch")}
                found=true;
            }
        }
        if !found{return Err("selected_ancestry_membership_missing")}
    }
    Ok(())
}
impl ScriptHost{
    pub fn observe_live_graph(&self,q:&LiveGraphQuery)->LiveGraphReceipt{
        // Public callers need not come through UiBridge. Reject before VM reads
        // or any query-to-JSON conversion; a rejected vector is never reflected.
        if q.validate().is_err(){
            return LiveGraphReceipt{value:json!({"schema_version":1,"status":"refused",
                "reason":"invalid_query","complete":false,
                "work":{"nodes":0,"field_steps":0,"scalar_bytes":0,"field_budget_exhausted":false}})};
        }
        let mut b=Budget::default();let mut v=self.live_graph_inner(q,&mut b).unwrap_or_else(|r|json!({"schema_version":1,"status":"refused","reason":r,"complete":false}));
        v["work"]=json!({"nodes":b.nodes,"field_steps":b.steps,"scalar_bytes":b.bytes,"field_budget_exhausted":b.exhausted});
        struct Counter(usize);impl std::io::Write for Counter{
            fn write(&mut self,b:&[u8])->std::io::Result<usize>{self.0=self.0.checked_add(b.len()).ok_or(std::io::ErrorKind::OutOfMemory)?;if self.0>SIZE{return Err(std::io::ErrorKind::OutOfMemory.into())}Ok(b.len())}
            fn flush(&mut self)->std::io::Result<()>{Ok(())}}
        if serde_json::to_writer(&mut Counter(0),&v).is_err(){v=json!({"schema_version":1,"status":"refused","reason":"receipt_size","complete":false})}
        LiveGraphReceipt{value:v}
    }
    fn live_graph_inner(&self,q:&LiveGraphQuery,b:&mut Budget)->Result<Json,&'static str>{
        b.step(32)?;q.validate()?;let root:Table=read(&self.lua.globals(),"Program",b)?;
        let mut pending=vec![(root,None,0usize,1usize)];let mut all:BTreeMap<usize,Entry>=BTreeMap::new();let mut physical=BTreeMap::new();
        let nodes=self.shared.nodes.borrow();let kinds=self.shared.kinds.borrow();
        while let Some((table,parent,ordinal,depth))=pending.pop(){
            b.step(1)?;if b.nodes>=GRAPH||depth>192{return Err("graph_budget")}b.nodes+=1;
            let i=id(&table,b)?;b.step(2)?;let &(node,_)=nodes.get(i).ok_or("invalid_model_id")?;
            let kind:mlua::String=read(&table,"type",b)?;let kind_bytes=kind.as_bytes();
            b.bytes(kind_bytes.len())?;b.step(1)?;
            if kinds.get(i).is_none_or(|expected|expected.as_bytes()!=kind_bytes.as_ref()){return Err("mutated_kind")}
            if let Some(p)=parent{
                let actual:Table=read(&table,"parent",b)?;b.step(1)?;
                let expected=&all.get(&p).ok_or("inconsistent_parent")?.table;
                if id(&actual,b)?!=p||!same_table(&actual,expected,b)?{return Err("inconsistent_parent")}
            }
            b.step(1)?;if all.insert(i,Entry{table:table.clone(),parent,ordinal}).is_some(){return Err("duplicate_or_cycle")}
            if let Some(n)=node{b.step(1)?;if physical.insert(n,i).is_some(){return Err("duplicate_physical_node")}}
            let children=list(&table,"children",b)?;
            if b.nodes.checked_add(pending.len()).and_then(|n|n.checked_add(children.len())).is_none_or(|n|n>GRAPH){return Err("graph_budget")}
            for(o,c)in children.into_iter().enumerate().rev(){b.step(1)?;pending.push((c,Some(i),o,depth+1))}
        }
        let lookup=|n:u32,b:&mut Budget|{b.step(1)?;physical.get(&(n as usize)).copied().ok_or("node_missing")};
        let key=lookup(q.keygroup_node,b)?;let matrix=lookup(q.matrix_node,b)?;
        b.step(2)?;if kinds[key]!="Keygroup"||kinds[matrix]!="GainMatrix"{return Err("wrong_required_kind")}
        let mut selected=BTreeSet::new();let mut ancestors=Vec::new();let mut next=Some(key);
        while let Some(i)=next{
            b.step(ancestors.len()+2)?;if ancestors.len()==16||ancestors.contains(&i){return Err("ancestor_budget_or_cycle")}
            ancestors.push(i);selected.insert(i);next=all[&i].parent;
        }
        selected_ancestry(&ancestors,&all,kinds.as_slice(),b)?;
        let first=list(&all[&key].table,"inserts",b)?.into_iter().next().ok_or("keygroup_insert_missing")?;
        if canonical_member(&first,key,&all,b)?!=matrix{return Err("required_matrix_not_first_keygroup_insert")}
        let mut players=Vec::new();
        for t in list(&all[&key].table,"oscillators",b)?{
            let i=canonical_member(&t,key,&all,b)?;b.step(2)?;
            let n=nodes.get(i).and_then(|(n,_)|*n).ok_or("nonphysical_player")?;
            if kinds.get(i).is_none_or(|k|k!="SamplePlayer"){return Err("unsupported_oscillator_wrapper")}players.push(n as u32);
        }
        b.step(q.player_nodes.len()*players.len()+1)?;
        if players.len()!=q.player_nodes.len()||q.player_nodes.iter().any(|n|!players.contains(n)){return Err("player_membership_mismatch")}
        for n in &q.player_nodes{let i=lookup(*n,b)?;b.step(2)?;if all[&i].parent!=Some(key){return Err("player_scope_mismatch")}selected.insert(i);}
        // A single deterministic hop from synthesis owners/queried players.
        // Descendant processors can affect sound, but are deliberately unexpanded;
        // the receipt below must never claim their inventory or routing complete.
        b.step(selected.len())?;let base=selected.clone();
        for &i in &base{for collection in ["inserts","auxs","sends"]{
            for t in list(&all[&i].table,collection,b)?{
                let j=canonical_member(&t,i,&all,b)?;b.step(1)?;selected.insert(j);
                if selected.len()>128{return Err("selected_node_budget")}
            }
        }}
        b.step(1)?;if !selected.contains(&matrix){return Err("matrix_not_on_selected_path")}
        let selected_model_nodes=selected.len();
        // Whole-model omissions include synthesis siblings, not merely audio
        // processors. Counts intentionally make no claim about their relevance.
        let unprojected_model_nodes=all.len().checked_sub(selected_model_nodes).ok_or("selection_count")?;
        let mut unexpanded_child_edges=0usize;
        // Count existing canonical edges only. This does not traverse a second VM
        // graph, infer causal relevance, or count transitive descendants as edges.
        for (&i,e) in &all{
            b.step(3)?;
            if !selected.contains(&i)&&e.parent.is_some_and(|p|selected.contains(&p)){
                unexpanded_child_edges=unexpanded_child_edges.checked_add(1).ok_or("selection_count")?;
            }
        }
        let root_id=*ancestors.last().ok_or("ancestor_missing")?;let root=&all[&root_id].table;
        let synthetic=read::<Option<Table>>(root,"parent",b)?;
        if let Some(t)=&synthetic{
            let part:Table=read(root,"part",b)?;
            if !same_table(t,&part,b)?{return Err("synthetic_parent_alias")}
        }
        let mut rows=Vec::new();let mut matrices=Vec::new();let mut inserts=Vec::new();let mut routes=Vec::new();
        for &i in &selected{
            b.step(1)?;let e=&all[&i];let mut collections=serde_json::Map::new();let mut collection_members=Vec::new();
            for collection in ["inserts","auxs","sends","connections","oscillators"]{
                let mut ids=Vec::new();
                for(ordinal,t)in list(&e.table,collection,b)?.into_iter().enumerate(){
                    let j=canonical_member(&t,i,&all,b)?;
                    for &prior in &collection_members{b.step(1)?;if prior==j{return Err("duplicate_collection_membership")}}
                    b.step(3)?;collection_members.push(j);ids.push(nodes[j].0);
                    let mut row=json!({"owner_node":nodes[i].0,"collection":collection,"ordinal":ordinal,"node":nodes[j].0,"kind":kinds[j]});
                    if collection=="inserts"{if inserts.len()==32{return Err("insert_budget")}inserts.push(row);}
                    else if matches!(collection,"auxs"|"sends"){
                        if routes.len()==16{return Err("route_budget")}
                        // Inventoried descendants outside the one-hop selection
                        // retain identity/order only. Their scalar state is not
                        // a prerequisite for the required owner facts.
                        b.step(1)?;
                        if selected.contains(&j){
                            let mut fields=serde_json::Map::new();for name in ["PreFader","Bypass","Gain","Destination"]{fields.insert(name.into(),scalar(self,&t,j,name,b)?);}
                            row["fields"]=json!(fields);row["scalar_projection"]=json!("selected_current_fields");
                        }else{
                            row["scalar_projection"]=json!("omitted_unselected_member");
                        }
                        row["compiled_linkage"]=json!("unavailable");routes.push(row);
                    }
                }
                if !ids.is_empty(){b.step(1)?;collections.insert(collection.into(),json!(ids));}
            }
            let mut fields=serde_json::Map::new();
            for &name in FIELDS{
                let v=scalar(self,&e.table,i,name,b)?;b.step(1)?;
                if v["catalog_type"]=="boolean"&&v["current"]["type"]!="boolean"{return Err("invalid_catalog_boolean")}
                fields.insert(name.into(),if v["current"].is_null(){json!({"current":Json::Null,"provenance":"unavailable"})}else{v});
            }
            if kinds[i]=="GainMatrix"{
                if matrices.len()==8{return Err("matrix_budget")}
                let mut cells=Vec::new();for output in 1..=12{let mut row=Vec::new();for input in 1..=12{
                    let cell=scalar(self,&e.table,i,&format!("Gain_{input}_{output}"),b)?;b.step(1)?;
                    if cell["current"]["type"]!="number"||cell["current"]["value"].as_f64().is_none_or(|n|!n.is_finite()){return Err("matrix_nonnumeric_or_nonfinite")}row.push(cell);
                }cells.push(row)}
                matrices.push(json!({"node":nodes[i].0,"cells":cells,"primary_width":6,"width":12,"ordering":"output-major; Gain_input_output","scope":"owner_model_not_DSP_matrix"}));
            }
            rows.push(json!({"node":nodes[i].0,"model_index":i,"kind":kinds[i],"parent":e.parent.and_then(|p|nodes[p].0),"child_ordinal":e.ordinal,"collections":collections,"fields":fields,"raw_bypass_property":value(read::<Value>(&e.table,"bypass",b)?,b)?}));
        }
        if let Some(t)=synthetic{
            let i=id(&t,b)?;b.step(3)?;
            if nodes.get(i).is_none_or(|(n,_)|n.is_some())||kinds.get(i).is_none_or(|k|k!="Part"){return Err("invalid_synthetic_ancestor")}
            let mut f=serde_json::Map::new();for n in ["Bypass","BypassInsertFX","Gain","Pan"]{f.insert(n.into(),scalar(self,&t,i,n,b)?);}
            rows.push(json!({"node":Json::Null,"model_index":i,"kind":"Part","synthetic":true,"fields":f}));
        }
        if rows.len()>128{return Err("selected_node_budget")}
        let inventory_complete=unprojected_model_nodes==0&&unexpanded_child_edges==0;
        let route_complete=inventory_complete&&routes.is_empty();
        Ok(json!({"schema_version":1,"status":"observed","inventory_complete":inventory_complete,"complete":route_complete,
            "selection":{"policy":"required_owner_facts_v1","required_facts_complete":true,
                "selected_model_nodes":selected_model_nodes,"unprojected_model_nodes":unprojected_model_nodes,
                "unexpanded_child_edges":unexpanded_child_edges},
            "route_topology":{"complete":route_complete,"scope":"Owner route inventory only; compiled route linkage is separate","reason":if route_complete{"no_selected_owner_audio_route"}else{"compiled_route_linkage_unavailable"}},
            "query":q,"owner_time_ms":self.shared.now.get(),"nodes":rows,"matrices":matrices,"ancestors":ancestors.iter().map(|i|nodes[*i].0).collect::<Vec<_>>(),"ordered_inserts":inserts,"routes":routes,"oscillator_order":players,"fx_post_gain_support":"not_implemented; keygroup chain currently pre_amplitude","scope":"Current owner tables/getter model, not native scope or instantaneous DSP ramps"}))
    }
}
#[cfg(test)]
mod tests{
    use super::*;
    fn list(t:&Table,k:&str)->Result<Vec<Table>,&'static str>{super::list(t,k,&mut Budget::default())}
    struct Empty;impl super::super::Files for Empty{fn script(&self,_:&str)->Option<String>{None}}
    fn fixture()->(ScriptHost,LiveGraphQuery){fixture_with_init(None)}
    fn fixture_with_init(init:Option<&str>)->(ScriptHost,LiveGraphQuery){
        let xml="<Program><Layers><Layer><Keygroups><Keygroup FXPostGain='0'><Inserts><GainMatrix Gain_3_1='-0.25' Gain_1_3='0.75' Gain_12_7='0.4'/></Inserts><Oscillators><SamplePlayer/><SamplePlayer Bypass='1'/><SamplePlayer Bypass='1'/></Oscillators></Keygroup></Keygroups></Layer></Layers></Program>";
        let xml=match init {Some(source)=>xml.replace("</Program>",&format!("<EventProcessors><ScriptProcessor><script><![CDATA[{source}]]></script></ScriptProcessor></EventProcessors></Program>")),None=>xml.to_owned()};
        let d=roxmltree::Document::parse(&xml).unwrap();let n=|k:&str|d.descendants().find(|n|n.has_tag_name(k)).unwrap().id().get_usize() as u32;
        let q=LiveGraphQuery{keygroup_node:n("Keygroup"),matrix_node:n("GainMatrix"),player_nodes:d.descendants().filter(|n|n.has_tag_name("SamplePlayer")).map(|n|n.id().get_usize()as u32).collect()};
        (ScriptHost::new(&xml,Empty,super::super::Config::default()).unwrap(),q)
    }
    #[test]fn signed_direction_outer_cells_default_and_declared_live(){let(h,q)=fixture();let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");let c=&v["matrices"][0]["cells"];assert_eq!(c[0][2]["current"]["value"],-0.25);assert_eq!(c[2][0]["current"]["value"],0.75);assert_eq!(c[6][11]["current"]["value"],0.4);assert_eq!(c[5][5]["current"]["value"],1.);assert_eq!(c[0][2]["declared_live"],true);assert_eq!(c[2][0]["declared_live"],true);assert_eq!(c[6][11]["declared_live"],true);assert_eq!(v["oscillator_order"].as_array().unwrap().len(),3);let k=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.keygroup_node).unwrap();assert_eq!(k["fields"]["FXPostGain"]["current"]["type"],"boolean");assert_eq!(k["fields"]["FXPostGain"]["current"]["value"],false);}
    #[test]fn real_initializer_fold_reaches_declared_live_lane_and_keeps_retained_value(){
        let(h,q)=fixture_with_init(Some("Program.layers[1].keygroups[1].inserts[1]:setParameter('Gain_3_1', -0.6)"));
        let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");
        let c=&v["matrices"][0]["cells"][0][2];assert_eq!(c["current"]["value"],-0.6);
        assert_eq!(c["retained_params"]["value"],-0.6);assert_eq!(c["declared_live"],true);
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

    fn routed_fixture(pre_fader:bool)->(ScriptHost,LiveGraphQuery){
        let xml=format!("<Program><Layers><Layer><Keygroups><Keygroup><Inserts><GainMatrix Gain_3_1='-0.25'/></Inserts><Oscillators><SamplePlayer/><SamplePlayer Bypass='1'/><SamplePlayer Bypass='1'/></Oscillators><BusRouters><BusRouter PreFader='{}' Gain='0.4' Destination='authored-destination'/></BusRouters></Keygroup></Keygroups></Layer></Layers></Program>",u8::from(pre_fader));
        let d=roxmltree::Document::parse(&xml).unwrap();let node=|k:&str|d.descendants().find(|n|n.has_tag_name(k)).unwrap().id().get_usize()as u32;
        let q=LiveGraphQuery{keygroup_node:node("Keygroup"),matrix_node:node("GainMatrix"),player_nodes:d.descendants().filter(|n|n.has_tag_name("SamplePlayer")).map(|n|n.id().get_usize()as u32).collect()};
        (ScriptHost::new(&xml,Empty,super::super::Config::default()).unwrap(),q)
    }
    fn keygroup(h:&ScriptHost)->Table{
        let p:Table=h.lua.globals().raw_get("Program").unwrap();let l=list(&p,"layers").unwrap().remove(0);list(&l,"keygroups").unwrap().remove(0)
    }
    fn shallow(h:&ScriptHost,t:&Table)->Table{
        let copy=h.lua.create_table().unwrap();for pair in t.pairs::<Value,Value>(){let(k,v)=pair.unwrap();copy.raw_set(k,v).unwrap();}
        copy.set_metatable(t.metatable()).unwrap();copy
    }
    #[test]
    fn collection_copies_with_different_getter_state_are_refused(){
        for collection in ["inserts","oscillators","sends"]{
            let(h,q)=routed_fixture(false);let k=keygroup(&h);let original=list(&k,collection).unwrap().remove(0);let copy=shallow(&h,&original);
            let overlay=h.lua.create_table().unwrap();overlay.raw_set("Gain_3_1",0.875).unwrap();copy.raw_set("__set",overlay).unwrap();
            if collection=="inserts"{
                let getter:mlua::Function=copy.get("getParameter").unwrap();
                assert_eq!(getter.call::<f64>((copy.clone(),"Gain_3_1")).unwrap(),0.875);
            }
            let entries:Table=k.raw_get(collection).unwrap();entries.raw_set(1,copy).unwrap();
            let v=h.observe_live_graph(&q).value;assert_eq!(v["complete"],false);assert_eq!(v["reason"],"collection_identity_mismatch");
        }
    }
    #[test]
    fn parent_and_synthetic_part_copies_cannot_alias_canonical_tables(){
        let(h,q)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);
        m.raw_set("parent",shallow(&h,&k)).unwrap();assert_eq!(h.observe_live_graph(&q).value["reason"],"inconsistent_parent");
        let(h,q)=fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();let part:Table=p.raw_get("part").unwrap();
        p.raw_set("parent",shallow(&h,&part)).unwrap();assert_eq!(h.observe_live_graph(&q).value["reason"],"synthetic_parent_alias");
    }
    #[test]
    fn overlay_index_function_is_never_invoked_by_observer(){
        let(h,q)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);
        let calls=std::rc::Rc::new(std::cell::Cell::new(0));let seen=calls.clone();
        let fallback=h.lua.create_function(move|_,(_t,_key):(Table,Value)|{seen.set(seen.get()+1);Ok(0.625)}).unwrap();
        let mt=h.lua.create_table().unwrap();mt.raw_set("__index",fallback).unwrap();
        let overlay=h.lua.create_table().unwrap();overlay.set_metatable(Some(mt)).unwrap();m.raw_set("__set",overlay).unwrap();
        let getter:mlua::Function=m.get("getParameter").unwrap();assert_eq!(getter.call::<f64>((m.clone(),"Gain_3_1")).unwrap(),0.625);
        // The ordinary getter indexes once for its nil check and once to return.
        assert_eq!(calls.get(),2);
        calls.set(0);let v=h.observe_live_graph(&q).value;
        assert_eq!(v["reason"],"overlay_metatable_unsupported");assert_eq!(v["complete"],false);assert_eq!(calls.get(),0);
    }
    #[test]
    fn overlay_index_table_does_not_silently_fall_back_to_saved_cell(){
        let(h,q)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);
        let fallback=h.lua.create_table().unwrap();fallback.raw_set("Gain_3_1",0.75).unwrap();
        let mt=h.lua.create_table().unwrap();mt.raw_set("__index",fallback).unwrap();let overlay=h.lua.create_table().unwrap();overlay.set_metatable(Some(mt)).unwrap();m.raw_set("__set",overlay).unwrap();
        let getter:mlua::Function=m.get("getParameter").unwrap();assert_eq!(getter.call::<f64>((m.clone(),"Gain_3_1")).unwrap(),0.75);
        assert_eq!(h.observe_live_graph(&q).value["reason"],"overlay_metatable_unsupported");
    }
    #[test]
    fn prefader_changes_are_retained_but_route_inventory_stays_incomplete(){
        for pre in [false,true]{let(h,q)=routed_fixture(pre);let v=h.observe_live_graph(&q).value;
            assert_eq!(v["status"],"observed");assert_eq!(v["inventory_complete"],true);assert_eq!(v["complete"],false);
            assert_eq!(v["routes"][0]["fields"]["PreFader"]["current"]["type"],"boolean");
            assert_eq!(v["routes"][0]["fields"]["PreFader"]["current"]["value"],pre);
            assert_eq!(v["route_topology"]["reason"],"compiled_route_linkage_unavailable");
        }
    }
    #[test]
    fn duplicate_collection_entries_and_cross_collection_membership_refuse(){
        let(h,q)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);let repeated=h.lua.create_table().unwrap();
        for n in 1..=8192{repeated.raw_set(n,m.clone()).unwrap();}k.raw_set("connections",repeated).unwrap();
        let v=h.observe_live_graph(&q).value;assert_eq!(v["reason"],"duplicate_collection_member");
        assert!(v["work"]["field_steps"].as_u64().unwrap()>=8192);assert!(v["work"]["field_steps"].as_u64().unwrap()<=STEPS as u64);
        let(h,q)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);let alias=h.lua.create_table().unwrap();alias.raw_set(1,m).unwrap();k.raw_set("connections",alias).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"duplicate_collection_membership");
    }
    #[test]
    fn collection_work_is_shared_and_exhaustion_precedes_entry_extraction(){
        let(h,_)=fixture();let k=keygroup(&h);let m=list(&k,"inserts").unwrap().remove(0);
        let container=h.lua.create_table().unwrap();let entries=h.lua.create_table().unwrap();entries.raw_set(1,m).unwrap();container.raw_set("connections",entries.clone()).unwrap();
        let mut b=Budget{steps:STEPS-9,..Default::default()};assert_eq!(super::list(&container,"connections",&mut b).unwrap().len(),1);
        assert!(matches!(super::list(&container,"connections",&mut b),Err("field_budget")));assert!(b.exhausted);assert!(b.steps<=STEPS);
        entries.raw_set(1,false).unwrap();entries.raw_set(2,false).unwrap();let mut b=Budget{steps:STEPS-3,..Default::default()};
        assert!(matches!(super::list(&container,"connections",&mut b),Err("field_budget")));assert!(b.exhausted);assert!(b.steps<=STEPS);
    }

    #[test]
    fn selected_layer_and_keygroup_collection_copies_refuse_without_getter_calls(){
        for collection in ["layers","keygroups"]{
            let(h,q)=fixture();let program:Table=h.lua.globals().raw_get("Program").unwrap();
            let layer=list(&program,"layers").unwrap().remove(0);
            let parent=if collection=="layers"{program}else{layer};
            let original=list(&parent,collection).unwrap().remove(0);let copy=shallow(&h,&original);
            let parameter=if collection=="layers"{"Bypass"}else{"FXPostGain"};
            let overlay=h.lua.create_table().unwrap();overlay.raw_set(parameter,true).unwrap();copy.raw_set("__set",overlay).unwrap();
            let get_copy:mlua::Function=copy.get("getParameter").unwrap();
            let get_original:mlua::Function=original.get("getParameter").unwrap();
            assert!(get_copy.call::<bool>((copy.clone(),parameter)).unwrap());
            assert!(!get_original.call::<bool>((original.clone(),parameter)).unwrap());
            let calls=std::rc::Rc::new(std::cell::Cell::new(0));let seen=calls.clone();
            let hook=h.lua.create_function(move|_,_:mlua::MultiValue|{seen.set(seen.get()+1);Ok(Value::Nil)}).unwrap();
            original.raw_set("getParameter",hook.clone()).unwrap();copy.raw_set("getParameter",hook).unwrap();
            let entries:Table=parent.raw_get(collection).unwrap();entries.raw_set(1,copy).unwrap();
            let v=h.observe_live_graph(&q).value;
            assert_eq!(v["reason"],"collection_identity_mismatch");assert_eq!(v["complete"],false);assert_eq!(calls.get(),0);
        }
    }
    fn from_ancestry_xml(xml:&str)->(ScriptHost,LiveGraphQuery){
        let doc=roxmltree::Document::parse(xml).unwrap();let node=|kind:&str|doc.descendants().find(|n|n.has_tag_name(kind)).unwrap().id().get_usize()as u32;
        let q=LiveGraphQuery{keygroup_node:node("Keygroup"),matrix_node:node("GainMatrix"),
            player_nodes:doc.descendants().filter(|n|n.has_tag_name("SamplePlayer")).map(|n|n.id().get_usize()as u32).collect()};
        (ScriptHost::new(xml,Empty,super::super::Config::default()).unwrap(),q)
    }
    #[test]
    fn nested_layer_membership_is_checked_and_unknown_wrappers_refuse(){
        let body="<Keygroup><Inserts><GainMatrix/></Inserts><Oscillators><SamplePlayer/><SamplePlayer Bypass='1'/><SamplePlayer Bypass='1'/></Oscillators></Keygroup>";
        let xml=format!("<Program><Layers><Layer><Layers><Layer><Keygroups>{body}</Keygroups></Layer></Layers></Layer></Layers></Program>");
        let(h,q)=from_ancestry_xml(&xml);assert_eq!(h.observe_live_graph(&q).value["status"],"observed");
        let program:Table=h.lua.globals().raw_get("Program").unwrap();let outer=list(&program,"layers").unwrap().remove(0);
        let inner=list(&outer,"layers").unwrap().remove(0);let copy=shallow(&h,&inner);
        let overlay=h.lua.create_table().unwrap();overlay.raw_set("Bypass",true).unwrap();copy.raw_set("__set",overlay).unwrap();
        let entries:Table=outer.raw_get("layers").unwrap();entries.raw_set(1,copy).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"collection_identity_mismatch");
        let xml=format!("<Program><Layers><Layer><Keygroups><Container><Keygroups>{body}</Keygroups></Container></Keygroups></Layer></Layers></Program>");
        let(h,q)=from_ancestry_xml(&xml);assert_eq!(h.observe_live_graph(&q).value["reason"],"unsupported_selected_ancestry");
    }
    #[test]
    fn selected_ancestor_absent_from_public_collection_is_not_inferred_from_children(){
        let(h,q)=fixture();let program:Table=h.lua.globals().raw_get("Program").unwrap();let layer=list(&program,"layers").unwrap().remove(0);
        let groups:Table=layer.raw_get("keygroups").unwrap();groups.raw_set(1,Value::Nil).unwrap();
        let v=h.observe_live_graph(&q).value;assert_eq!(v["reason"],"selected_ancestry_membership_missing");assert_eq!(v["complete"],false);
    }
    #[test]
    fn public_invalid_player_vector_has_fixed_refusal_without_query_reflection(){
        let(h,valid)=fixture();h.lua.globals().raw_set("Program",false).unwrap();
        let mut encoded=None;
        for len in [4,65536,1000000]{
            let q=LiveGraphQuery{keygroup_node:valid.keygroup_node,matrix_node:valid.matrix_node,player_nodes:vec![9;len]};
            let v=h.observe_live_graph(&q).value;
            assert_eq!(v["reason"],"invalid_query");assert_eq!(v["complete"],false);assert!(v.get("query").is_none());
            assert_eq!(v["work"]["nodes"],0);assert_eq!(v["work"]["field_steps"],0);
            let bytes=serde_json::to_vec(&v).unwrap();assert!(bytes.len()<512);
            if let Some(expected)=&encoded{assert_eq!(&bytes,expected)}else{encoded=Some(bytes)}
        }
    }

    fn selection_fixture(inserts:&str,routes:&str,program_extra:&str)->(ScriptHost,LiveGraphQuery){
        let xml=format!("<Program><Layers><Layer><Keygroups><Keygroup FXPostGain='1'><Inserts>{inserts}</Inserts><Oscillators><SamplePlayer/><SamplePlayer Bypass='1'/><SamplePlayer Bypass='1'/></Oscillators>{routes}</Keygroup></Keygroups></Layer></Layers>{program_extra}</Program>");
        from_ancestry_xml(&xml)
    }
    fn large_descendant_fixture()->(ScriptHost,LiveGraphQuery){
        // The former recursive selection has 8 + 9*(1+15) = 152 nodes and
        // 10 matrices. Immediate aux insert inventory remains below32.
        let branch=format!("<GainMatrix><Connections>{}</Connections></GainMatrix>","<SignalConnection/>".repeat(15));
        let aux=format!("<Auxs><AuxEffect><Inserts>{}</Inserts></AuxEffect></Auxs>",branch.repeat(9));
        selection_fixture("<GainMatrix Gain_3_1='-0.25' Gain_12_7='0.4'/>","",&aux)
    }
    #[test]
    fn required_owner_facts_survive_large_unexpanded_descendants_with_exact_partial_counts(){
        let(h,q)=large_descendant_fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();
        let aux=list(&p,"auxs").unwrap().remove(0);let matrices=list(&aux,"inserts").unwrap();
        assert_eq!(matrices.len(),9);assert!(8+matrices.len()*16>128);assert!(1+matrices.len()>8);
        let calls=std::rc::Rc::new(std::cell::Cell::new(0));let seen=calls.clone();
        let hook=h.lua.create_function(move|_,_:mlua::MultiValue|{seen.set(seen.get()+1);Ok(Value::Nil)}).unwrap();
        let mt=h.lua.create_table().unwrap();mt.raw_set("__index",hook.clone()).unwrap();
        for m in matrices{
            let overlay=h.lua.create_table().unwrap();overlay.set_metatable(Some(mt.clone())).unwrap();
            m.raw_set("__set",overlay).unwrap();m.raw_set("getParameter",hook.clone()).unwrap();
        }
        let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");
        assert_eq!(v["selection"]["policy"],"required_owner_facts_v1");
        assert_eq!(v["selection"]["required_facts_complete"],true);
        assert_eq!(v["selection"]["selected_model_nodes"],8);
        assert_eq!(v["selection"]["unprojected_model_nodes"],144);
        assert_eq!(v["selection"]["unexpanded_child_edges"],9);
        assert_eq!(v["work"]["nodes"],152);assert_eq!(v["inventory_complete"],false);assert_eq!(v["complete"],false);
        assert_eq!(v["route_topology"]["complete"],false);
        assert_eq!(v["route_topology"]["reason"],"compiled_route_linkage_unavailable");
        assert_eq!(v["matrices"].as_array().unwrap().len(),1);
        assert_eq!(v["matrices"][0]["node"],q.matrix_node);
        assert_eq!(v["matrices"][0]["cells"][0][2]["current"]["value"],-0.25);
        assert_eq!(v["matrices"][0]["cells"][6][11]["current"]["value"],0.4);
        let k=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.keygroup_node).unwrap();
        assert_eq!(k["fields"]["FXPostGain"]["current"]["value"],true);
        assert_eq!(k["fields"]["Bypass"]["current"]["value"],false);
        assert_eq!(v["oscillator_order"].as_array().unwrap().len(),3);
        assert_eq!(v["ordered_inserts"].as_array().unwrap().len(),10);assert_eq!(calls.get(),0);
    }
    #[test]
    fn required_owner_facts_keep_selected_matrix_insert_and_route_refusal_caps(){
        let inserts=format!("<GainMatrix/>{}","<Gain/>".repeat(122));
        let(h,q)=selection_fixture(&inserts,"","");
        assert_eq!(h.observe_live_graph(&q).value["reason"],"selected_node_budget");
        let(h,q)=selection_fixture(&"<GainMatrix/>".repeat(9),"","");
        assert_eq!(h.observe_live_graph(&q).value["reason"],"matrix_budget");
        let inserts=format!("<GainMatrix/>{}","<Gain/>".repeat(32));
        let(h,q)=selection_fixture(&inserts,"","");
        assert_eq!(h.observe_live_graph(&q).value["reason"],"insert_budget");
        let routes=format!("<BusRouters>{}</BusRouters>","<BusRouter PreFader='1'/>".repeat(17));
        let(h,q)=selection_fixture("<GainMatrix/>",&routes,"");
        assert_eq!(h.observe_live_graph(&q).value["reason"],"route_budget");
    }
    #[test]
    fn required_owner_facts_validate_direct_aux_and_unexpanded_member_identity(){
        let(h,q)=large_descendant_fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();
        let entries:Table=p.raw_get("auxs").unwrap();let aux=list(&p,"auxs").unwrap().remove(0);
        let copy=shallow(&h,&aux);let overlay=h.lua.create_table().unwrap();overlay.raw_set("Bypass",true).unwrap();copy.raw_set("__set",overlay).unwrap();
        entries.raw_set(1,copy).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"collection_identity_mismatch");
        let(h,q)=large_descendant_fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();
        let aux=list(&p,"auxs").unwrap().remove(0);let m=list(&aux,"inserts").unwrap().remove(0);
        let entries:Table=aux.raw_get("inserts").unwrap();entries.raw_set(1,shallow(&h,&m)).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"collection_identity_mismatch");
    }
    #[test]
    fn unexpanded_descendants_still_obey_whole_graph_parent_and_work_guards(){
        let(h,q)=large_descendant_fixture();let p:Table=h.lua.globals().raw_get("Program").unwrap();
        let aux=list(&p,"auxs").unwrap().remove(0);let m=list(&aux,"inserts").unwrap().remove(0);
        let c=list(&m,"connections").unwrap().remove(0);c.raw_set("parent",aux).unwrap();
        assert_eq!(h.observe_live_graph(&q).value["reason"],"inconsistent_parent");
        let(h,q)=large_descendant_fixture();let mut b=Budget{steps:STEPS-32,..Default::default()};
        assert!(matches!(h.live_graph_inner(&q,&mut b),Err("field_budget")));assert!(b.exhausted);
    }

    fn nested_route_fixture(collection:&str)->(ScriptHost,LiveGraphQuery){
        let child=match collection{
            "auxs"=>"<Auxs><AuxEffect PreFader='0' Gain='0.2'/></Auxs>",
            "sends"=>"<BusRouters><BusRouter PreFader='0' Gain='0.2'/></BusRouters>",
            _=>panic!("unsupported authored fixture collection"),
        };
        let extra=format!("<Auxs><AuxEffect>{child}</AuxEffect></Auxs>");
        selection_fixture("<GainMatrix Gain_3_1='-0.25' Gain_12_7='0.4'/>",
            "<BusRouters><BusRouter PreFader='1' Gain='0.4'/></BusRouters>",&extra)
    }
    #[test]
    fn omitted_nested_aux_and_send_have_no_scalar_fields_or_overlay_callbacks(){
        for collection in ["auxs","sends"]{for unsupported in [false,true]{
            let(h,q)=nested_route_fixture(collection);let p:Table=h.lua.globals().raw_get("Program").unwrap();
            let aux=list(&p,"auxs").unwrap().remove(0);let child=list(&aux,collection).unwrap().remove(0);
            let calls=std::rc::Rc::new(std::cell::Cell::new(0));let seen=calls.clone();
            let hook=h.lua.create_function(move|_,_:mlua::MultiValue|{seen.set(seen.get()+1);Ok(Value::Nil)}).unwrap();
            let mt=h.lua.create_table().unwrap();mt.raw_set("__index",hook.clone()).unwrap();
            let overlay=h.lua.create_table().unwrap();overlay.raw_set("PreFader",false).unwrap();overlay.raw_set("Gain",0.875).unwrap();
            if unsupported{overlay.set_metatable(Some(mt.clone())).unwrap();}
            child.raw_set("__set",overlay).unwrap();child.raw_set("getParameter",hook).unwrap();
            let v=h.observe_live_graph(&q).value;assert_eq!(v["status"],"observed");
            assert_eq!(v["selection"]["required_facts_complete"],true);
            assert_eq!(v["selection"]["selected_model_nodes"],9);
            assert_eq!(v["selection"]["unprojected_model_nodes"],1);
            assert_eq!(v["selection"]["unexpanded_child_edges"],1);
            assert_eq!(v["inventory_complete"],false);assert_eq!(v["complete"],false);
            let child_id:usize=child.raw_get("__id").unwrap();let aux_id:usize=aux.raw_get("__id").unwrap();
            let(child_node,aux_node)={let nodes=h.shared.nodes.borrow();(nodes[child_id].0.unwrap(),nodes[aux_id].0.unwrap())};
            let route=v["routes"].as_array().unwrap().iter().find(|r|r["node"]==child_node).unwrap();
            assert_eq!(route["owner_node"],aux_node);assert_eq!(route["collection"],collection);assert_eq!(route["ordinal"],0);
            assert_eq!(route["kind"],if collection=="auxs"{"AuxEffect"}else{"BusRouter"});
            assert_eq!(route["compiled_linkage"],"unavailable");
            assert_eq!(route["scalar_projection"],"omitted_unselected_member");assert!(route.get("fields").is_none());
            assert!(!v["nodes"].as_array().unwrap().iter().any(|n|n["node"]==child_node));
            let direct=v["routes"].as_array().unwrap().iter().find(|r|r["owner_node"]==q.keygroup_node).unwrap();
            assert_eq!(direct["fields"]["PreFader"]["current"]["type"],"boolean");
            assert_eq!(direct["fields"]["PreFader"]["current"]["value"],true);
            assert_eq!(direct["scalar_projection"],"selected_current_fields");
            assert_eq!(v["matrices"][0]["node"],q.matrix_node);
            assert_eq!(v["matrices"][0]["cells"][0][2]["current"]["value"],-0.25);
            assert_eq!(v["matrices"][0]["cells"][6][11]["current"]["value"],0.4);
            let k=v["nodes"].as_array().unwrap().iter().find(|n|n["node"]==q.keygroup_node).unwrap();
            assert_eq!(k["fields"]["FXPostGain"]["current"]["value"],true);
            assert_eq!(v["oscillator_order"].as_array().unwrap().len(),3);assert_eq!(calls.get(),0);
            // Selected route overlays still refuse; omission is not a weakening
            // of the scalar helper or the selected-node metatable contract.
            let k=keygroup(&h);let selected=list(&k,"sends").unwrap().remove(0);
            let overlay=h.lua.create_table().unwrap();overlay.set_metatable(Some(mt)).unwrap();selected.raw_set("__set",overlay).unwrap();
            assert_eq!(h.observe_live_graph(&q).value["reason"],"overlay_metatable_unsupported");assert_eq!(calls.get(),0);
        }}
    }
    #[test]
    fn omitted_nested_aux_and_send_canonical_copies_still_refuse(){
        for collection in ["auxs","sends"]{
            let(h,q)=nested_route_fixture(collection);let p:Table=h.lua.globals().raw_get("Program").unwrap();
            let aux=list(&p,"auxs").unwrap().remove(0);let child=list(&aux,collection).unwrap().remove(0);
            let copy=shallow(&h,&child);let calls=std::rc::Rc::new(std::cell::Cell::new(0));let seen=calls.clone();
            let hook=h.lua.create_function(move|_,_:mlua::MultiValue|{seen.set(seen.get()+1);Ok(Value::Nil)}).unwrap();
            let mt=h.lua.create_table().unwrap();mt.raw_set("__index",hook).unwrap();
            let overlay=h.lua.create_table().unwrap();overlay.set_metatable(Some(mt)).unwrap();copy.raw_set("__set",overlay).unwrap();
            let entries:Table=aux.raw_get(collection).unwrap();entries.raw_set(1,copy).unwrap();
            let v=h.observe_live_graph(&q).value;assert_eq!(v["reason"],"collection_identity_mismatch");
            assert_eq!(v["complete"],false);assert_eq!(calls.get(),0);
        }
    }
    #[test]
    fn omitted_nested_route_references_still_consume_the_sixteen_route_cap(){
        for children in [15,16]{
            let extra=format!("<Auxs><AuxEffect><BusRouters>{}</BusRouters></AuxEffect></Auxs>","<BusRouter/>".repeat(children));
            let(h,q)=selection_fixture("<GainMatrix/>","",&extra);
            let v=h.observe_live_graph(&q).value;
            if children==15{
                assert_eq!(v["status"],"observed");assert_eq!(v["routes"].as_array().unwrap().len(),16);
                assert_eq!(v["routes"].as_array().unwrap().iter().filter(|r|r["scalar_projection"]=="omitted_unselected_member").count(),15);
                assert_eq!(v["selection"]["required_facts_complete"],true);assert_eq!(v["complete"],false);
            }else{assert_eq!(v["reason"],"route_budget");}
        }
    }

}
