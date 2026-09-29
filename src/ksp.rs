//! Bounded KSP initialization for inspecting authored performance interfaces.
//! No note callbacks or audio-side execution. Unknown syntax fails explicitly.
use anyhow::{bail, ensure, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone,Debug,Serialize)]
#[serde(untagged)]
pub enum Value { Int(i32), Real(f64), Text(String), Array(Vec<Value>) }
impl Value {
    fn bytes(&self)->usize {match self {Self::Int(_)=>4,Self::Real(_)=>8,Self::Text(s)=>s.len(),Self::Array(a)=>a.iter().map(Value::bytes).sum()}}
    fn real(&self)->Result<f64>{if let Self::Real(n)=self {Ok(*n)}else{bail!("Expected real number")}}
    fn int(&self)->Result<i32>{if let Self::Int(n)=self {Ok(*n)}else{bail!("Expected integer")}}
    fn text(&self)->String{match self{Self::Int(n)=>n.to_string(),Self::Real(n)=>n.to_string(),Self::Text(s)=>s.clone(),Self::Array(_)=>"[array]".into()}}
}
/// Instrument-local services shared by initialization slots, never process-global.
#[derive(Clone,Debug,Default,Serialize)]
pub struct HostState {
    integers:BTreeMap<String,Vec<i32>>, strings:BTreeMap<String,String>,
    pub keyboard:BTreeMap<u8,KeyState>, pub script_pressed:bool,
}
#[derive(Clone,Debug,Default,Serialize)]
pub struct KeyState {pub name:String,pub color:Option<Value>,pub kind:Option<Value>,pub pressed:bool}
fn key_id(a:&[Expr])->Result<&str>{
    let Some(Expr::Value(Value::Text(key)))=a.first() else{bail!("PGS requires a literal key identifier")};
    ensure!(!key.is_empty() && key.len()<=64 && key.starts_with(|c:char|c.is_ascii_alphabetic()) && key.chars().all(|c|c.is_ascii_alphanumeric() || c=='_'),"Invalid PGS key identifier");Ok(key)
}
fn midi_note(n:i32)->Result<u8>{ensure!((0..128).contains(&n),"MIDI note must be 0..127");Ok(n as u8)}
#[derive(Clone,Debug,Serialize)]
pub struct Control {
    pub variable:String, pub kind:String, pub properties:BTreeMap<String,Value>, pub menu:Vec<(String,i32)>,
}
#[derive(Clone,Debug,Serialize)]
pub struct Interface {
    pub performance:bool, pub width:i32, pub height:i32, pub title:String, pub wallpaper:String, pub controls:Vec<Control>,
    pub diagnostics:BTreeSet<String>, pub listeners:BTreeMap<String,i32>,
}
impl Default for Interface {fn default()->Self{Self{performance:false,width:632,height:350,title:String::new(),wallpaper:String::new(),controls:Vec::new(),diagnostics:BTreeSet::new(),listeners:BTreeMap::new()}}}
#[derive(Clone,Debug)]
enum Expr { Value(Value), Var(String,Option<Box<Expr>>), Call(String,Vec<Expr>), Binary(String,Box<Expr>,Box<Expr>), Unary(String,Box<Expr>) }
#[derive(Clone,Debug)]
enum Stmt { Declare(String,String,Option<Expr>,Vec<Expr>), Assign(Expr,Expr), Command(String,Vec<Expr>), If(Expr,Vec<Stmt>,Vec<Stmt>), While(Expr,Vec<Stmt>), Select(Expr,Vec<(Expr,Expr,Vec<Stmt>)>), Function(String) }

