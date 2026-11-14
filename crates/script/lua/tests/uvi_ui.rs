//! The UVIScript UI probe (`uvi_ui/probe.lua`) against the reference: each
//! documented widget member, one line. `uvi_ui/probe.expected` is what this
//! host answers; a `FAIL` line is a gap (docs/uvi/UI-WIDGETS.md). Run with
//! `UPDATE_PROBE=1` to rewrite it after closing one.

use kontra_lua::{Callbacks, compile};
use kontra_vm::{Action, Foreign, HostApi, HostCall, HostValue, Step};

struct Lines(Vec<String>);

impl HostApi for Lines {
    fn call(&mut self, call: HostCall<'_>) -> HostValue {
        if call.action == Action::Message {
            self.0.extend(call.texts.iter().map(|t| t.as_str().to_string()));
        }
        HostValue::Int(0)
    }
}

#[test]
fn every_documented_ui_member_answers_as_the_expected_file_says() {
    let tree = "return {params={},db=0,pan=0,modulations={},layers={{params={Gain='0.5'},keygroups={
        {params={},oscillators={{params={SamplePath='./a.wav',BaseNote='60'}}}}}}}}";
    let source = include_str!("uvi_ui/probe.lua");
    let modules = [("__program.lua".to_string(), tree.to_string())];
    let mut machine = compile(source, &modules, &[]).expect("probe loads").machine();
    let mut lines = Lines(Vec::new());
    let co = machine.start(Callbacks::INIT, 0).expect("init");
    assert_eq!(machine.run(co, &mut lines), Step::Done);
    let got = lines.0.join("\n") + "\n";
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/uvi_ui/probe.expected");
    if std::env::var_os("UPDATE_PROBE").is_some() {
        std::fs::write(path, &got).expect("write");
        return;
    }
    let want = std::fs::read_to_string(path).expect("probe.expected");
    assert_eq!(got, want);
}
