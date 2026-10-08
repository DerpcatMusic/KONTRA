//! Static KSP surface: builtin signatures, system variables and constants.
//! Values are resolved at compile time; the VM only sees integers and enums.

/// Compile-time argument kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Integer expression.
    I,
    /// Real expression.
    R,
    /// Any expression, converted to text.
    S,
    /// Integer or real; every `N` in one call shares a type.
    N,
    /// A whole variable (scalar, array or UI control), by reference.
    V,
    /// An array variable, by reference.
    A,
    /// Assignable integer place (`inc`/`dec`).
    P,
    /// Bare identifier or string literal naming a key.
    K,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    Void,
    Int,
    Real,
    Str,
    Bool,
    /// Same type as the `N` arguments.
    Num,
}

pub struct Sig {
    pub args: &'static [Arg],
    /// Trailing optional argument count.
    pub optional: u8,
    pub ret: Ret,
}

macro_rules! builtins {
    ($($id:ident $name:literal [$($arg:ident)*] $opt:literal $ret:ident;)*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Builtin { $($id),* }
        impl Builtin {
            fn lookup(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$id),)* _ => None }
            }
            pub fn name(self) -> &'static str {
                match self { $(Self::$id => $name,)* }
            }
            pub fn sig(self) -> Sig {
                match self {
                    $(Self::$id => Sig { args: &[$(Arg::$arg),*], optional: $opt, ret: Ret::$ret },)*
                }
            }
        }
    };
}

impl Builtin {
    pub fn from_name(name: &str) -> Option<Self> {
        // Kontakt 2's underscore spellings (`_read_persistent_var`, ...).
        Self::lookup(name).or_else(|| Self::lookup(name.strip_prefix('_')?))
    }
}

