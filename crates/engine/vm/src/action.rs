//! What a script asks of its host. Front ends map their builtins onto these
//! (KSP `play_note`, a UVI `playNote`); the engine implements each once.
//! Pure actions (arithmetic, array search and sort) never reach the host:
//! the VM evaluates them ([`crate::pure`]).
//!
//! Units, whatever the front end: tuning in millicents (100000 = a semitone),
//! volume in millidecibels, pan in per mille (-1000..=1000), time in
//! microseconds. An event argument is an event id, [`event::ALL`] or
//! [`event::BY_MARKS`] with mark bits.

/// Event addressing and parameter ids shared by every front end: KSP's
/// numbering, measured in the native oracle (ksp_constants.py).
pub mod event {
    /// Every live event (event ids are positive).
    pub const ALL: i32 = -1;
    /// Flag: the low bits are marks; the argument means every event carrying one.
    pub const BY_MARKS: i32 = 0x2000_0000;
    /// Every group; so does any group past the last.
    pub const ALL_GROUPS: i32 = -1;

    /// `0..=3` are free for the script.
    pub const CUSTOM_LAST: i32 = 3;
    pub const VOLUME: i32 = 4;
    pub const PAN: i32 = 5;
    pub const TUNE: i32 = 6;
    pub const NOTE: i32 = 7;
    /// Read only: -1 for a note from outside, else the script slot that played it.
    pub const SOURCE: i32 = 8;
    pub const VELOCITY: i32 = 9;
    pub const ALLOW_GROUP: i32 = 10;
    /// With `*_arr`: the value for a "from script" modulator, ±1 000 000.
    pub const MOD_VALUE: i32 = 30;
    /// Before the note starts: only zones in this slot start; -1 (the
    /// default) starts any.
    pub const ZONE_SLOT: i32 = 15;
    /// Read only: microseconds into its zone's sample its first voice plays.
    pub const PLAY_POS: i32 = 1026;
    /// Read only: the number of the zone its first voice plays (its format's
    /// numbering), -1 before a voice starts.
    pub const ZONE_ID: i32 = 1027;
}