fn tokens(s:&str)->Result<Vec<String>> {
    let c:Vec<char>=s.chars().collect();let mut out=Vec::new();let mut i=0;
    while i<c.len() {
        if c[i].is_whitespace(){i+=1;continue;}
        let start=i;
        if c[i]=='"' {i+=1;while i<c.len() && c[i]!='"'{i+=1;}ensure!(i<c.len(),"Unterminated string");i+=1;}
        else if c[i]=='.' && [".and.",".or.",".not."].iter().any(|op|c[i..].iter().take(op.len()).copied().eq(op.chars())) {let len=if c.get(i+1)==Some(&'o'){4}else{5};i+=len;}
        else if c[i].is_ascii_digit() && let Some(dot)=c[i..].iter().position(|c|!c.is_ascii_digit()) && c[i+dot]=='.' && c.get(i+dot+1).is_some_and(|c|c.is_ascii_digit()) {
            while i<c.len() && c[i].is_ascii_digit(){i+=1;}i+=1;while i<c.len() && c[i].is_ascii_digit(){i+=1;}
            if i<c.len() && matches!(c[i],'e'|'E'){i+=1;if i<c.len() && matches!(c[i],'+'|'-'){i+=1;}while i<c.len() && c[i].is_ascii_digit(){i+=1;}}
        }
        else if c[i].is_alphanumeric() || "_$%@!~?".contains(c[i]) {i+=1;while i<c.len() && (c[i].is_alphanumeric() || c[i]=='_'){i+=1;}}
        else {i+=1;if i<c.len() && matches!((c[start],c[i]),(':','=')|('<','=')|('>','=')|('#','=')|('!','=')){i+=1;}}
        ensure!(i-start<=65536,"KSP token length limit");out.push(c[start..i].iter().collect());ensure!(out.len()<=500_000,"KSP statement token budget exhausted");
    }
    Ok(out)
}
struct Parser { t:Vec<String>, p:usize }
impl Parser {
    fn new(s:&str)->Result<Self>{Ok(Self{t:tokens(s)?,p:0})}
    fn peek(&self)->&str{self.t.get(self.p).map(String::as_str).unwrap_or("")}
    fn pop(&mut self)->Result<String>{let t=self.t.get(self.p).context("Unexpected end of expression")?.clone();self.p+=1;Ok(t)}
    fn eat(&mut self,t:&str)->bool{if self.peek()==t{self.p+=1;true}else{false}}
    fn need(&mut self,t:&str)->Result<()>{ensure!(self.eat(t),"Expected {t}, got {}",self.peek());Ok(())}
    fn expr(&mut self,min:u8,depth:usize)->Result<Expr>{
        ensure!(depth<64,"Expression nesting limit");let start=self.p;let t=self.pop()?;
        let mut lhs=if t=="("{let e=self.expr(0,depth+1)?;self.need(")")?;e}
        else if ["-","+","not",".not."].contains(&t.as_str()){Expr::Unary(t,Box::new(self.expr(8,depth+1)?))}
        else if t.starts_with('"'){Expr::Value(Value::Text(t[1..t.len()-1].into()))}
        else if t.starts_with(|c:char|c.is_ascii_digit()) && t.ends_with(['h','H']){Expr::Value(Value::Int(u32::from_str_radix(&t[..t.len()-1],16).context("Invalid hexadecimal integer")? as i32))}
        else if let Ok(n)=t.parse::<i32>(){Expr::Value(Value::Int(n))}
        else if t.contains('.') && t.starts_with(|c:char|c.is_ascii_digit()){let n=t.parse::<f64>().context("Invalid real literal")?;ensure!(n.is_finite(),"Nonfinite real literal");Expr::Value(Value::Real(n))}
        else if self.eat("("){let a=self.args(")",depth+1)?;Expr::Call(t,a)}
        else if t.starts_with(['$','%','@','!','~','?']){let index=if self.eat("["){let e=self.expr(0,depth+1)?;self.need("]")?;Some(Box::new(e))}else{None};Expr::Var(t,index)}
        else {Expr::Value(Value::Text(t))};
        loop {
            ensure!(self.p-start<=512,"KSP expression token budget exhausted");
            let op=self.peek();let bp=match op{"or"|".or."=>1,"and"|".and."=>2,"="|"#"|"<"|">"|"<="|">="|"!="=>3,"&"=>4,"+"|"-"=>5,"*"|"/"|"mod"=>6,_=>break};if bp<min{break;}
            let op=self.pop()?;let rhs=self.expr(bp+1,depth+1)?;lhs=Expr::Binary(op,Box::new(lhs),Box::new(rhs));
        }
        Ok(lhs)
    }
    fn args(&mut self,end:&str,depth:usize)->Result<Vec<Expr>>{let mut a=Vec::new();if self.eat(end){return Ok(a);}loop{a.push(self.expr(0,depth)?);if self.eat(end){break;}self.need(",")?;}Ok(a)}
    fn finish(&self)->Result<()>{ensure!(self.p==self.t.len(),"Unexpected token {}",self.peek());Ok(())}
}
fn expression(s:&str)->Result<Expr>{let mut p=Parser::new(s)?;let e=p.expr(0,0)?;p.finish()?;Ok(e)}
fn statement(s:&str)->Result<Stmt>{
    let mut p=Parser::new(s)?;
    let result=if p.eat("declare") {
        let mut kind=String::new();if p.peek()=="const" || p.peek()=="polyphonic"{kind=p.pop()?;}
        if p.peek().starts_with("ui_"){kind=p.pop()?;}
        let name=p.pop()?;ensure!(name.starts_with(['$','%','@','!','~','?']),"Invalid variable {name}");
        let array=if p.eat("["){let e=p.expr(0,0)?;p.need("]")?;Some(e)}else{None};
        let mut args=if p.eat("("){p.args(")",0)?}else{Vec::new()};
        if p.eat(":="){ensure!(args.is_empty(),"Unexpected initializer");if array.is_some() && p.eat("("){args=p.args(")",0)?;}else{args.push(p.expr(0,0)?);}}
        Stmt::Declare(kind,name,array,args)
    }else if p.eat("call"){Stmt::Function(p.pop()?)}
    else if p.peek().starts_with(['$','%','@','!','~','?']){let left=p.expr(0,0)?;p.need(":=")?;Stmt::Assign(left,p.expr(0,0)?)}
    else {let name=p.pop()?;let args=if p.eat("("){p.args(")",0)?}else{Vec::new()};Stmt::Command(name,args)};
    p.finish()?;Ok(result)
}
fn keyword<'a>(s:&'a str,name:&str)->Option<&'a str>{
    s.strip_prefix(name).filter(|tail|tail.is_empty() || tail.starts_with(|c:char|c.is_whitespace() || c=='(')).map(str::trim_start)
}
fn block(lines:&[(usize,String)],at:&mut usize,depth:usize)->Result<Vec<Stmt>> {
    ensure!(depth<64,"Statement nesting limit");let mut out=Vec::new();
    while *at<lines.len() {
        let (line,s)=&lines[*at];if s.starts_with("end ") || s=="else" || s.starts_with("case "){break;}*at+=1;
        let stmt=if let Some(e)=keyword(s,"while"){let e=expression(e)?;let body=block(lines,at,depth+1)?;ensure!(lines.get(*at).is_some_and(|(_,s)|s=="end while"),"Unclosed while");*at+=1;Stmt::While(e,body)}
        else if let Some(e)=keyword(s,"if"){let e=expression(e)?;let yes=block(lines,at,depth+1)?;let no=if lines.get(*at).is_some_and(|(_,s)|s=="else"){*at+=1;block(lines,at,depth+1)?}else{Vec::new()};ensure!(lines.get(*at).is_some_and(|(_,s)|s=="end if"),"Unclosed if");*at+=1;Stmt::If(e,yes,no)}
        else if let Some(e)=keyword(s,"select") {
            let value=expression(e)?;let mut cases=Vec::new();
            while let Some((line,case))=lines.get(*at).filter(|(_,s)|s.starts_with("case ")) {
                let mut parser=Parser::new(&case[5..])?;let low=parser.expr(0,0)?;let high=if parser.eat("to"){parser.expr(0,0)?}else{low.clone()};parser.finish().with_context(||format!("KSP case at line {line}"))?;
                *at+=1;cases.push((low,high,block(lines,at,depth+1)?));
            }
            ensure!(lines.get(*at).is_some_and(|(_,s)|s=="end select"),"Unclosed select");*at+=1;Stmt::Select(value,cases)
        }
        else {statement(s).with_context(||format!("KSP line {line}"))?};out.push(stmt);
    }
    Ok(out)
}
fn lines(source:&str)->Result<Vec<(usize,String)>> {
    ensure!(source.len()<=16*1024*1024,"KSP source exceeds 16 MiB");
    let mut out=Vec::new();let mut comment=false;let mut quoted=false;let mut line=String::new();let mut n=1;
    for c in source.chars() {
        if c=='\n'{if !quoted && line.trim_end().ends_with("..."){line.truncate(line.trim_end().len()-3);line.push(' ');n+=1;continue;}if !line.trim().is_empty(){out.push((n,line.trim().to_owned()));}line.clear();n+=1;ensure!(out.len()<300_000,"KSP line limit");continue;}
        if !quoted && c=='{'{comment=true;if !line.ends_with(' '){line.push(' ');}continue;}if comment {if c=='}'{comment=false;}continue;}
        if c=='"'{quoted=!quoted;}if !quoted && c.is_whitespace(){if !line.ends_with(' '){line.push(' ');}}else{line.push(c);}
    }
    ensure!(!comment && !quoted,"Unterminated KSP comment/string");if !line.trim().is_empty(){out.push((n,line.trim().to_owned()));}
    Ok(out)
}
struct Runtime<'a> {host:&'a mut HostState,vars:BTreeMap<String,Value>,ids:BTreeMap<String,usize>,controls_by_id:Vec<Option<usize>>,functions:BTreeMap<String,std::sync::Arc<Vec<Stmt>>>,sources:BTreeMap<String,&'a [(usize,String)]>,parse_budget:usize,ui:Interface,steps:usize,elements:usize,bytes:usize}
impl Runtime<'_> {
    fn tick(&mut self)->Result<()>{ensure!(self.steps>0,"KSP execution budget exhausted");self.steps-=1;Ok(())}
    fn eval(&mut self,e:&Expr)->Result<Value>{
        self.tick()?;
        let value=match e {
            Expr::Value(v)=>v.clone(),
            Expr::Var(n,index)=>{
                if let Some(index)=index {let i=usize::try_from(self.eval(index)?.int()?)?;let Some(Value::Array(a))=self.vars.get(n) else{bail!("Unknown array {n}")};a.get(i).context("KSP array index out of bounds")?.clone()}
                else {match self.vars.get(n) {Some(Value::Array(_))=>bail!("Whole-array expressions are unsupported"),Some(v)=>v.clone(),None=>{ensure!(n.chars().nth(1).is_some_and(|c|c.is_ascii_uppercase()),"Undeclared variable {n}");Value::Text(n.clone())}}}
            },
            Expr::Unary(op,e)=>{let value=self.eval(e)?;if let Value::Real(n)=value {Value::Real(match op.as_str(){"-"=>-n,"+"=>n,_=>bail!("Invalid real unary operator")})}else{let n=value.int()?;Value::Int(match op.as_str(){"-"=>n.wrapping_neg(),"not"=>i32::from(n==0),".not."=>!n,_=>n})}},
            Expr::Binary(op,a,b)=>{let a=self.eval(a)?;let b=self.eval(b)?;if op=="&"{let a=a.text();let b=b.text();ensure!(a.len()+b.len()<=65536,"KSP string length limit");Value::Text(a+&b)}else if matches!(a,Value::Real(_)) || matches!(b,Value::Real(_)) {
                let a=a.real()?;let b=b.real()?;match op.as_str(){
                    "="=>Value::Int(i32::from(a==b)),"#"|"!="=>Value::Int(i32::from(a!=b)),"<"=>Value::Int(i32::from(a<b)),">"=>Value::Int(i32::from(a>b)),"<="=>Value::Int(i32::from(a<=b)),">="=>Value::Int(i32::from(a>=b)),
                    _=>Value::Real(match op.as_str(){"+"=>a+b,"-"=>a-b,"*"=>a*b,"/"=>{ensure!(b!=0.0,"Division by zero");a/b},"mod"=>{ensure!(b!=0.0,"Modulo by zero");a%b},_=>bail!("Invalid real operator {op}")})}
            }else{let a=a.int()?;let b=b.int()?;Value::Int(match op.as_str(){"+"=>a.wrapping_add(b),"-"=>a.wrapping_sub(b),"*"=>a.wrapping_mul(b),"/"=>{ensure!(b!=0,"Division by zero");a.wrapping_div(b)},"mod"=>{ensure!(b!=0,"Modulo by zero");a.wrapping_rem(b)},"="=>i32::from(a==b),"#"|"!="=>i32::from(a!=b),"<"=>i32::from(a<b),">"=>i32::from(a>b),"<="=>i32::from(a<=b),">="=>i32::from(a>=b),".and."=>a&b,".or."=>a|b,"and"=>i32::from(a!=0 && b!=0),"or"=>i32::from(a!=0 || b!=0),_=>unreachable!()})}},
            Expr::Call(n,a)=>self.call(n,a)?,
        };if let Value::Real(n)=&value{ensure!(n.is_finite(),"Nonfinite real result");}self.bytes=self.bytes.saturating_add(value.bytes());ensure!(self.bytes<=32*1024*1024,"KSP value allocation budget exhausted");Ok(value)
    }
    fn control_index(&self,id:usize)->Result<usize>{
        id.checked_sub(32768).and_then(|i|self.controls_by_id.get(i)).copied().flatten().context("ID does not refer to a UI control")
    }
    fn call(&mut self,n:&str,a:&[Expr])->Result<Value>{
        if n.starts_with("pgs_") {
            let key=key_id(a)?;
            return Ok(match n {
                "pgs_key_exists"|"pgs_str_key_exists"=>{ensure!(a.len()==1,"PGS exists arity");Value::Int(i32::from(if n=="pgs_key_exists"{self.host.integers.contains_key(key)}else{self.host.strings.contains_key(key)}))},
                "pgs_get_key_val"=>{ensure!(a.len()==2,"PGS get arity");let index=usize::try_from(self.eval(&a[1])?.int()?)?;Value::Int(*self.host.integers.get(key).context("Unknown PGS integer key")?.get(index).context("PGS index out of bounds")?)},
                "pgs_get_str_key_val"=>{ensure!(a.len()==1,"PGS string get arity");Value::Text(self.host.strings.get(key).context("Unknown PGS string key")?.clone())},
                _=>bail!("Unsupported PGS function: {n}"),
            });
        }
        if n=="get_ui_id" {ensure!(a.len()==1,"get_ui_id arity");if let Expr::Var(v,None)=&a[0]{return Ok(Value::Int(*self.ids.get(v).context("Unknown UI variable")? as i32));}bail!("get_ui_id requires a UI variable");}
        if n=="num_elements" {ensure!(a.len()==1,"num_elements arity");let Expr::Var(name,None)=&a[0] else{bail!("num_elements requires array")};let Some(Value::Array(v))=self.vars.get(name) else{bail!("Unknown array {name}")};return Ok(Value::Int(v.len() as i32));}
        let v:Vec<_>=a.iter().map(|e|self.eval(e)).collect::<Result<_>>()?;
        let int=|i:usize|v.get(i).context("Missing function argument")?.int();
        Ok(match n {
            "get_key_name"|"get_key_color"|"get_key_type"|"get_key_triggerstate"=>{
                ensure!(v.len()==1,"Keyboard getter arity");let key=self.host.keyboard.get(&midi_note(int(0)?)?);
                match n {"get_key_name"=>Value::Text(key.map(|k|k.name.clone()).unwrap_or_default()),"get_key_color"=>key.and_then(|k|k.color.clone()).unwrap_or(Value::Text("$KEY_COLOR_NONE".into())),"get_key_type"=>key.and_then(|k|k.kind.clone()).unwrap_or(Value::Text("$NI_KEY_TYPE_NONE".into())),_=>{ensure!(self.host.script_pressed,"get_key_triggerstate requires script pressed support");Value::Int(i32::from(key.is_some_and(|k|k.pressed)))}}
            },
            "get_control_par"|"get_control_par_str"=>{ensure!(v.len()==2,"Control getter arity");let id=self.control_index(usize::try_from(int(0)?)?)?;let prop=v.get(1).context("Missing control property")?.text();let c=self.ui.controls.get(id).context("Unknown control ID")?;if prop=="$CONTROL_PAR_VALUE"{self.vars.get(&c.variable).cloned().context("Missing control value")?}else{c.properties.get(&prop).cloned().unwrap_or(Value::Int(0))}},
            "output_channel_name"=>Value::Text(format!("Out {}",int(0)?+1)),
            "sh_left"|"sh_right"=>{ensure!(v.len()==2,"Bit shift arity");let shift=int(1)?;ensure!((0..32).contains(&shift),"Bit shift must be 0..31");Value::Int(if n=="sh_left"{int(0)?.wrapping_shl(shift as u32)}else{int(0)?>>shift})},
            "real"|"int_to_real"=>{ensure!(v.len()==1,"real arity");Value::Real(int(0)? as f64)},
            "int"|"real_to_int"=>{ensure!(v.len()==1,"int arity");let n=v[0].real()?.trunc();ensure!(n>=i32::MIN as f64 && n<=i32::MAX as f64,"Real to integer overflow");Value::Int(n as i32)},
            "abs"=>{ensure!(v.len()==1,"abs arity");match &v[0]{Value::Int(n)=>Value::Int(n.wrapping_abs()),Value::Real(n)=>Value::Real(n.abs()),_=>bail!("abs requires a number")}},
            "round"|"floor"|"ceil"|"sqrt"|"exp"|"log"|"sin"|"cos"|"tan"=>{ensure!(v.len()==1,"Math function arity");let x=v[0].real()?;Value::Real(match n {"round"=>x.round(),"floor"=>x.floor(),"ceil"=>x.ceil(),"sqrt"=>x.sqrt(),"exp"=>x.exp(),"log"=>x.ln(),"sin"=>x.sin(),"cos"=>x.cos(),_=>x.tan()})},
            "in_range"=>Value::Int(i32::from(int(0)?>=int(1)? && int(0)?<=int(2)?)),
            "get_engine_par_disp"=>{self.ui.diagnostics.insert("Engine display values are unavailable".into());Value::Text(String::new())},
            _=>bail!("Unsupported KSP expression function: {n}"),
        })
    }
    fn assign(&mut self,e:&Expr,value:Value)->Result<()> {
        let Expr::Var(name,index)=e else{bail!("Assignment target must be a variable")};
        if let Some(index)=index {let n=usize::try_from(self.eval(index)?.int()?)?;let Some(Value::Array(a))=self.vars.get_mut(name) else{bail!("Unknown array {name}")};*a.get_mut(n).context("KSP array index out of bounds")?=value;}
        else{ensure!(self.vars.contains_key(name),"Undeclared variable {name}");self.vars.insert(name.clone(),value);}Ok(())
    }
    fn run(&mut self,statements:&[Stmt],depth:usize)->Result<()> {
        ensure!(depth<64,"KSP call nesting limit");
        for s in statements {self.tick()?;match s {
            Stmt::Declare(kind,name,array,args)=>{
                ensure!(self.vars.len()<16384,"KSP variable limit");ensure!(!self.vars.contains_key(name),"Duplicate variable {name}");let args:Vec<_>=args.iter().map(|e|self.eval(e)).collect::<Result<_>>()?;
                let zero=if name.starts_with(['@','!']){Value::Text(String::new())}else if name.starts_with(['~','?']){Value::Real(0.0)}else{Value::Int(0)};
                let value=if let Some(size)=array{let size=usize::try_from(self.eval(size)?.int()?)?;ensure!(size<=100_000 && self.elements+size<=500_000,"KSP array memory limit");self.elements+=size;ensure!(args.len()<=size,"Too many array initializers");let mut a=vec![zero;size];for (i,v) in args.iter().enumerate(){a[i]=v.clone();}Value::Array(a)}else if kind.starts_with("ui_"){zero}else{args.first().cloned().unwrap_or(zero)};
                self.vars.insert(name.clone(),value);
                self.ids.insert(name.clone(),32768+self.controls_by_id.len());
                self.controls_by_id.push(kind.starts_with("ui_").then_some(self.ui.controls.len()));
                if kind.starts_with("ui_") {ensure!(self.ui.controls.len()<4096,"KSP control limit");let mut properties=BTreeMap::new();
                    for (k,v) in [("POS_X",0),("POS_Y",0),("WIDTH",85),("HEIGHT",if kind=="ui_knob"{40}else{18}),("HIDE",0)]{properties.insert(format!("$CONTROL_PAR_{k}"),Value::Int(v));}
                    if ["ui_knob","ui_slider","ui_value_edit"].contains(&kind.as_str()) {ensure!(args.len()>=2,"Control range missing");properties.insert("$CONTROL_PAR_MIN_VALUE".into(),args[0].clone());properties.insert("$CONTROL_PAR_MAX_VALUE".into(),args[1].clone());}
                    self.ui.controls.push(Control{variable:name.clone(),kind:kind.clone(),properties,menu:Vec::new()});
                }
            },
            Stmt::Assign(e,v)=>{let v=self.eval(v)?;self.assign(e,v)?;},
            Stmt::If(e,yes,no)=>{let yes_branch=self.eval(e)?.int()?!=0;self.run(if yes_branch{yes}else{no},depth+1)?;},
            Stmt::While(e,body)=>{while self.eval(e)?.int()?!=0{self.run(body,depth+1)?;}},
            Stmt::Select(e,cases)=>{let value=self.eval(e)?.int()?;for (low,high,body) in cases {self.tick()?;let low=self.eval(low)?.int()?;let high=self.eval(high)?.int()?;ensure!(low<=high,"Reversed case range");if (low..=high).contains(&value){self.run(body,depth+1)?;break;}}},
            Stmt::Function(name)=>{
                if !self.functions.contains_key(name){let source=self.sources.get(name).with_context(||format!("Unknown function {name}"))?;let body=parse_body(source,&mut self.parse_budget)?;self.functions.insert(name.clone(),std::sync::Arc::new(body));}
                let body=self.functions[name].clone();self.run(&body,depth+1)?;
            },
            Stmt::Command(name,args)=>self.command(name,args)?,
        }}Ok(())
    }
    fn command(&mut self,n:&str,a:&[Expr])->Result<()> {
        if n=="inc" || n=="dec" {ensure!(a.len()==1,"inc/dec arity");let v=self.eval(&a[0])?.int()?.wrapping_add(if n=="inc"{1}else{-1});return self.assign(&a[0],Value::Int(v));}
        // Saved values and system-script behavior remain unavailable; expose that limitation.
        if ["make_persistent","read_persistent_var","make_instr_persistent"].contains(&n){self.ui.diagnostics.insert("Saved KSP variable values are not restored".into());return Ok(());}
        if ["SET_CONDITION","message"].contains(&n){self.ui.diagnostics.insert(format!("Not executed: {n}"));return Ok(());}
        if n.starts_with("pgs_") {
            let key=key_id(a)?.to_owned();
            match n {
                "pgs_create_key"=>{
                    ensure!(a.len()==2,"PGS create arity");let size=usize::try_from(self.eval(&a[1])?.int()?)?;ensure!((1..=256).contains(&size),"PGS key size must be 1..256");
                    if let Some(old)=self.host.integers.get(&key){ensure!(old.len()==size,"PGS key redeclared with a different size");}else{ensure!(self.host.integers.len()<4096,"PGS key limit");self.host.integers.insert(key,vec![0;size]);}
                },
                "pgs_create_str_key"=>{ensure!(a.len()==1,"PGS string create arity");ensure!(self.host.strings.len()<4096 || self.host.strings.contains_key(&key),"PGS key limit");self.host.strings.entry(key).or_default();},
                "pgs_set_key_val"=>{
                    ensure!(a.len()==3,"PGS set arity");let index=usize::try_from(self.eval(&a[1])?.int()?)?;let value=self.eval(&a[2])?.int()?;
                    *self.host.integers.get_mut(&key).context("Unknown PGS integer key")?.get_mut(index).context("PGS index out of bounds")?=value;
                    self.ui.diagnostics.insert("PGS storage updated; pgs_changed callbacks are not dispatched".into());
                },
                "pgs_set_str_key_val"=>{
                    ensure!(a.len()==2,"PGS string set arity");let value=self.eval(&a[1])?.text();ensure!(value.len()<=65536,"PGS string length limit");
                    let previous=self.host.strings.get(&key).context("Unknown PGS string key")?.len();let bytes:usize=self.host.strings.values().map(String::len).sum();ensure!(bytes-previous+value.len()<=4*1024*1024,"PGS string memory limit");
                    self.host.strings.insert(key,value);self.ui.diagnostics.insert("PGS storage updated; pgs_changed callbacks are not dispatched".into());
                },
                _=>bail!("Unsupported PGS command: {n}"),
            }return Ok(());
        }
        let values:Vec<_>=a.iter().map(|e|self.eval(e)).collect::<Result<_>>()?;
        let int=|i:usize|values.get(i).context("Missing command argument")?.int();let text=|i:usize|values.get(i).map(Value::text).context("Missing command argument");
        match n {
            "set_key_pressed_support"=>{ensure!(values.len()==1 && (0..=1).contains(&int(0)?),"Pressed support mode must be 0 or 1");self.host.script_pressed=int(0)?==1;self.ui.diagnostics.insert("Keyboard state captured; script keyboard display is not connected".into());},
            "set_key_name"|"set_key_color"|"set_key_type"|"set_key_pressed"=>{
                ensure!(values.len()==2,"Keyboard setter arity");let note=midi_note(int(0)?)?;
                if n=="set_key_pressed"{ensure!((0..=1).contains(&int(1)?),"Pressed state must be 0 or 1");if !self.host.script_pressed{return Ok(());}}
                let key=self.host.keyboard.entry(note).or_default();match n {"set_key_name"=>key.name=text(1)?,"set_key_color"=>key.color=Some(values[1].clone()),"set_key_type"=>key.kind=Some(values[1].clone()),_=>key.pressed=int(1)?==1}
                self.ui.diagnostics.insert("Keyboard state captured; script keyboard display is not connected".into());
            },
            "set_listener"|"change_listener_par"=>{
                ensure!(values.len()==2,"Listener arity");let signal=text(0)?;let value=int(1)?;
                ensure!(match signal.as_str(){"$NI_SIGNAL_TIMER_MS"=>value>=1000 || value==0,"$NI_SIGNAL_TIMER_BEAT"=>(0..=24).contains(&value),"$NI_SIGNAL_TRANSP_START"|"$NI_SIGNAL_TRANSP_STOP"=>n=="set_listener" && (0..=1).contains(&value),_=>false},"Invalid listener signal/parameter");
                if value==0{self.ui.listeners.remove(&signal);}else{self.ui.listeners.insert(signal,value);}
                self.ui.diagnostics.insert("Listener registered; listener callbacks are not dispatched".into());
            },
            "make_perfview"=>self.ui.performance=true,
            "set_ui_height_px"=>self.ui.height=int(0)?.clamp(1,4096),"set_ui_width_px"=>self.ui.width=int(0)?.clamp(1,4096),
            "set_script_title"=>self.ui.title=text(0)?,
            "set_control_par"|"set_control_par_str"=>{ensure!(values.len()==3,"set_control_par arity");let prop=text(1)?;if n=="set_control_par"{values[2].int()?;}
                if text(0)?=="$INST_WALLPAPER_ID" && prop=="$CONTROL_PAR_PICTURE"{self.ui.wallpaper=text(2)?;return Ok(());}
                if text(0)?=="$INST_ICON_ID"{return Ok(());}
                let id=self.control_index(usize::try_from(int(0)?)?)?;let c=self.ui.controls.get_mut(id).context("Unknown control ID")?;
                if prop=="$CONTROL_PAR_VALUE"{self.vars.insert(c.variable.clone(),values[2].clone());}else{c.properties.insert(prop,values[2].clone());}
            },
            "add_menu_item"|"set_text"|"hide_part"|"move_control_px"|"move_control"|"set_knob_unit"|"set_knob_label"|"set_control_help"=>{
                let Some(Expr::Var(variable,None))=a.first() else{bail!("{n} requires a control variable")};let id=self.control_index(*self.ids.get(variable).context("Unknown UI variable")?)?;
                let c=&mut self.ui.controls[id];match n {
                    "add_menu_item"=>{ensure!(c.menu.len()<4096,"Menu item limit");c.menu.push((text(1)?,int(2)?));},
                    "set_text"|"set_knob_label"|"set_control_help"=>{ensure!(values.len()==2,"Control text arity");let property=match n{"set_text"=>"$CONTROL_PAR_TEXT","set_knob_label"=>"$CONTROL_PAR_LABEL",_=>"$CONTROL_PAR_HELP"};c.properties.insert(property.into(),Value::Text(text(1)?));},
                    "set_knob_unit"=>{ensure!(values.len()==2,"Knob unit arity");c.properties.insert("$CONTROL_PAR_UNIT".into(),values[1].clone());},
                    "hide_part"=>{c.properties.insert("$CONTROL_PAR_HIDE".into(),values.get(1).context("Missing hide value")?.clone());},
                    _=>{let mut x=int(1)?;let mut y=int(2)?;if n=="move_control"{x=x.saturating_sub(1).saturating_mul(92).saturating_add(66);y=y.saturating_sub(1).saturating_mul(21).saturating_add(2);}c.properties.insert("$CONTROL_PAR_POS_X".into(),Value::Int(x));c.properties.insert("$CONTROL_PAR_POS_Y".into(),Value::Int(y));},
                }
            },
            _=>bail!("Unsupported KSP initialization command: {n}"),
        }Ok(())
    }
}
/// Execute on init with deterministic limits. Partial/failed layouts are never returned as success.
/// Static requirements, including callbacks that the initialization preview does not execute.
/// Observed names are an inventory, not a claim that their behavior is implemented.
pub fn requirements(source:&str)->Result<serde_json::Value>{
    ensure!(source.len()<=128*1024*1024,"KSP static inventory exceeds 128 MiB");
    let mut sets:[BTreeSet<String>;4]=std::array::from_fn(|_|BTreeSet::new());
    let mut previous=String::new();let mut token=String::new();let mut comment=false;let mut quoted=false;
    let mut emit=|token:&mut String,delimiter:char|->Result<()> {
        if !token.is_empty() {
            if previous=="on" {sets[0].insert(token.clone());}
            if previous=="declare" && token.starts_with("ui_"){sets[3].insert(token.clone());}
            if token.starts_with('$') && token.chars().nth(1).is_some_and(|c|c.is_ascii_uppercase()){sets[2].insert(token.clone());}
            previous=std::mem::take(token);
            if matches!(previous.as_str(),"make_perfview"|"exit"|"ignore_controller"|"reset_ksp_timer"|"expose_controls"){sets[1].insert(previous.clone());}
        }
        if delimiter=='(' && previous.chars().next().is_some_and(|c|c.is_ascii_alphabetic()) && !matches!(previous.as_str(),"if"|"while"|"select"|"case"|"ui_control"|"and"|"or"|"not"|"xor"|"mod"){sets[1].insert(previous.clone());}
        if !delimiter.is_whitespace() || delimiter=='\n'{previous.clear();}
        ensure!(sets.iter().map(BTreeSet::len).sum::<usize>()<=65536,"Too many distinct KSP identifiers");Ok(())
    };
    for c in source.chars().chain(std::iter::once(' ')) {
        if comment {if c=='}'{comment=false;}continue;}
        if quoted {if c=='"'{quoted=false;}continue;}
        if c=='{' {emit(&mut token,' ')?;comment=true;continue;}
        if c=='"' {emit(&mut token,'"')?;quoted=true;continue;}
        if c.is_alphanumeric() || "_$%@!~?".contains(c) {token.push(c);ensure!(token.len()<=65536,"KSP identifier too long");}else{emit(&mut token,c)?;}
    }
    ensure!(!comment && !quoted,"Unterminated KSP comment/string");
    Ok(serde_json::json!({"callbacks":sets[0],"calls":sets[1],"constants":sets[2],"controls":sets[3],"runtime_callbacks_implemented":false}))
}