builtins! {
    // Control flow and statements.
    Exit "exit" [] 0 Void;
    Continue "continue" [] 0 Void;
    Inc "inc" [P] 0 Void;
    Dec "dec" [P] 0 Void;
    // Arithmetic.
    Abs "abs" [N] 0 Num;
    Min "min" [N N] 0 Num;
    Max "max" [N N] 0 Num;
    InRange "in_range" [N N N] 0 Bool;
    Sgn "sgn" [N] 0 Int;
    Signbit "signbit" [N] 0 Int;
    ShLeft "sh_left" [I I] 0 Int;
    ShRight "sh_right" [I I] 0 Int;
    Random "random" [I I] 0 Int;
    IntToReal "int_to_real" [I] 0 Real;
    Real "real" [I] 0 Real;
    RealToInt "real_to_int" [R] 0 Int;
    Int "int" [R] 0 Int;
    Round "round" [R] 0 Real;
    Floor "floor" [R] 0 Real;
    Ceil "ceil" [R] 0 Real;
    Sqrt "sqrt" [R] 0 Real;
    Cbrt "cbrt" [R] 0 Real;
    Exp "exp" [R] 0 Real;
    Exp2 "exp2" [R] 0 Real;
    Log "log" [R] 0 Real;
    Log2 "log2" [R] 0 Real;
    Log10 "log10" [R] 0 Real;
    Sin "sin" [R] 0 Real;
    Cos "cos" [R] 0 Real;
    Tan "tan" [R] 0 Real;
    Asin "asin" [R] 0 Real;
    Acos "acos" [R] 0 Real;
    Atan "atan" [R] 0 Real;
    Pow "pow" [R R] 0 Real;
    Msb "msb" [I] 0 Int;
    Lsb "lsb" [I] 0 Int;
    MsToTicks "ms_to_ticks" [I] 0 Int;
    TicksToMs "ticks_to_ms" [I] 0 Int;
    // Arrays.
    NumElements "num_elements" [A] 0 Int;
    Search "search" [A N I I] 2 Int;
    Sort "sort" [A I I I] 2 Void;
    ArrayEqual "array_equal" [A A] 0 Bool;
    LoadArray "load_array" [A I] 0 Int;
    SaveArray "save_array" [A I] 0 Int;
    LoadArrayStr "load_array_str" [A S] 0 Int;
    SaveArrayStr "save_array_str" [A S] 0 Int;
    // Events.
    PlayNote "play_note" [I I I I] 0 Int;
    NoteOff "note_off" [I I] 1 Void;
    IgnoreEvent "ignore_event" [I] 0 Void;
    ChangeVol "change_vol" [I I I] 1 Void;
    ChangeTune "change_tune" [I I I] 1 Void;
    ChangePan "change_pan" [I I I] 1 Void;
    ChangeVelo "change_velo" [I I] 0 Void;
    ChangeNote "change_note" [I I] 0 Void;
    FadeIn "fade_in" [I I I] 1 Void;
    FadeOut "fade_out" [I I I] 1 Void;
    SetEventPar "set_event_par" [I I I] 0 Void;
    GetEventPar "get_event_par" [I I] 0 Int;
    SetEventParArr "set_event_par_arr" [I I I I] 0 Void;
    GetEventParArr "get_event_par_arr" [I I I] 0 Int;
    AllowGroup "allow_group" [I] 0 Void;
    DisallowGroup "disallow_group" [I] 0 Void;
    ByMarks "by_marks" [I] 0 Int;
    SetEventMark "set_event_mark" [I I] 0 Void;
    DeleteEventMark "delete_event_mark" [I I] 0 Void;
    GetEventMark "get_event_mark" [I I] 0 Int;
    EventStatus "event_status" [I] 0 Int;
    GetEventIds "get_event_ids" [A] 0 Void;
    IgnoreController "ignore_controller" [] 0 Void;
    SetController "set_controller" [I I] 0 Void;
    SetNoteController "set_note_controller" [I I I] 0 Void;
    SetRpn "set_rpn" [I I] 0 Void;
    SetNrpn "set_nrpn" [I I] 0 Void;
    ResetRlsTrigCounter "reset_rls_trig_counter" [I] 0 Void;
    WillNeverTerminate "will_never_terminate" [I] 0 Void;
    RedirectOutput "redirect_output" [I I] 0 Void;
    // Time.
    Wait "wait" [I] 0 Void;
    WaitTicks "wait_ticks" [I] 0 Void;
    WaitAsync "wait_async" [I] 0 Void;
    StopWait "stop_wait" [I I] 0 Void;
    ResetKspTimer "reset_ksp_timer" [] 0 Void;
    SetListener "set_listener" [I I] 0 Void;
    ChangeListenerPar "change_listener_par" [I I] 0 Void;
    // Groups, zones, modules and engine parameters.
    FindGroup "find_group" [S] 0 Int;
    GetGroupIdx "get_group_idx" [S] 0 Int;
    GroupName "group_name" [I] 0 Str;
    GetNumZones "get_num_zones" [] 0 Int;
    GetZoneId "get_zone_id" [I] 0 Int;
    GetZonePar "get_zone_par" [I I] 0 Int;
    SetZonePar "set_zone_par" [I I I] 0 Int;
    PurgeGroup "purge_group" [I I] 0 Int;
    GetPurgeState "get_purge_state" [I] 0 Int;
    FindMod "find_mod" [I S] 0 Int;
    FindTarget "find_target" [I I S] 0 Int;
    GetModIdx "get_mod_idx" [I S] 0 Int;
    GetTargetIdx "get_target_idx" [I I S] 0 Int;
    GetEnginePar "get_engine_par" [I I I I] 0 Int;
    GetEngineParDisp "get_engine_par_disp" [I I I I] 0 Str;
    GetEngineParDispExt "get_engine_par_disp_ext" [I I I I I] 0 Str;
    SetEnginePar "set_engine_par" [I I I I I] 0 Int;
    GetVoiceLimit "get_voice_limit" [I] 0 Int;
    SetVoiceLimit "set_voice_limit" [I I] 0 Int;
    OutputChannelName "output_channel_name" [I] 0 Str;
    LoadIrSample "load_ir_sample" [S I I] 0 Int;
    FindZone "find_zone" [S] 0 Int;
    LoadNativeUi "load_native_ui" [S] 0 Void;
    LoadPerformanceView "load_performance_view" [S] 0 Void;
    // User interface.
    GetUiId "get_ui_id" [V] 0 Int;
    AttachLevelMeter "attach_level_meter" [I I I I I] 0 Void;
    SetControlPar "set_control_par" [I I I] 0 Void;
    SetControlParStr "set_control_par_str" [I I S] 0 Void;
    SetControlParReal "set_control_par_real" [I I R] 0 Void;
    SetControlParArr "set_control_par_arr" [I I I I] 0 Void;
    SetControlParStrArr "set_control_par_str_arr" [I I S I] 0 Void;
    SetControlParRealArr "set_control_par_real_arr" [I I R I] 0 Void;
    GetControlPar "get_control_par" [I I] 0 Int;
    GetControlParStr "get_control_par_str" [I I] 0 Str;
    GetControlParReal "get_control_par_real" [I I] 0 Real;
    GetControlParArr "get_control_par_arr" [I I I] 0 Int;
    GetControlParStrArr "get_control_par_str_arr" [I I I] 0 Str;
    GetControlParRealArr "get_control_par_real_arr" [I I I] 0 Real;
    SetText "set_text" [V S] 0 Void;
    AddTextLine "add_text_line" [V S] 0 Void;
    SetKnobLabel "set_knob_label" [V S] 0 Void;
    SetKnobUnit "set_knob_unit" [V I] 0 Void;
    SetKnobDefval "set_knob_defval" [V I] 0 Void;
    SetControlHelp "set_control_help" [V S] 0 Void;
    MoveControl "move_control" [V I I] 0 Void;
    MoveControlPx "move_control_px" [V I I] 0 Void;
    HidePart "hide_part" [V I] 0 Void;
    AddMenuItem "add_menu_item" [V S I] 0 Void;
    SetMenuItemStr "set_menu_item_str" [I I S] 0 Void;
    SetMenuItemVisibility "set_menu_item_visibility" [I I I] 0 Void;
    SetMenuItemValue "set_menu_item_value" [I I I] 0 Void;
    GetMenuItemStr "get_menu_item_str" [I I] 0 Str;
    GetMenuItemValue "get_menu_item_value" [I I] 0 Int;
    GetMenuItemVisibility "get_menu_item_visibility" [I I] 0 Int;
    GetNumMenuItems "get_num_menu_items" [I] 0 Int;
    SetTableStepsShown "set_table_steps_shown" [V I] 0 Void;
    AttachZone "attach_zone" [V I I] 0 Void;
    SetSkinOffset "set_skin_offset" [I] 0 Void;
    SetUiColor "set_ui_color" [I] 0 Void;
    SetUiHeight "set_ui_height" [I] 0 Void;
    SetUiHeightPx "set_ui_height_px" [I] 0 Void;
    SetUiWidthPx "set_ui_width_px" [I] 0 Void;
    SetScriptTitle "set_script_title" [S] 0 Void;
    MakePerfview "make_perfview" [] 0 Void;
    ExposeControls "expose_controls" [] 0 Void;
    SetSnapshotType "set_snapshot_type" [I] 0 Void;
    ShowLibraryTab "show_library_tab" [] 0 Void;
    SetUiWfProperty "set_ui_wf_property" [V I I I] 0 Void;
    GetUiWfProperty "get_ui_wf_property" [V I I] 0 Int;
    GetFontId "get_font_id" [S] 0 Int;
    GetFolder "get_folder" [I] 0 Str;
    FsGetFilename "fs_get_filename" [I I] 0 Str;
    FsNavigate "fs_navigate" [I I] 0 Void;
    SetNksNavName "set_nks_nav_name" [I I S] 0 Void;
    SetNksNavPar "set_nks_nav_par" [I I I] 0 Void;
    ResetNksNav "reset_nks_nav" [] 0 Void;
    // Keyboard display.
    SetKeyColor "set_key_color" [I I] 0 Void;
    SetKeyName "set_key_name" [I S] 0 Void;
    SetKeyType "set_key_type" [I I] 0 Void;
    SetKeyPressed "set_key_pressed" [I I] 0 Void;
    SetKeyPressedSupport "set_key_pressed_support" [I] 0 Void;
    GetKeyColor "get_key_color" [I] 0 Int;
    GetKeyName "get_key_name" [I] 0 Str;
    GetKeyType "get_key_type" [I] 0 Int;
    GetKeyTriggerstate "get_key_triggerstate" [I] 0 Int;
    SetKeyrange "set_keyrange" [I I S] 0 Void;
    RemoveKeyrange "remove_keyrange" [I] 0 Void;
    // Diagnostics.
    Message "message" [S] 0 Void;
    DisableLogging "disable_logging" [I] 0 Void;
    WatchVar "watch_var" [V] 0 Void;
    WatchArrayIdx "watch_array_idx" [A I] 0 Void;
    // Persistence.
    MakePersistent "make_persistent" [V] 0 Void;
    MakeInstrPersistent "make_instr_persistent" [V] 0 Void;
    ReadPersistentVar "read_persistent_var" [V] 0 Void;
    // Program global storage, shared by all script slots.
    PgsCreateKey "pgs_create_key" [K I] 0 Void;
    PgsKeyExists "pgs_key_exists" [K] 0 Bool;
    PgsSetKeyVal "pgs_set_key_val" [K I I] 0 Void;
    PgsGetKeyVal "pgs_get_key_val" [K I] 0 Int;
    PgsCreateStrKey "pgs_create_str_key" [K] 0 Void;
    PgsStrKeyExists "pgs_str_key_exists" [K] 0 Bool;
    PgsSetStrKeyVal "pgs_set_str_key_val" [K S] 0 Void;
    PgsGetStrKeyVal "pgs_get_str_key_val" [K] 0 Str;
}

