//! KONTRA as an app of its own: the plugin and its editor in a window, with
//! audio and MIDI from the system (moose-standalone). `--help` lists the
//! device and buffer options.

fn main() {
    moose_standalone::run::<kontra_plugin::PluginShell>();
}