pub fn inspect(source:&str,groups:usize,host:&mut HostState)->serde_json::Value {
    let requirements=requirements(source).unwrap_or_else(|e|serde_json::json!({"inventory_error":format!("{e:#}")}));
    let initialization=match initialize_with_host(source,groups,8,host){Ok(ui)=>serde_json::json!({"controls":ui.controls.len(),"listeners":ui.listeners,"diagnostics":ui.diagnostics,"preview_only":true}),Err(e)=>serde_json::json!({"error":format!("{e:#}")})};
    serde_json::json!({"requirements":requirements,"initialization":initialization})
}

fn parse_body(lines:&[(usize,String)],budget:&mut usize)->Result<Vec<Stmt>> {
    for (_,line) in lines {let count=tokens(line)?.len();ensure!(count<=*budget,"KSP program token budget exhausted");*budget-=count;}
    let mut at=0;let body=block(lines,&mut at,0)?;ensure!(at==lines.len(),"Unexpected block terminator");Ok(body)
}

pub fn initialize(source:&str,groups:usize,outputs:usize)->Result<Interface>{initialize_with_host(source,groups,outputs,&mut HostState::default())}
/// Commit shared state only if the entire slot initializes successfully.
pub fn initialize_with_host(source:&str,groups:usize,outputs:usize,host:&mut HostState)->Result<Interface>{
    let mut staged=host.clone();let ui=initialize_slot(source,groups,outputs,&mut staged)?;*host=staged;Ok(ui)
}
fn initialize_slot(source:&str,groups:usize,outputs:usize,host:&mut HostState)->Result<Interface>{
    let lines=lines(source)?;let mut init=None;let mut sources=BTreeMap::new();let mut i=0;
    // Parse only initialization and called functions; playback callbacks remain unavailable.
    while i<lines.len(){let header=&lines[i].1;let start=i+1;
        let end=if header.starts_with("on "){"end on"}else if header.starts_with("function "){"end function"}else{bail!("Unexpected top-level KSP statement: {header}")};
        i=start;while i<lines.len() && lines[i].1!=end {ensure!(!lines[i].1.starts_with("on ") && !lines[i].1.starts_with("function "),"Unclosed KSP callback/function");i+=1;}ensure!(i<lines.len(),"Unclosed KSP callback/function");
        if header=="on init" {ensure!(init.is_none(),"Duplicate init callback");init=Some(&lines[start..i]);}
        else if let Some(name)=header.strip_prefix("function "){ensure!(sources.insert(name.to_owned(),&lines[start..i]).is_none(),"Duplicate KSP function");}
        i+=1;
    }
    let mut r=Runtime{host,vars:BTreeMap::new(),ids:BTreeMap::new(),controls_by_id:Vec::new(),functions:BTreeMap::new(),sources,parse_budget:500_000,ui:Interface::default(),steps:200_000,elements:0,bytes:0};
    for (n,v) in [("$NUM_GROUPS",i32::try_from(groups)?),("$NUM_OUTPUT_CHANNELS",i32::try_from(outputs)?),("$HIDE_WHOLE_CONTROL",1),("$HIDE_PART_BG",2),("$HIDE_PART_NOTHING",0)]{r.vars.insert(n.into(),Value::Int(v));}
    let init=parse_body(init.context("No KSP init callback")?,&mut r.parse_budget)?;r.run(&init,0)?;
    for c in &mut r.ui.controls {c.properties.insert("$CONTROL_PAR_VALUE".into(),r.vars.get(&c.variable).cloned().unwrap_or(Value::Int(0)));}
    Ok(r.ui)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn control_ids_follow_declaration_order() {
        let source="on init\ndeclare const $prefix:=0\ndeclare ui_knob $first(0,100,1)\ndeclare $gap\ndeclare ui_knob $second(0,100,1)\ndeclare ui_label $label(1,1)\nset_control_par(32769,$CONTROL_PAR_VALUE,17)\nset_control_par(get_ui_id($first)+2,$CONTROL_PAR_VALUE,29)\nset_text($label,get_ui_id($prefix) & \":\" & get_control_par(get_ui_id($second),$CONTROL_PAR_VALUE))\nset_text($label,get_control_par_str(get_ui_id($label),$CONTROL_PAR_TEXT) & \":ok\")\nend on";
        let source=source.replace("end on","set_knob_label($first,\"dB\")\nset_knob_unit($first,$KNOB_UNIT_DB)\nset_control_help($first,\"Mic volume\")\nend on");
        let ui=initialize(&source,0,0).unwrap();assert_eq!(ui.controls[0].properties["$CONTROL_PAR_LABEL"].text(),"dB");assert_eq!(ui.controls[0].properties["$CONTROL_PAR_UNIT"].text(),"$KNOB_UNIT_DB");assert_eq!(ui.controls[0].properties["$CONTROL_PAR_HELP"].text(),"Mic volume");assert_eq!(ui.controls[0].properties["$CONTROL_PAR_VALUE"].int().unwrap(),17);assert_eq!(ui.controls[1].properties["$CONTROL_PAR_VALUE"].int().unwrap(),29);assert_eq!(ui.controls[2].properties["$CONTROL_PAR_TEXT"].text(),"32768:29:ok");
        for bad in ["set_control_par(get_ui_id($gap),$CONTROL_PAR_VALUE,1)","set_control_par(0,$CONTROL_PAR_VALUE,1)"] {assert!(initialize(&source.replace("end on",&format!("{bad}\nend on")),0,0).is_err());}
    }
    #[test]
    fn shared_host_services_are_scoped_and_transactional() {
        let mut host=HostState::default();
        let first="on init\npgs_create_key(MIC_LEVEL,2)\npgs_set_key_val(MIC_LEVEL,1,73)\npgs_create_str_key(PRESET_NAME)\npgs_set_str_key_val(PRESET_NAME,\"Warm\")\nset_key_pressed(60,1)\nset_key_pressed_support(1)\nset_key_pressed(61,1)\nset_key_name(61,\"Keyswitch\")\nset_key_color(61,$KEY_COLOR_RED)\nset_key_type(61,$NI_KEY_TYPE_CONTROL)\nset_listener($NI_SIGNAL_TIMER_MS,1000)\nchange_listener_par($NI_SIGNAL_TIMER_MS,2000)\nend on";
        let ui=initialize_with_host(first,0,0,&mut host).unwrap();assert_eq!(ui.listeners["$NI_SIGNAL_TIMER_MS"],2000);
        assert!(!host.keyboard.contains_key(&60));assert!(host.keyboard[&61].pressed);assert_eq!(host.keyboard[&61].name,"Keyswitch");
        let next="on init\npgs_create_key(MIC_LEVEL,2)\ndeclare ui_label $label(1,1)\nset_text($label,pgs_get_str_key_val(PRESET_NAME) & pgs_get_key_val(MIC_LEVEL,1) & get_key_name(61) & get_key_triggerstate(61))\nend on";
        assert_eq!(initialize_with_host(next,0,0,&mut host).unwrap().controls[0].properties["$CONTROL_PAR_TEXT"].text(),"Warm73Keyswitch1");
        assert!(initialize(next,0,0).is_err());
        assert!(initialize_with_host("on init\npgs_set_key_val(MIC_LEVEL,1,0)\nunsupported()\nend on",0,0,&mut host).is_err());assert_eq!(host.integers["MIC_LEVEL"][1],73);
        for bad in ["pgs_create_key(TOO_BIG,257)","pgs_set_key_val(MIC_LEVEL,2,1)","pgs_create_key(MIC_LEVEL,3)","set_key_pressed_support(2)","set_key_name(128,\"bad\")","set_listener($NI_SIGNAL_TIMER_MS,999)","set_listener($NI_SIGNAL_TIMER_BEAT,25)"] {
            assert!(initialize_with_host(&format!("on init\n{bad}\nend on"),0,0,&mut host).is_err(),"{bad}");
        }
        let ui=initialize_with_host("on init\nset_listener($NI_SIGNAL_TIMER_BEAT,4)\nchange_listener_par($NI_SIGNAL_TIMER_BEAT,0)\nend on",0,0,&mut host).unwrap();assert!(ui.listeners.is_empty());
    }
    #[test]
    fn real_expressions_and_arrays() {
        let source="on init\ndeclare ?a[2] := (2.5, -3.0)\ndeclare ~x := ?a[0]*2.0\ndeclare ui_label $label(1,1)\nif (~x=5.0)\nset_text($label,int(round(~x+real(2))) & \":\" & int(abs(?a[1])))\nend if\nend on";
        assert_eq!(initialize(source,0,0).unwrap().controls[0].properties["$CONTROL_PAR_TEXT"].text(),"7:3");
        for expression in ["1.0/0.0","sqrt(-1.0)","exp(1000.0)","int(2147483648.0)","1.0+1"] {assert!(initialize(&format!("on init\ndeclare ~x := {expression}\nend on"),0,0).is_err());}
        assert!(initialize("on init\ndeclare ~x := 1.0e-3\ndeclare $bits := sh_right(sh_left(3.and.1,4),2)\ndeclare $cast := real_to_int(int_to_real(4))\nend on",0,0).is_ok());
    }
    #[test]
    fn whitespace_and_comments() {
        let source="on\tinit\ndeclare{separator}ui_label $label(1,1)\ndeclare $n:=0\nwhile($n<1)\nif(1)\nselect($n)\ncase\t0\nset_text($label,\" keep  := spaces \")\nend\t select\nend  if\ninc($n)\nend\twhile\nend  on";
        assert_eq!(initialize(source,0,0).unwrap().controls[0].properties["$CONTROL_PAR_TEXT"].text()," keep  := spaces ");
        assert!(initialize("on init\niffy(1)\nend on",0,0).unwrap_err().to_string().contains("iffy"));
    }
    #[test]
    fn select_and_reachable_parsing() {
        let source="on init\ndeclare ui_label $label(1,1)\ndeclare $n:=4\nselect ($n)\ncase 0\nunknown()\ncase 3 to 5\nselect (1)\ncase 1\nset_text($label,\"selected\")\nend select\ncase 4\nunknown()\nend select\nend on\nfunction unused\nunsupported syntax here\nend function\non note\nunsupported playback syntax\nend on";
        assert_eq!(initialize(source,0,0).unwrap().controls[0].properties["$CONTROL_PAR_TEXT"].text(),"selected");
        assert!(initialize(&source.replace("declare $n:=4","call unused\ndeclare $n:=4"),0,0).is_err());
        let array=format!("on init\ndeclare %a[1000] := ({})\nend on",vec!["1";1000].join(","));assert!(initialize(&array,0,0).is_ok());
        assert!(expression(&vec!["1";10000].join("+")).unwrap_err().to_string().contains("expression token budget"));
        assert!(initialize("on init\nselect (0)\ncase 1 to 0\nend select\nend on",0,0).is_err());
        assert!(initialize("on init\nend on\non note\non release\nend on",0,0).is_err());
        assert!(initialize("on init\ncall loop\nend on\nfunction loop\ncall loop\nend function",0,0).is_err());
    }
    #[test]
    fn computed_ui_and_execution_limits(){
        let source=r#"on init
 declare $i
 declare %ids[2]
 declare ui_switch $a
 declare ui_switch $b
 declare @picture := "My" & " UI"
 set_control_par_str($INST_WALLPAPER_ID,$CONTROL_PAR_PICTURE,@picture)
 while ($i<2)
 %ids[$i] := get_ui_id($a)+$i
 set_control_par(%ids[$i],$CONTROL_PAR_POS_X,10+$i*90)
 set_control_par_str(%ids[$i],$CONTROL_PAR_TEXT,"Mic " & ($i+1))
 inc($i)
 end while
 if ($NUM_GROUPS>1 and $i=2)
 set_control_par(get_ui_id($b),$CONTROL_PAR_HIDE,$HIDE_WHOLE_CONTROL)
 end if
 end on"#;
        let inventory=requirements(&format!("{source}\non note\nplay_note($EVENT_NOTE,100,0,-1)\nend on\n{{ fake_call() }}")).unwrap();
        assert_eq!(inventory["callbacks"],serde_json::json!(["init","note"]));
        assert!(inventory["calls"].as_array().unwrap().contains(&serde_json::json!("play_note")));
        assert!(!inventory["calls"].as_array().unwrap().contains(&serde_json::json!("fake_call")));
        let names=requirements("on init\nmake_perfview\nif (1 and (2))\nexit\nend if\nend on").unwrap();assert_eq!(names["calls"],serde_json::json!(["exit","make_perfview"]));
        let ui=initialize(source,2,8).unwrap();assert_eq!(ui.wallpaper,"My UI");assert_eq!(ui.controls[1].properties["$CONTROL_PAR_POS_X"].int().unwrap(),100);assert_eq!(ui.controls[1].properties["$CONTROL_PAR_TEXT"].text(),"Mic 2");assert_eq!(ui.controls[1].properties["$CONTROL_PAR_HIDE"].int().unwrap(),1);
        assert!(initialize("on init\nwhile (1)\nend while\nend on",0,0).unwrap_err().to_string().contains("budget"));
        assert!(initialize("on init\ndeclare %a[1]\n%a[2] := 1\nend on",0,0).is_err());
        assert!(initialize("on init\nunknown_function()\nend on",0,0).is_err());
        assert!(initialize("on init\ndeclare $a := 1/0\nend on",0,0).is_err());
        assert!(initialize("on init\ndeclare @s := \"x\"\nwhile (1)\n@s := @s & @s\nend while\nend on",0,0).is_err());
        assert!(initialize(&format!("on init\n{}end on", "message(1)\n".repeat(125_001)),0,0).unwrap_err().to_string().contains("token budget"));
        assert!(initialize("on init\ndeclare ui_button $a\nmove_control($a,2147483647,-2147483647-1)\nend on",0,0).is_ok());
        assert_eq!(expression("080000000h").map(|e|matches!(e,Expr::Value(Value::Int(i32::MIN)))).unwrap(),true);
        let bits=initialize("on init\ndeclare ui_label $x(1,1)\nset_text($x, ...\n (0FFh .and. 15) .or. (.not. 0FFFFFFF0h))\nend on",0,0).unwrap();assert_eq!(bits.controls[0].properties["$CONTROL_PAR_TEXT"].text(),"15");
        let large=format!("{{ {} }}\non note\nplay_note(60,100,0,-1)\nend on", "x".repeat(17*1024*1024));assert!(requirements(&large).is_ok());assert!(initialize(&large,0,0).is_err());
        let called=initialize("on init\ndeclare ui_label $a(1,1)\ncall label\nend on\nfunction label\nset_text($a,\"{quoted}\")\nend function",0,0).unwrap();
        assert_eq!(called.controls[0].properties["$CONTROL_PAR_TEXT"].text(),"{quoted}");
    }
}