/// Script-visible scalars owned by the runtime, read in the current callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SysVar {
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
    Tempo,
    CurrentScriptSlot,
    UiId,
    PlayedVoicesTotal,
    PlayedVoicesInst,
    DistanceBarStart,
    NumGroups,
    NumZones,
    NumOutputChannels,
    MouseOverControl,
    WidgetInteraction(u8),
    Date(u8),
    Time(u8),
}

pub fn sys_var(name: &str) -> Option<SysVar> {
    use SysVar::*;
    Some(match name {
        "$EVENT_ID" => EventId,
        "$EVENT_NOTE" => EventNote,
        "$EVENT_VELOCITY" => EventVelocity,
        "$NOTE_HELD" => NoteHeld,
        "$CC_NUM" => CcNum,
        "$PITCH_BEND" => PitchBend,
        "$POLY_AT_NUM" => PolyAtNum,
        "$RPN_ADDRESS" => RpnAddress,
        "$RPN_VALUE" => RpnValue,
        "$MIDI_CHANNEL" => MidiChannel,
        "$ENGINE_UPTIME" => EngineUptime,
        "$KSP_TIMER" => KspTimer,
        "$NI_CALLBACK_TYPE" => CallbackType,
        "$NI_CALLBACK_ID" => CallbackId,
        "$NI_SIGNAL_TYPE" => SignalType,
        "$NI_ASYNC_ID" => AsyncId,
        "$NI_ASYNC_EXIT_STATUS" => AsyncExitStatus,
        "$DURATION_QUARTER" => DurationQuarter,
        "$DURATION_EIGHTH" => DurationEighth,
        "$DURATION_SIXTEENTH" => DurationSixteenth,
        "$DURATION_QUARTER_TRIPLET" => DurationQuarterTriplet,
        "$DURATION_EIGHTH_TRIPLET" => DurationEighthTriplet,
        "$DURATION_SIXTEENTH_TRIPLET" => DurationSixteenthTriplet,
        "$DURATION_BAR" => DurationBar,
        "$NI_SONG_POSITION" => SongPosition,
        "$SIGNATURE_NUM" => SignatureNum,
        "$SIGNATURE_DENOM" => SignatureDenom,
        "$NI_TRANSPORT_RUNNING" => TransportRunning,
        "$NI_BPM" | "$NI_TEMPO" => Tempo,
        "$CURRENT_SCRIPT_SLOT" => CurrentScriptSlot,
        "$NI_UI_ID" => UiId,
        "$PLAYED_VOICES_TOTAL" => PlayedVoicesTotal,
        "$PLAYED_VOICES_INST" => PlayedVoicesInst,
        "$DISTANCE_BAR_START" => DistanceBarStart,
        "$NUM_GROUPS" => NumGroups,
        "$NUM_ZONES" => NumZones,
        "$NUM_OUTPUT_CHANNELS" => NumOutputChannels,
        "$NI_MOUSE_OVER_CONTROL" => MouseOverControl,
        "$NI_CONTROL_PAR_IDX" => WidgetInteraction(0),
        "$NI_MOUSE_EVENT_TYPE" => WidgetInteraction(5),
        "$NI_DATE_YEAR" => Date(0),
        "$NI_DATE_MONTH" => Date(1),
        "$NI_DATE_DAY" => Date(2),
        "$NI_TIME_HOUR" => Time(0),
        "$NI_TIME_MINUTE" => Time(1),
        "$NI_TIME_SECOND" => Time(2),
        _ => return None,
    })
}

