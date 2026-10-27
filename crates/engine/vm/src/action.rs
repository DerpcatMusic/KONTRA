//! What a script asks of its host. Front ends map their builtins onto these
//! (KSP `play_note`, a UVI `playNote`); the engine implements each once.
//! Pure actions (arithmetic, array search and sort) never reach the host:
//! the VM evaluates them ([`crate::pure`]).
//!
//! Units, whatever the front end: tuning in millicents (100000 = a semitone),
//! volume in millidecibels, pan in per mille (-1000..=1000), time in
//! microseconds. An event argument is an event id, [`event::ALL`] or
//! [`event::BY_MARKS`] with mark bits.

/// Event addressing and parameter ids shared by every front end (KSP's numbering).
pub mod event {
    /// Every live event.
    pub const ALL: i32 = 0x3FFF_FFFE;
    /// Flag: the low bits are marks; the argument means every event carrying one.
    pub const BY_MARKS: i32 = 0x2000_0000;
    /// A group past the last means every group.
    pub const ALL_GROUPS: i32 = 0x3FFF_FFFF;

    /// `0..=3` are free for the script.
    pub const CUSTOM_LAST: i32 = 3;
    pub const VOLUME: i32 = 4;
    pub const TUNE: i32 = 5;
    pub const PAN: i32 = 6;
    pub const NOTE: i32 = 7;
    pub const VELOCITY: i32 = 8;
    pub const ALLOW_GROUP: i32 = 9;
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
    // Time.
    WaitTicks,
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
}