macro_rules! actions {
    ($($(#[$doc:meta])* $id:ident,)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u16)]
        pub enum Action { $($(#[$doc])* $id),* }

        const ALL: &[Action] = &[$(Action::$id),*];
    };
}

impl Action {
    #[must_use]
    pub fn id(self) -> u16 {
        self as u16
    }

    #[must_use]
    pub fn from_id(id: u16) -> Option<Self> {
        ALL.get(usize::from(id)).copied()
    }

    /// Every action, in id order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        ALL
    }

    /// Whether the VM answers this action itself, without the host.
    #[must_use]
    #[inline]
    pub fn is_pure(self) -> bool {
        matches!(
            self,
            Self::Abs
                | Self::Min
                | Self::Max
                | Self::InRange
                | Self::Sgn
                | Self::Signbit
                | Self::Round
                | Self::Floor
                | Self::Ceil
                | Self::Sqrt
                | Self::Cbrt
                | Self::Exp
                | Self::Exp2
                | Self::Log
                | Self::Log2
                | Self::Log10
                | Self::Sin
                | Self::Cos
                | Self::Tan
                | Self::Asin
                | Self::Acos
                | Self::Atan
                | Self::Pow
                | Self::Msb
                | Self::Lsb
                | Self::Search
                | Self::Sort
                | Self::ArrayEqual
        )
    }
}

actions! {
    // Arithmetic (pure).
    Abs,
    Min,
    Max,
    InRange,
    Sgn,
    Signbit,
    Round,
    Floor,
    Ceil,
    Sqrt,
    Cbrt,
    Exp,
    Exp2,
    Log,
    Log2,
    Log10,
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Pow,
    Msb,
    Lsb,
    // Arrays (pure).
    Search,
    Sort,
    ArrayEqual,
    // Host state.
    Random,
    MsToTicks,
    TicksToMs,
    LoadArray,
    SaveArray,
    LoadArrayStr,
    SaveArrayStr,
    ReadPersistentVar,
    // Events.
    PlayNote,
    NoteOff,
    IgnoreEvent,
    ChangeVol,
    ChangeTune,
    ChangePan,
    ChangeVelo,
    ChangeNote,
    FadeIn,
    FadeOut,
    SetEventPar,
    GetEventPar,
    SetEventParArr,
    GetEventParArr,
    AllowGroup,
    DisallowGroup,
    ByMarks,
    SetEventMark,
    DeleteEventMark,
    GetEventMark,
    EventStatus,
    GetEventIds,
    IgnoreController,
    SetController,
    SetNoteController,
    SetRpn,
    SetNrpn,
    ResetRlsTrigCounter,
    WillNeverTerminate,
    RedirectOutput,
    /// A group's live level: `(group, millidecibels)`, at most -200 dB is
    /// silence. The group's settings, not a note's.
    GroupVolume,
    /// `(group, per mille)`.
    GroupPan,
    /// `(group, millicents)`.
    GroupTune,
    /// An output bus's live level: `(bus, millidecibels)`, at most -200 dB
    /// is silence.
    BusVolume,
    /// A route's live amount: `(key, millionths)`, set on every group route
    /// whose name holds the word `#key`, in its kind's unit (semitones,
    /// pan, octaves of depth, ...).
    RouteAmount,
    /// An effect's bypass: `(where, index, slot, on)`, where 0 is group
    /// `index`'s inserts, 1 bus `index`'s rack, 2 the instrument's inserts.
    EffectBypass,
    /// A gain effect's level: `(where, index, slot, millionths)`, placed as
    /// for [`Action::EffectBypass`].
    EffectGain,
    /// An effect control's value: `(where, index, slot, control, thousandths)`,
    /// placed as for [`Action::EffectBypass`]; the control is an
    /// `ir::EffectParam` code.
    EffectField,
    // Time.
    /// Microseconds the transport takes for a number of MIDI ticks: `wait_ticks`
    /// compiles to this and a `wait`.
    TicksToMicros,
    WaitAsync,
    StopWait,
    ResetKspTimer,
    SetListener,
    ChangeListenerPar,
    // MIDI file object.
    MfGetFirst,
    MfGetLast,
    MfGetNext,
    MfGetPrev,
    MfGetNextAt,
    MfGetPrevAt,
    MfGetId,
    MfGetCommand,
    MfSetCommand,
    MfGetByteOne,
    MfSetByteOne,
    MfGetByteTwo,
    MfSetByteTwo,
    MfGetChannel,
    MfSetChannel,
    MfGetPos,
    MfSetPos,
    MfGetLength,
    MfSetLength,
    MfGetTrackIdx,
    MfSetTrackIdx,
    MfGetEventPar,
    MfSetEventPar,
    MfGetNumTracks,
    MfGetBufferSize,
    MfSetBufferSize,
    MfInsertEvent,
    MfRemoveEvent,
    MfGetMark,
    MfSetMark,
    MfSetExportArea,
    MfSetNumExportAreas,
    MfCopyExportArea,
    MfReset,
    MfInsertFile,
    SaveMidiFile,
    LoadMidiFile,
    MfGetLastFilename,
    ByTrack,
    // Groups, zones, modules and engine parameters.
    FindGroup,
    GetGroupIdx,
    GroupName,
    GetNumZones,
    GetZoneId,
    GetSampleLength,
    GetZonePar,
    SetZonePar,
    PurgeGroup,
    GetPurgeState,
    FindMod,
    FindTarget,
    GetModIdx,
    GetTargetIdx,
    GetEnginePar,
    GetEngineParDisp,
    GetEngineParDispExt,
    SetEnginePar,
    GetEngineParM,
    GetEngineParDispM,
    SetEngineParM,
    GetVoiceLimit,
    SetVoiceLimit,
    OutputChannelName,
    GetScale,
    GetScaleName,
    SubscribeAsync,
    SetGlobalKey,
    SetGlobalScale,
    LoadIrSample,
    FindZone,
    LoadNativeUi,
    LoadPerformanceView,
    // User interface.
    AttachLevelMeter,
    SetControlPar,
    SetControlParStr,
    SetControlParReal,
    SetControlParArr,
    SetControlParStrArr,
    SetControlParRealArr,
    GetControlPar,
    GetControlParStr,
    GetControlParReal,
    GetControlParArr,
    GetControlParStrArr,
    GetControlParRealArr,
    SetText,
    AddTextLine,
    SetKnobLabel,
    SetKnobUnit,
    SetKnobDefval,
    SetControlHelp,
    MoveControl,
    MoveControlPx,
    HidePart,
    AddMenuItem,
    SetMenuItemStr,
    SetMenuItemVisibility,
    SetMenuItemValue,
    GetMenuItemStr,
    GetMenuItemValue,
    GetMenuItemVisibility,
    GetNumMenuItems,
    SetTableStepsShown,
    AttachZone,
    SetSkinOffset,
    SetUiColor,
    SetUiHeight,
    SetUiHeightPx,
    SetUiWidthPx,
    SetScriptTitle,
    MakePerfview,
    ExposeControls,
    SetSnapshotType,
    ShowLibraryTab,
    SetUiWfProperty,
    GetUiWfProperty,
    GetFontId,
    GetFolder,
    FsGetFilename,
    FsNavigate,
    SetNksNavName,
    SetNksNavPar,
    ResetNksNav,
    // Keyboard display.
    SetKeyColor,
    SetKeyName,
    SetKeyType,
    SetKeyPressed,
    SetKeyPressedSupport,
    GetKeyColor,
    GetKeyName,
    GetKeyType,
    GetKeyTriggerstate,
    SetKeyrange,
    RemoveKeyrange,
    SetPolyAt,
    DetectKey,
    DetectPitch,
    DetectLoudness,
    DetectPeak,
    DetectRms,
    DetectTempo,
    DetectSampleType,
    DetectDrumType,
    DetectInstrumentType,
    GetKeyrangeMinNote,
    GetKeyrangeMaxNote,
    GetKeyrangeName,
    SetMapEditorEventColor,
    LoadKompleteUi,
    // Diagnostics.
    Message,
    DisableLogging,
    // Program global storage, shared by every script slot.
    PgsCreateKey,
    PgsKeyExists,
    PgsSetKeyVal,
    PgsGetKeyVal,
    PgsCreateStrKey,
    PgsStrKeyExists,
    PgsSetStrKeyVal,
    PgsGetStrKeyVal,
    // System variables, read in the running callback.
    EventId,
    EventNote,
    EventVelocity,
    NoteHeld,
    CcNum,
    PitchBend,
    PolyAtNum,
    RpnAddress,
    RpnValue,
    MidiChannel,
    EngineUptime,
    KspTimer,
    CallbackType,
    CallbackId,
    SignalType,
    AsyncId,
    AsyncExitStatus,
    DurationQuarter,
    DurationEighth,
    DurationSixteenth,
    DurationQuarterTriplet,
    DurationEighthTriplet,
    DurationSixteenthTriplet,
    DurationBar,
    SongPosition,
    SignatureNum,
    SignatureDenom,
    TransportRunning,
    Bpm,
    Tempo,
    CurrentScriptSlot,
    UiIdVar,
    PlayedVoicesTotal,
    PlayedVoicesInst,
    DistanceBarStart,
    NumGroups,
    NumZones,
    NumOutputChannels,
    MouseOverControl,
    ControlParIdx,
    MouseEventType,
    DateYear,
    DateMonth,
    DateDay,
    TimeHour,
    TimeMinute,
    TimeSecond,
    // System arrays: the index is the argument.
    KeyDown,
    Cc,
    CcTouched,
    PolyAt,
    GroupsSelected,
    GroupsAffected,
    KeyDownOct,
    EventParAt,
    // Appended rather than filed with their neighbours: an action's id is its
    // position, and ids appear in traces and in the engine's per-action
    // counts, so a new one goes on the end.
    //
    // `%NOTE_DURATION[key]`: microseconds since the newest live note on that
    // key started (the KSP manual's time-and-transport array). A script that
    // reads it failed to compile without it, so its `on init` never ran.
    NoteDuration,
    // `hide_inst_icon` and `dont_use_machine_mode`: answered, nothing to do
    // (the engine's host-only list).
    HideInstIcon,
    DontUseMachineMode,
    // Milliseconds since the engine started, with the fraction
    // (`1000 * samples / rate`): notes one frame apart read apart.
    // `EngineUptime` is the same clock in whole milliseconds.
    EngineMs,
    // A file the player picks in the editor (UVI `browseForFile`): ints
    // (save 1, open 0), text the file type (`.ext`); an async id.
    PickFile,
    // The pick with async id int 0: its file name as text once picked, 0
    // when cancelled, nothing while the player is still choosing.
    PickedFile,
}