/// Runtime-maintained integer arrays.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SysArray {
    KeyDown,
    Cc,
    CcTouched,
    PolyAt,
    GroupsSelected,
    GroupsAffected,
    KeyDownOct,
    EventPar,
}
impl SysArray {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "%KEY_DOWN" => Self::KeyDown,
            "%CC" => Self::Cc,
            "%CC_TOUCHED" => Self::CcTouched,
            "%POLY_AT" => Self::PolyAt,
            "%GROUPS_SELECTED" => Self::GroupsSelected,
            "%GROUPS_AFFECTED" => Self::GroupsAffected,
            "%KEY_DOWN_OCT" => Self::KeyDownOct,
            "%EVENT_PAR" => Self::EventPar,
            _ => return None,
        })
    }
    pub fn len(self) -> u32 {
        match self {
            Self::KeyDown | Self::PolyAt => 128,
            Self::Cc | Self::CcTouched => 130,
            Self::KeyDownOct => 12,
            Self::GroupsSelected | Self::GroupsAffected => 4096,
            Self::EventPar => 4,
        }
    }
}

pub const VCC_PITCH_BEND: i32 = 128;
pub const VCC_MONO_AT: i32 = 129;
pub const NOT_FOUND: i32 = -1;
pub const ALL_GROUPS: i32 = 0x3FFF_FFFF;
pub const ALL_EVENTS: i32 = 0x3FFF_FFFE;
pub const MARKS_FLAG: i32 = 0x2000_0000;
pub const INST_ICON_ID: i32 = 0x3F00_0001;
pub const INST_WALLPAPER_ID: i32 = 0x3F00_0002;
pub const FIRST_UI_ID: i32 = 32768;
pub const HIDE_WHOLE_CONTROL: i32 = 16;

