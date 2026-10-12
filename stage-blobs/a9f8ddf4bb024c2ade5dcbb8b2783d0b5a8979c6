pub mod audio_tap;
pub mod buffer;
pub mod bus;
pub mod bus_routing;
pub mod chunked_process;
pub mod config;
pub mod custom_state;
pub mod denormal;
pub mod editor;
pub mod events;
pub mod export;
pub mod info;
pub mod meters;
pub mod midi;
pub mod plugin;
pub mod presets;
pub mod process;
pub mod rt;
pub mod screenshot;
pub mod snapshot;
pub mod state;
pub mod tasks;
pub mod transport;
pub mod ump;
pub mod util;
pub mod wrapper;

pub use buffer::{AudioBuffer, RawBufferScratch};
pub use bus::{BusConfig, BusKind, BusLayout, ChannelConfig};
pub use bus_routing::{
    BusActivation, BusRoute, BusRouting, MAX_AUDIO_BUSES, bus_layout_fits_routing,
    bus_layouts_fit_routing,
};
pub use config::{AudioConfig, ProcessMode};
pub use editor::{Editor, EditorBuilder, IntoEditor, PluginContext};
pub use events::{
    AuEventMetadata, Event, EventBody, EventList, ExactAddress, ExactEvent, ExactEventBody,
    ExactEventMetadata, ExactEventQualifiers, ExactEventRef, ExactEventToken, ExactNoteAddress,
    ExactNoteKind, LosslessEventCursor, LosslessEventRef, OutputEventStatus, PushError, RawMidi1,
    RawUmp, SYSEX_POOL_PREALLOC, TransportInfo, Vst3EventMetadata,
};
pub use export::PluginExport;
pub use info::{AutomationConfig, MidiDialect, PluginCategory, PluginInfo};
pub use meters::MeterStore;
pub use plugin::PluginRuntime;
pub use process::{ProcessContext, ProcessStatus};
pub use rt::{RtSection, allow_alloc};
pub use snapshot::SnapshotSlot;

#[cfg(feature = "rt-paranoid")]
pub use rt::RtCheckAlloc;
pub use transport::TransportSlot;

// `Float` / `Sample` live in `moose-params` (moose-core depends on
// moose-params, not the other way around). Re-exported here so
// `moose_core::Float` / `moose_core::sample::Sample` are valid paths
// for callers that don't want to depend on moose-params directly.
pub use moose_params::sample;
pub use moose_params::sample::{Float, Sample};
pub use util::{db_to_linear, linear_to_db, meter_display, midi_note_to_freq};

// `cast`, `shell_sidecar`, and `slugify` are hosted in `moose-utils`
// (a dependency-free crate) so build-time consumers like `cargo-moose`
// can use them without inheriting `moose-core`'s `moose-params` + `png`
// publish chain. Re-exported here under `moose_core::cast::*` etc.
// for callers that already depend on moose-core.
pub use moose_utils::{cast, shell_sidecar, slugify};