pub mod event_par {
    pub const VOLUME: i32 = 4;
    pub const TUNE: i32 = 5;
    pub const PAN: i32 = 6;
    pub const NOTE: i32 = 7;
    pub const VELOCITY: i32 = 8;
    pub const ALLOW_GROUP: i32 = 9;
    pub const ZONE_ID: i32 = 10;
    pub const SOURCE: i32 = 11;
    pub const PLAY_POS: i32 = 12;
    pub const MIDI_CHANNEL: i32 = 13;
    pub const MOD_VALUE_ID: i32 = 14;
    pub const REL_VELOCITY: i32 = 15;
    pub const CUSTOM: i32 = 16;
}

pub mod cb {
    pub const INIT: i32 = 0;
    pub const NOTE: i32 = 1;
    pub const RELEASE: i32 = 2;
    pub const CONTROLLER: i32 = 3;
    pub const POLY_AT: i32 = 4;
    pub const UI_CONTROL: i32 = 7;
    pub const LISTENER: i32 = 9;
    pub const PGS_CHANGED: i32 = 10;
    pub const PERSISTENCE_CHANGED: i32 = 11;
    pub const ASYNC_COMPLETE: i32 = 12;
}

pub mod signal {
    pub const TIMER_MS: i32 = 1;
    pub const TIMER_BEAT: i32 = 2;
    pub const TRANSP_START: i32 = 3;
    pub const TRANSP_STOP: i32 = 4;
}

pub fn real_constant(name: &str) -> Option<f64> {
    match name {
        "~NI_MATH_PI" => Some(std::f64::consts::PI),
        "~NI_MATH_E" => Some(std::f64::consts::E),
        _ => None,
    }
}

/// Enumerations scripts index arrays with or compare against. The reference
/// lists each family in order without numbers; positions from 0 are assumed
/// (values recorded in KONTRA v1's catalog).
const VALUED: &[(&str, i32)] = &[
    ("$NI_FILE_TYPE_MIDI", 0),
    ("$NI_FILE_TYPE_AUDIO", 1),
    ("$NI_FILE_TYPE_ARRAY", 2),
    ("$KNOB_UNIT_NONE", 0),
    ("$KNOB_UNIT_DB", 1),
    ("$KNOB_UNIT_HZ", 2),
    ("$KNOB_UNIT_PERCENT", 3),
    ("$KNOB_UNIT_MS", 4),
    ("$KNOB_UNIT_OCT", 5),
    ("$KNOB_UNIT_ST", 6),
    ("$KEY_COLOR_RED", 0),
    ("$KEY_COLOR_ORANGE", 1),
    ("$KEY_COLOR_LIGHT_ORANGE", 2),
    ("$KEY_COLOR_WARM_YELLOW", 3),
    ("$KEY_COLOR_YELLOW", 4),
    ("$KEY_COLOR_LIME", 5),
    ("$KEY_COLOR_GREEN", 6),
    ("$KEY_COLOR_MINT", 7),
    ("$KEY_COLOR_CYAN", 8),
    ("$KEY_COLOR_TURQUOISE", 9),
    ("$KEY_COLOR_BLUE", 10),
    ("$KEY_COLOR_PLUM", 11),
    ("$KEY_COLOR_VIOLET", 12),
    ("$KEY_COLOR_PURPLE", 13),
    ("$KEY_COLOR_MAGENTA", 14),
    ("$KEY_COLOR_FUCHSIA", 15),
    ("$KEY_COLOR_DEFAULT", 16),
    ("$KEY_COLOR_INACTIVE", 17),
    ("$KEY_COLOR_NONE", 18),
    ("$KEY_COLOR_WHITE", 19),
    ("$KEY_COLOR_BLACK", 20),
    ("$NI_KEY_TYPE_DEFAULT", 0),
    ("$NI_KEY_TYPE_CONTROL", 1),
    ("$NI_KEY_TYPE_NONE", 2),
    ("$NI_SYNC_UNIT_ABS", 0),
    ("$NI_SYNC_UNIT_WHOLE", 1),
    ("$NI_SYNC_UNIT_WHOLE_TRIPLET", 2),
    ("$NI_SYNC_UNIT_HALF", 3),
    ("$NI_SYNC_UNIT_HALF_TRIPLET", 4),
    ("$NI_SYNC_UNIT_QUARTER", 5),
    ("$NI_SYNC_UNIT_QUARTER_TRIPLET", 6),
    ("$NI_SYNC_UNIT_8TH", 7),
    ("$NI_SYNC_UNIT_8TH_TRIPLET", 8),
    ("$NI_SYNC_UNIT_16TH", 9),
    ("$NI_SYNC_UNIT_16TH_TRIPLET", 10),
    ("$NI_SYNC_UNIT_32ND", 11),
    ("$NI_SYNC_UNIT_32ND_TRIPLET", 12),
    ("$NI_SYNC_UNIT_64TH", 13),
    ("$NI_SYNC_UNIT_64TH_TRIPLET", 14),
    ("$NI_SYNC_UNIT_256TH", 15),
    ("$NI_SYNC_UNIT_ZONE_LENGTH", 16),
    ("$EVENT_STATUS_INACTIVE", 0),
    ("$EVENT_STATUS_NOTE_QUEUE", 1),
    ("$EVENT_STATUS_MIDI_QUEUE", 2),
    ("$HIDE_PART_NOTHING", 0),
    ("$HIDE_PART_BG", 1),
    ("$HIDE_PART_VALUE", 2),
    ("$HIDE_PART_TITLE", 4),
    ("$HIDE_PART_MOD_LIGHT", 8),
    ("$HIDE_WHOLE_CONTROL", HIDE_WHOLE_CONTROL),
    ("$NI_SEND_BUS", 0),
    ("$NI_INSERT_BUS", 1),
    ("$NI_MAIN_BUS", 2),
    ("$NI_BUS_OFFSET", 1000),
    ("$NI_NOT_FOUND", NOT_FOUND),
    ("$GET_FOLDER_LIBRARY_DIR", 0),
    ("$GET_FOLDER_INSTALL_DIR", 1),
    ("$GET_FOLDER_PATCH_DIR", 2),
    ("$GET_FOLDER_FACTORY_DIR", 3),
    ("$UI_WAVEFORM_USE_SLICES", 1),
    ("$UI_WAVEFORM_USE_TABLE", 2),
    ("$UI_WAVEFORM_TABLE_IS_BIPOLAR", 4),
    ("$UI_WAVEFORM_USE_MIDI_DRAG", 8),
    ("$NI_KONTAKT_IS_HEADLESS", 0),
    ("$NI_KONTAKT_IS_STANDALONE", 0),
    ("$ALL_GROUPS", ALL_GROUPS),
    ("$ALL_EVENTS", ALL_EVENTS),
    ("$VCC_PITCH_BEND", VCC_PITCH_BEND),
    ("$VCC_MONO_AT", VCC_MONO_AT),
    ("$INST_ICON_ID", INST_ICON_ID),
    ("$INST_WALLPAPER_ID", INST_WALLPAPER_ID),
    ("$EVENT_PAR_VOLUME", event_par::VOLUME),
    ("$EVENT_PAR_TUNE", event_par::TUNE),
    ("$EVENT_PAR_PAN", event_par::PAN),
    ("$EVENT_PAR_NOTE", event_par::NOTE),
    ("$EVENT_PAR_VELOCITY", event_par::VELOCITY),
    ("$EVENT_PAR_ALLOW_GROUP", event_par::ALLOW_GROUP),
    ("$EVENT_PAR_ZONE_ID", event_par::ZONE_ID),
    ("$EVENT_PAR_SOURCE", event_par::SOURCE),
    ("$EVENT_PAR_PLAY_POS", event_par::PLAY_POS),
    ("$EVENT_PAR_MIDI_CHANNEL", event_par::MIDI_CHANNEL),
    ("$EVENT_PAR_MOD_VALUE_ID", event_par::MOD_VALUE_ID),
    ("$EVENT_PAR_REL_VELOCITY", event_par::REL_VELOCITY),
    ("$EVENT_PAR_CUSTOM", event_par::CUSTOM),
    ("$NI_CB_TYPE_INIT", cb::INIT),
    ("$NI_CB_TYPE_NOTE", cb::NOTE),
    ("$NI_CB_TYPE_RELEASE", cb::RELEASE),
    ("$NI_CB_TYPE_CONTROLLER", cb::CONTROLLER),
    ("$NI_CB_TYPE_POLY_AT", cb::POLY_AT),
    ("$NI_CB_TYPE_UI_CONTROL", cb::UI_CONTROL),
    ("$NI_CB_TYPE_LISTENER", cb::LISTENER),
    ("$NI_CB_TYPE_PGS", cb::PGS_CHANGED),
    ("$NI_CB_TYPE_PERSISTENCE_CHANGED", cb::PERSISTENCE_CHANGED),
    ("$NI_CB_TYPE_ASYNC_COMPLETE", cb::ASYNC_COMPLETE),
    ("$NI_SIGNAL_TIMER_MS", signal::TIMER_MS),
    ("$NI_SIGNAL_TIMER_BEAT", signal::TIMER_BEAT),
    ("$NI_SIGNAL_TRANSP_START", signal::TRANSP_START),
    ("$NI_SIGNAL_TRANSP_STOP", signal::TRANSP_STOP),
];

/// Constants whose numeric value carries meaning.
pub fn constant(name: &str) -> Option<i32> {
    if let Some(event) = sampler_core::WidgetEventType::ksp_constant(name) {
        return Some(event as i32);
    }
    if let Some(&(_, v)) = VALUED.iter().find(|(n, _)| *n == name) {
        return Some(v);
    }
    if let Some(name) = name.strip_prefix("$NI_CONTROL_TYPE_") {
        return CONTROL_TYPES
            .iter()
            .position(|(_, n)| *n == name)
            .map(|i| i as i32);
    }
    if let Some(n) = name
        .strip_prefix("$MARK_")
        .and_then(|n| n.parse::<u32>().ok())
    {
        return (1..=28).contains(&n).then(|| 1 << (n - 1));
    }
    if let Some(n) = name
        .strip_prefix("$EVENT_PAR_")
        .and_then(|n| n.parse::<i32>().ok())
    {
        return (0..=3).contains(&n).then_some(n);
    }
    None
}

/// Widget keywords in `$NI_CONTROL_TYPE_*` order.
pub const CONTROL_TYPES: &[(&str, &str)] = &[
    ("", "NONE"),
    ("ui_button", "BUTTON"),
    ("ui_knob", "KNOB"),
    ("ui_menu", "MENU"),
    ("ui_value_edit", "VALUE_EDIT"),
    ("ui_label", "LABEL"),
    ("ui_table", "TABLE"),
    ("ui_waveform", "WAVEFORM"),
    ("ui_wavetable", "WAVETABLE"),
    ("ui_slider", "SLIDER"),
    ("ui_text_edit", "TEXT_EDIT"),
    ("ui_file_selector", "FILE_SELECTOR"),
    ("ui_switch", "SWITCH"),
    ("ui_xy", "XY"),
    ("ui_level_meter", "LEVEL_METER"),
    ("ui_mouse_area", "MOUSE_AREA"),
    ("ui_panel", "PANEL"),
];

/// Opaque vendor symbols: only identity matters. Control parameters come first
/// because the UI model interprets them; any other undeclared uppercase name is
/// interned after them by the resolver and reported.
pub const SYMBOL_BASE: i32 = 0x0100_0000;
pub const CONTROL_PARS: &[&str] = &[
    "$CONTROL_PAR_VALUE",
    "$CONTROL_PAR_POS_X",
    "$CONTROL_PAR_POS_Y",
    "$CONTROL_PAR_WIDTH",
    "$CONTROL_PAR_HEIGHT",
    "$CONTROL_PAR_HIDE",
    "$CONTROL_PAR_TEXT",
    "$CONTROL_PAR_LABEL",
    "$CONTROL_PAR_HELP",
    "$CONTROL_PAR_UNIT",
    "$CONTROL_PAR_MIN_VALUE",
    "$CONTROL_PAR_MAX_VALUE",
    "$CONTROL_PAR_PICTURE",
    "$CONTROL_PAR_DEFAULT_VALUE",
    "$CONTROL_PAR_SELECTED_ITEM_IDX",
    "$CONTROL_PAR_NUM_ITEMS",
    "$CONTROL_PAR_ALLOW_AUTOMATION",
    "$CONTROL_PAR_AUTOMATION_ID",
    "$CONTROL_PAR_AUTOMATION_NAME",
    "$CONTROL_PAR_PICTURE_STATE",
    "$CONTROL_PAR_TEXTLINE",
    "$CONTROL_PAR_TYPE",
    "$CONTROL_PAR_KEY_SHIFT",
    "$CONTROL_PAR_KEY_ALT",
    "$CONTROL_PAR_KEY_CONTROL",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR",
    "$CONTROL_PAR_SHORT_NAME",
    "$CONTROL_PAR_IDENTIFIER",
    "$CONTROL_PAR_BASEPATH",
    "$CONTROL_PAR_FILEPATH",
    // Appended: numbering above is stable.
    "$CONTROL_PAR_TEXTPOS_Y",
    "$CONTROL_PAR_SHOW_ARROWS",
    "$CONTROL_PAR_CURSOR_PICTURE",
    "$CONTROL_PAR_BG_COLOR",
    "$CONTROL_PAR_ON_COLOR",
    "$CONTROL_PAR_OFF_COLOR",
    "$CONTROL_PAR_BAR_COLOR",
    "$CONTROL_PAR_PEAK_COLOR",
    "$CONTROL_PAR_OVERLOAD_COLOR",
    "$CONTROL_PAR_ZERO_LINE_COLOR",
    "$CONTROL_PAR_VERTICAL",
];

pub fn control_par(name: &str) -> Option<i32> {
    CONTROL_PARS
        .iter()
        .position(|n| *n == name)
        .map(|i| SYMBOL_BASE + i as i32)
}
pub const CONTROL_PAR_VALUE: i32 = SYMBOL_BASE;
pub const CONTROL_PAR_POS_X: i32 = SYMBOL_BASE + 1;
pub const CONTROL_PAR_POS_Y: i32 = SYMBOL_BASE + 2;
pub const CONTROL_PAR_HIDE: i32 = SYMBOL_BASE + 5;
pub const CONTROL_PAR_TEXT: i32 = SYMBOL_BASE + 6;
pub const CONTROL_PAR_LABEL: i32 = SYMBOL_BASE + 7;
pub const CONTROL_PAR_HELP: i32 = SYMBOL_BASE + 8;
pub const CONTROL_PAR_UNIT: i32 = SYMBOL_BASE + 9;
pub const CONTROL_PAR_MIN_VALUE: i32 = SYMBOL_BASE + 10;
pub const CONTROL_PAR_MAX_VALUE: i32 = SYMBOL_BASE + 11;
pub const CONTROL_PAR_DEFAULT_VALUE: i32 = SYMBOL_BASE + 13;
pub const CONTROL_PAR_NUM_ITEMS: i32 = SYMBOL_BASE + 15;
pub const CONTROL_PAR_TYPE: i32 = SYMBOL_BASE + 21;
pub const CONTROL_PAR_PICTURE: i32 = SYMBOL_BASE + 12;
pub const CONTROL_PAR_PARENT_PANEL: i32 = SYMBOL_BASE + 28;
