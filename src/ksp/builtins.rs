//! Static KSP surface: builtin signatures, built-in constants and system variables.
//! Everything here is resolved at compile time; the VM only sees enum values.

/// Compile-time argument kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Integer expression.
    I,
    /// Real expression.
    R,
    /// Any expression, converted to text.
    S,
    /// Integer or real; every `N` in one call must share a type.
    N,
    /// A whole variable (scalar or array), passed by reference.
    V,
    /// Any array variable, passed by reference.
    A,
    /// A bare identifier or string literal naming a key.
    K,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    Void,
    Int,
    Real,
    Str,
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
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Builtin { $($id),* }
        impl Builtin {
            fn lookup(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$id),)* _ => None }
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
        Self::lookup(name).or_else(|| match name {
            // Kontakt 2's underscore spellings (`_set_engine_par`, `_pgs_key_exists`, ...).
            _ => Self::lookup(name.strip_prefix('_').filter(|n| !n.starts_with(['_', '#']))?),
        })
    }
}

builtins! {
    // Arithmetic.
    Abs "abs" [N] 0 Num;
    Min "min" [N N] 0 Num;
    Max "max" [N N] 0 Num;
    InRange "in_range" [N N N] 0 Int;
    // Real forms of the `N` builtins above; selected by the compiler, not callable by name.
    AbsReal "#abs" [R] 0 Real;
    MinReal "#min" [R R] 0 Real;
    MaxReal "#max" [R R] 0 Real;
    InRangeReal "#in_range" [R R R] 0 Int;
    Sgn "sgn" [N] 0 Int;
    Signbit "signbit" [N] 0 Int;
    SgnReal "#sgn" [R] 0 Int;
    SignbitReal "#signbit" [R] 0 Int;
    Exp2 "exp2" [R] 0 Real;
    Cbrt "cbrt" [R] 0 Real;
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
    Exp "exp" [R] 0 Real;
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
    ArrayEqual "array_equal" [A A] 0 Int;
    LoadArray "load_array" [A I] 0 Int;
    SaveArray "save_array" [A I] 0 Int;
    LoadArrayStr "load_array_str" [A S] 0 Int;
    SaveArrayStr "save_array_str" [A S] 0 Int;
    // Events.
    PlayNote "play_note" [I I I I] 0 Int;
    NoteOff "note_off" [I I] 1 Void;
    IgnoreEvent "ignore_event" [I] 1 Void;
    ChangeVol "change_vol" [I I I] 1 Void;
    ChangeTune "change_tune" [I I I] 1 Void;
    ChangePan "change_pan" [I I I] 1 Void;
    ChangeVelo "change_velo" [I I] 0 Void;
    ChangeNote "change_note" [I I] 0 Void;
    FadeIn "fade_in" [I I I] 1 Void;
    FadeOut "fade_out" [I I I I] 2 Void;
    SetEventPar "set_event_par" [I I I] 0 Void;
    GetEventPar "get_event_par" [I I] 0 Int;
    SetEventParArr "set_event_par_arr" [I I I I] 0 Void;
    // Intrinsic keeps LHS index evaluation before the assignment value.
    SetEventParIndexed "#set_event_par_indexed" [I I I] 0 Void;
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
    SetMapEditorEventColor "set_map_editor_event_color" [I] 0 Void;
    Exit "exit" [] 0 Void;
    // Time.
    Wait "wait" [I] 0 Void;
    WaitTicks "wait_ticks" [I] 0 Void;
    WaitAsync "wait_async" [I] 0 Void;
    StopWait "stop_wait" [I I] 0 Void;
    ResetKspTimer "reset_ksp_timer" [] 0 Void;
    SetListener "set_listener" [I I] 0 Void;
    ChangeListenerPar "change_listener_par" [I I] 0 Void;
    // Groups, modules and engine parameters.
    FindGroup "find_group" [S] 0 Int;
    GetGroupIdx "get_group_idx" [S] 0 Int;
    GroupName "group_name" [I] 0 Str;
    GetNumZones "get_num_zones" [] 0 Int;
    GetZoneId "get_zone_id" [I] 0 Int;
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
    // User interface.
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
    GetUiWfProperty "get_ui_wf_property" [V I I] 0 Int;
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
    SetSnapshotType "set_snapshot_type" [I] 0 Void;
    ShowLibraryTab "show_library_tab" [] 0 Void;
    SetUiWfProperty "set_ui_wf_property" [V I I I] 0 Void;
    GetFontId "get_font_id" [S] 0 Int;
    GetFolder "get_folder" [I] 0 Str;
    FsGetFilename "fs_get_filename" [I I] 0 Str;
    FsNavigate "fs_navigate" [I I] 0 Void;
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
    GetKeyrangeMinNote "get_keyrange_min_note" [I] 0 Int;
    GetKeyrangeMaxNote "get_keyrange_max_note" [I] 0 Int;
    GetKeyrangeName "get_keyrange_name" [I] 0 Str;
    // Diagnostics and preprocessor leftovers.
    Message "message" [S] 0 Void;
    DisableLogging "disable_logging" [I] 0 Void;
    WatchVar "watch_var" [V] 0 Void;
    WatchArrayIdx "watch_array_idx" [A I] 0 Void;
    SetCondition "SET_CONDITION" [K] 0 Void;
    ResetCondition "RESET_CONDITION" [K] 0 Void;
    // Persistence.
    MakePersistent "make_persistent" [V] 0 Void;
    MakeInstrPersistent "make_instr_persistent" [V] 0 Void;
    ReadPersistentVar "read_persistent_var" [V] 0 Void;
    // Program global storage.
    PgsCreateKey "pgs_create_key" [K I] 0 Void;
    PgsKeyExists "pgs_key_exists" [K] 0 Int;
    PgsSetKeyVal "pgs_set_key_val" [K I I] 0 Void;
    PgsGetKeyVal "pgs_get_key_val" [K I] 0 Int;
    PgsCreateStrKey "pgs_create_str_key" [K] 0 Void;
    PgsStrKeyExists "pgs_str_key_exists" [K] 0 Int;
    PgsSetStrKeyVal "pgs_set_str_key_val" [K S] 0 Void;
    PgsGetStrKeyVal "pgs_get_str_key_val" [K] 0 Str;
}

/// Script-visible scalars owned by the runtime, read through the current callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysVar {
    EventId,
    EventNote,
    EventVelocity,
    NoteHeld,
    CcNum,
    PitchBend,
    PolyAtNum,
    NcNum,
    NcNote,
    NcValue,
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
    TransportRunning,
    Tempo,
    CurrentScriptSlot,
    UiId,
    PlayedVoices,
    DistanceBarStart,
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
        "$NC_NUM" => NcNum,
        "$NC_NOTE" => NcNote,
        "$NC_VALUE" => NcValue,
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
        "$NI_TRANSPORT_RUNNING" => TransportRunning,
        "$NI_BPM" | "$NI_TEMPO" => Tempo,
        "$CURRENT_SCRIPT_SLOT" => CurrentScriptSlot,
        "$NI_UI_ID" => UiId,
        "$PLAYED_VOICES_TOTAL" | "$PLAYED_VOICES_INST" => PlayedVoices,
        "$DISTANCE_BAR_START" => DistanceBarStart,
        "$NI_DATE_YEAR" => Date(0),
        "$NI_DATE_MONTH" => Date(1),
        "$NI_DATE_DAY" => Date(2),
        "$NI_TIME_HOUR" => Time(0),
        "$NI_TIME_MINUTE" => Time(1),
        "$NI_TIME_SECOND" => Time(2),
        _ => return None,
    })
}

/// Runtime-maintained integer arrays, stored in script memory and updated on input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysArray {
    KeyDown,
    Cc,
    CcTouched,
    PolyAt,
    /// Edit-mode group selection; nothing is selected in a player.
    GroupsSelected,
    /// Held keys per pitch class, C first.
    KeyDownOct,
}

impl SysArray {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "%KEY_DOWN" => Self::KeyDown,
            "%CC" => Self::Cc,
            "%CC_TOUCHED" => Self::CcTouched,
            "%POLY_AT" => Self::PolyAt,
            "%GROUPS_SELECTED" => Self::GroupsSelected,
            "%KEY_DOWN_OCT" => Self::KeyDownOct,
            _ => return None,
        })
    }

    pub fn len(self, groups: usize) -> u32 {
        match self {
            Self::GroupsSelected => groups.clamp(1, 4096) as u32,
            Self::KeyDown | Self::PolyAt => 128,
            Self::Cc | Self::CcTouched => CC_SLOTS as u32,
            Self::KeyDownOct => 12,
        }
    }
}

/// `%CC` covers 128 controllers plus the virtual pitch bend and aftertouch slots.
pub const CC_SLOTS: usize = 130;
pub const VCC_PITCH_BEND: i32 = 128;
pub const VCC_MONO_AT: i32 = 129;
/// Distinct from the 512 registered/assignable per-note controller numbers.
pub const VNC_PITCH_BEND: i32 = 512;

/// `get_folder` arguments. Kontakt's values are not published; only
/// `get_folder` reads them.
pub const GET_FOLDER_LIBRARY_DIR: i32 = 0;
pub const GET_FOLDER_INSTALL_DIR: i32 = 1;
pub const GET_FOLDER_PATCH_DIR: i32 = 2;
pub const GET_FOLDER_FACTORY_DIR: i32 = 3;
/// `$NI_NOT_FOUND`, what the `get_*_idx` commands return for a miss.
pub const NOT_FOUND: i32 = -1;
pub const ALL_GROUPS: i32 = 0x3FFF_FFFF;
pub const ALL_EVENTS: i32 = 0x3FFF_FFFE;
/// `by_marks` results carry this flag; plain event IDs never do.
pub const MARKS_FLAG: i32 = 0x2000_0000;
pub const INST_ICON_ID: i32 = 0x3F00_0001;
pub const INST_WALLPAPER_ID: i32 = 0x3F00_0002;
/// The library tab's two pictures, copyright and description (Kontakt 7).
pub const INST_LIB_LAST_ID: i32 = 0x3F00_0006;

/// Instrument-level pseudo controls: the icon, wallpaper and library tab.
/// Scripts style them like controls; only the wallpaper is used here.
pub fn instrument_control(id: i32) -> bool {
    (INST_ICON_ID..=INST_LIB_LAST_ID).contains(&id)
}
pub const FIRST_UI_ID: i32 = 32768;
/// Time Machine Pro voice types for `get_voice_limit`/`set_voice_limit`.
pub const VL_TMPRO_STANDARD: i32 = 0;
pub const VL_TMPRO_HQ: i32 = 1;
pub const HIDE_WHOLE_CONTROL: i32 = 16;

pub mod event_par {
    pub const PAR_0: i32 = 0;
    pub const PAR_3: i32 = 3;
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
    pub const RPN: i32 = 5;
    pub const NRPN: i32 = 6;
    pub const UI_CONTROL: i32 = 7;
    pub const UI_UPDATE: i32 = 8;
    pub const LISTENER: i32 = 9;
    pub const PGS_CHANGED: i32 = 10;
    pub const PERSISTENCE_CHANGED: i32 = 11;
    pub const ASYNC_COMPLETE: i32 = 12;
    pub const UI_CONTROLS: i32 = 13;
    pub const NOTE_CONTROLLER: i32 = 14;
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

/// Enumerations scripts index arrays with or compare against, so they need Kontakt's
/// small values rather than opaque ones. The KSP reference lists each family in
/// this order without numbers; positions from 0 are assumed. The filter types are
/// the type ids NKIs store (`audits/EFFECTS.md`); the AR, Daft, phaser and
/// formant ones are in `engine::filter::ksp_filter_type`.
const VALUED: &[(&str, i32)] = &[
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
    ("$FILTER_TYPE_LP1POLE", 0),
    ("$FILTER_TYPE_HP1POLE", 1),
    ("$FILTER_TYPE_LP2POLE", 2),
    ("$FILTER_TYPE_HP2POLE", 3),
    ("$FILTER_TYPE_BP2POLE", 4),
    ("$FILTER_TYPE_LP4POLE", 5),
    ("$FILTER_TYPE_HP4POLE", 6),
    ("$FILTER_TYPE_BP4POLE", 7),
    ("$FILTER_TYPE_BR4POLE", 8),
    ("$FILTER_TYPE_LP6POLE", 9),
    // Other `$FILTER_TYPE_*` values: `engine::filter::ksp_filter_type`.
    // Time units for `*_TIME_UNIT` / `*_FREQ_UNIT` engine parameters.
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
];

/// The name in a `VALUED` family (`"$KEY_COLOR_"`) that has this value.
pub fn named(family: &str, value: i32) -> Option<&'static str> {
    VALUED
        .iter()
        .find(|(n, v)| *v == value && n.starts_with(family))
        .map(|(n, _)| *n)
}

/// Constants whose numeric value carries meaning.
pub fn constant(name: &str) -> Option<i32> {
    if let Some(name) = name.strip_prefix("$NI_CONTROL_TYPE_") {
        return CONTROL_TYPES.iter().position(|(_, n)| *n == name).map(|i| i as i32);
    }
    if let Some(&(_, v)) = VALUED.iter().find(|(n, _)| *n == name) {
        return Some(v);
    }
    if let Some(v) = crate::fx::ksp_effect_type(name) {
        return Some(v);
    }
    if let Some(v) = crate::engine::filter::ksp_filter_type(name) {
        return Some(v);
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
    Some(match name {
        "$EVENT_PAR_VOLUME" => event_par::VOLUME,
        "$EVENT_PAR_TUNE" => event_par::TUNE,
        "$EVENT_PAR_PAN" => event_par::PAN,
        "$EVENT_PAR_NOTE" => event_par::NOTE,
        "$EVENT_PAR_VELOCITY" => event_par::VELOCITY,
        "$EVENT_PAR_ALLOW_GROUP" => event_par::ALLOW_GROUP,
        "$EVENT_PAR_ZONE_ID" => event_par::ZONE_ID,
        "$EVENT_PAR_SOURCE" => event_par::SOURCE,
        "$EVENT_PAR_PLAY_POS" => event_par::PLAY_POS,
        "$EVENT_PAR_MIDI_CHANNEL" => event_par::MIDI_CHANNEL,
        "$EVENT_PAR_MOD_VALUE_ID" => event_par::MOD_VALUE_ID,
        "$EVENT_PAR_REL_VELOCITY" => event_par::REL_VELOCITY,
        "$EVENT_PAR_CUSTOM" => event_par::CUSTOM,
        "$EVENT_STATUS_INACTIVE" => 0,
        "$EVENT_STATUS_NOTE_QUEUE" => 1,
        "$EVENT_STATUS_MIDI_QUEUE" => 2,
        "$ALL_GROUPS" => ALL_GROUPS,
        "$ALL_EVENTS" => ALL_EVENTS,
        "$VCC_PITCH_BEND" => VCC_PITCH_BEND,
        "$VNC_PITCH_BEND" => VNC_PITCH_BEND,
        "$VCC_MONO_AT" => VCC_MONO_AT,
        "$INST_ICON_ID" => INST_ICON_ID,
        "$INST_WALLPAPER_ID" => INST_WALLPAPER_ID,
        "$INST_LIB_PIC_ONE_ID" => INST_WALLPAPER_ID + 1,
        "$INST_LIB_PIC_TWO_ID" => INST_WALLPAPER_ID + 2,
        "$INST_LIB_COPYRIGHT_ID" => INST_WALLPAPER_ID + 3,
        "$INST_LIB_DESCRIPTION_ID" => INST_LIB_LAST_ID,
        // Kontakt's own bits: Una Corda hides its tab pages with a literal 16.
        "$HIDE_PART_NOTHING" => 0,
        "$HIDE_PART_BG" => 1,
        "$HIDE_PART_VALUE" => 2,
        "$HIDE_PART_TITLE" => 4,
        "$HIDE_PART_MOD_LIGHT" => 8,
        "$HIDE_WHOLE_CONTROL" => HIDE_WHOLE_CONTROL,
        "$NI_CB_TYPE_INIT" => cb::INIT,
        "$NI_CB_TYPE_NOTE" => cb::NOTE,
        "$NI_CB_TYPE_RELEASE" => cb::RELEASE,
        "$NI_CB_TYPE_CONTROLLER" => cb::CONTROLLER,
        "$NI_CB_TYPE_NOTE_CONTROLLER" => cb::NOTE_CONTROLLER,
        "$NI_CB_TYPE_POLY_AT" => cb::POLY_AT,
        "$NI_CB_TYPE_RPN" => cb::RPN,
        "$NI_CB_TYPE_NRPN" => cb::NRPN,
        "$NI_CB_TYPE_UI_CONTROL" => cb::UI_CONTROL,
        "$NI_CB_TYPE_UI_UPDATE" => cb::UI_UPDATE,
        "$NI_CB_TYPE_LISTENER" => cb::LISTENER,
        "$NI_CB_TYPE_PGS" => cb::PGS_CHANGED,
        "$NI_CB_TYPE_PERSISTENCE_CHANGED" => cb::PERSISTENCE_CHANGED,
        "$NI_CB_TYPE_ASYNC_COMPLETE" => cb::ASYNC_COMPLETE,
        "$NI_CB_TYPE_UI_CONTROLS" => cb::UI_CONTROLS,
        "$NI_SIGNAL_TIMER_MS" => signal::TIMER_MS,
        "$NI_SIGNAL_TIMER_BEAT" => signal::TIMER_BEAT,
        "$NI_SIGNAL_TRANSP_START" => signal::TRANSP_START,
        "$NI_SIGNAL_TRANSP_STOP" => signal::TRANSP_STOP,
        "$NI_SEND_BUS" => 0,
        "$NI_INSERT_BUS" => 1,
        "$NI_MAIN_BUS" => 2,
        "$NI_BUS_OFFSET" => 1000,
        "$NI_NOT_FOUND" => NOT_FOUND,
        "$GET_FOLDER_LIBRARY_DIR" => GET_FOLDER_LIBRARY_DIR,
        "$GET_FOLDER_INSTALL_DIR" => GET_FOLDER_INSTALL_DIR,
        "$GET_FOLDER_PATCH_DIR" => GET_FOLDER_PATCH_DIR,
        "$GET_FOLDER_FACTORY_DIR" => GET_FOLDER_FACTORY_DIR,
        // `$ENGINE_PAR_RV2_TYPE` values: Reverb's room/hall switch.
        "$NI_REVERB2_TYPE_ROOM" => 0,
        "$NI_REVERB2_TYPE_HALL" => 1,
        // `attach_zone` flags, or'd together.
        "$UI_WAVEFORM_USE_SLICES" => 1,
        "$UI_WAVEFORM_USE_TABLE" => 2,
        "$UI_WAVEFORM_TABLE_IS_BIPOLAR" => 4,
        "$UI_WAVEFORM_USE_MIDI_DRAG" => 8,
        "$NI_VL_TMPRO_STANDARD" => VL_TMPRO_STANDARD,
        "$NI_VL_TMPRO_HQ" | "$NI_VL_TMRPO_HQ" => VL_TMPRO_HQ,
        // A plugin with an editor, in 4/4 unless the host says otherwise.
        "$NI_KONTAKT_IS_HEADLESS" | "$NI_KONTAKT_IS_STANDALONE" => 0,
        "$SIGNATURE_NUM" | "$SIGNATURE_DENOM" => 4,
        _ => return None,
    })
}

/// Symbolic constants: only identity matters, so values are table positions.
/// The first entries are the control parameters the runtime interprets.
pub const SYMBOL_BASE: i32 = 0x0100_0000;
pub const SYMBOLS: &[&str] = &[
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
    "$CONTROL_PAR_BAR_COLOR",
    "$CONTROL_PAR_BASEPATH",
    "$CONTROL_PAR_BG_COLOR",
    "$CONTROL_PAR_COLUMN_WIDTH",
    "$CONTROL_PAR_FILE_TYPE",
    "$CONTROL_PAR_FONT_TYPE",
    "$CONTROL_PAR_KEY_ALT",
    "$CONTROL_PAR_KEY_CONTROL",
    "$CONTROL_PAR_KEY_SHIFT",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR",
    "$CONTROL_PAR_PICTURE_STATE",
    "$CONTROL_PAR_SHOW_ARROWS",
    "$CONTROL_PAR_TEXTPOS_Y",
    "$CONTROL_PAR_TEXT_ALIGNMENT",
    "$CONTROL_PAR_Z_LAYER",
    "$CONTROL_PAR_GRID_WIDTH",
    "$CONTROL_PAR_GRID_HEIGHT",
    "$CONTROL_PAR_ZERO_LINE_COLOR",
    "$CONTROL_PAR_TEXTLINE",
    "$CONTROL_PAR_TEXTPOS_X",
    "$CONTROL_PAR_TEXT_COLOR",
    "$CONTROL_PAR_FONT_TYPE_ON",
    "$CONTROL_PAR_FONT_TYPE_OFF_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_ON_PRESSED",
    "$CONTROL_PAR_FONT_TYPE_OFF_HOVER",
    "$CONTROL_PAR_FONT_TYPE_ON_HOVER",
    "$CONTROL_PAR_ACTIVE_INDEX",
    "$CONTROL_PAR_DND_BEHAVIOUR",
    "$CONTROL_PAR_MOUSE_MODE",
    "$CONTROL_PAR_MOVABLE",
    "$CONTROL_PAR_PARALLAX_X",
    "$CONTROL_PAR_PARALLAX_Y",
    "$CONTROL_PAR_VALUE_POS_X",
    "$CONTROL_PAR_VALUE_POS_Y",
    "$CONTROL_PAR_CURSOR_PICTURE",
    "$CONTROL_PAR_HEADER",
    "$CONTROL_PAR_WAVE_COLOR",
    "$CONTROL_PAR_WAVE_CURSOR_COLOR",
    "$CONTROL_PAR_WAVE_END_COLOR",
    "$CONTROL_PAR_WAVE_END_ALPHA",
    "$CONTROL_PAR_WAVE_ALPHA",
    "$CONTROL_PAR_SLICEMARKERS_COLOR",
    "$CONTROL_PAR_BG_ALPHA",
    "$CONTROL_PAR_CUSTOM_ID",
    "$CONTROL_PAR_TYPE",
    "$CONTROL_PAR_IDENTIFIER",
    "$CONTROL_PAR_NONE",
    "$CONTROL_PAR_SHORT_NAME",
    // Level meters (`attach_level_meter`) and the rest of Kontakt 7's control set.
    "$CONTROL_PAR_OFF_COLOR",
    "$CONTROL_PAR_ON_COLOR",
    "$CONTROL_PAR_OVERLOAD_COLOR",
    "$CONTROL_PAR_PEAK_COLOR",
    "$CONTROL_PAR_VERTICAL",
    "$CONTROL_PAR_RANGE_MIN",
    "$CONTROL_PAR_RANGE_MAX",
    "$CONTROL_PAR_PARENT_PANEL",
    "$CONTROL_PAR_VALUEPOS_Y",
    "$CONTROL_PAR_WF_VIS_MODE",
    "$CONTROL_PAR_WAVETABLE_COLOR",
    "$CONTROL_PAR_WAVETABLE_ALPHA",
    "$CONTROL_PAR_DISABLE_TEXT_SHIFTING",
    "$CONTROL_PAR_RECEIVE_DRAG_EVENTS",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_X",
    "$CONTROL_PAR_MOUSE_BEHAVIOUR_Y",
    // `ui_waveform` properties (`set_ui_wf_property`), kept on the control.
    "$UI_WF_PROP_PLAY_CURSOR",
    "$UI_WF_PROP_FLAGS",
    "$UI_WF_PROP_TABLE_VAL",
    "$UI_WF_PROP_TABLE_IDX_HIGHLIGHT",
    "$UI_WF_PROP_MIDI_DRAG_START_NOTE",
    // The zone `attach_zone` showed in a waveform: no name a script can write.
    "attached zone",
];

pub const CONTROL_PAR_VALUE: i32 = SYMBOL_BASE;
pub const CONTROL_PAR_POS_X: i32 = SYMBOL_BASE + 1;
pub const CONTROL_PAR_POS_Y: i32 = SYMBOL_BASE + 2;
pub const CONTROL_PAR_WIDTH: i32 = SYMBOL_BASE + 3;
pub const CONTROL_PAR_HEIGHT: i32 = SYMBOL_BASE + 4;
pub const CONTROL_PAR_HIDE: i32 = SYMBOL_BASE + 5;
pub const CONTROL_PAR_TEXT: i32 = SYMBOL_BASE + 6;
pub const CONTROL_PAR_LABEL: i32 = SYMBOL_BASE + 7;
pub const CONTROL_PAR_HELP: i32 = SYMBOL_BASE + 8;
pub const CONTROL_PAR_UNIT: i32 = SYMBOL_BASE + 9;
pub const CONTROL_PAR_MIN_VALUE: i32 = SYMBOL_BASE + 10;
pub const CONTROL_PAR_MAX_VALUE: i32 = SYMBOL_BASE + 11;
pub const CONTROL_PAR_PICTURE: i32 = SYMBOL_BASE + 12;
pub const CONTROL_PAR_DEFAULT_VALUE: i32 = SYMBOL_BASE + 13;
pub const CONTROL_PAR_PICTURE_STATE: i32 = SYMBOL_BASE + 29;
pub const CONTROL_PAR_SELECTED_ITEM_IDX: i32 = SYMBOL_BASE + 14;
pub const CONTROL_PAR_NUM_ITEMS: i32 = SYMBOL_BASE + 15;
pub const CONTROL_PAR_TEXTLINE: i32 = SYMBOL_BASE + 37;
pub const CONTROL_PAR_TYPE: i32 = SYMBOL_BASE + 63;
pub const CONTROL_PAR_IDENTIFIER: i32 = SYMBOL_BASE + 64;
pub const CONTROL_PAR_NONE: i32 = SYMBOL_BASE + 65;

const CONTROL_TYPES: &[(&str, &str)] = &[
    ("", "NONE"), ("ui_button", "BUTTON"), ("ui_knob", "KNOB"),
    ("ui_menu", "MENU"), ("ui_value_edit", "VALUE_EDIT"), ("ui_label", "LABEL"),
    ("ui_table", "TABLE"), ("ui_waveform", "WAVEFORM"), ("ui_wavetable", "WAVETABLE"),
    ("ui_slider", "SLIDER"), ("ui_text_edit", "TEXT_EDIT"),
    ("ui_file_selector", "FILE_SELECTOR"), ("ui_switch", "SWITCH"),
    ("ui_xy", "XY"), ("ui_level_meter", "LEVEL_METER"),
    ("ui_mouse_area", "MOUSE_AREA"), ("ui_panel", "PANEL"),
];

pub fn control_type(kind: &str) -> i32 {
    CONTROL_TYPES.iter().position(|(k, _)| *k == kind).unwrap_or(0) as i32
}
pub const UI_WF_PROP_FLAGS: i32 = SYMBOL_BASE + SYMBOLS.len() as i32 - 5;
pub const ATTACHED_ZONE: i32 = SYMBOL_BASE + SYMBOLS.len() as i32 - 1;

/// Engine parameters are published with stable IDs so an engine can map them once.
pub const ENGINE_PAR_BASE: i32 = 0x0200_0000;
pub const ENGINE_PARS: &[&str] = &[
    "$ENGINE_PAR_VOLUME",
    "$ENGINE_PAR_PAN",
    "$ENGINE_PAR_TUNE",
    "$ENGINE_PAR_OUTPUT_CHANNEL",
    "$ENGINE_PAR_CUTOFF",
    "$ENGINE_PAR_RESONANCE",
    "$ENGINE_PAR_ATTACK",
    "$ENGINE_PAR_DECAY",
    "$ENGINE_PAR_SUSTAIN",
    "$ENGINE_PAR_RELEASE",
    "$ENGINE_PAR_HOLD",
    "$ENGINE_PAR_ATK_CURVE",
    "$ENGINE_PAR_DECAY1",
    "$ENGINE_PAR_DECAY2",
    "$ENGINE_PAR_BREAK",
    "$ENGINE_PAR_SLOPE",
    "$ENGINE_PAR_MOD_TARGET_INTENSITY",
    "$ENGINE_PAR_MOD_TARGET_MP_INTENSITY",
    "$ENGINE_PAR_INTMOD_INTENSITY",
    "$ENGINE_PAR_INTMOD_BYPASS",
    "$ENGINE_PAR_LFO_FREQ",
    "$ENGINE_PAR_LFO_DELAY",
    "$ENGINE_PAR_EFFECT_BYPASS",
    "$ENGINE_PAR_EFFECT_TYPE",
    "$ENGINE_PAR_EFFECT_SUBTYPE",
    "$ENGINE_PAR_SEND_EFFECT_TYPE",
    "$ENGINE_PAR_SEND_EFFECT_BYPASS",
    "$ENGINE_PAR_SEND_EFFECT_DRY_LEVEL",
    "$ENGINE_PAR_SEND_EFFECT_OUTPUT_GAIN",
    "$ENGINE_PAR_INSERT_EFFECT_OUTPUT_GAIN",
    "$ENGINE_PAR_SENDLEVEL_0",
    "$ENGINE_PAR_SENDLEVEL_1",
    "$ENGINE_PAR_SENDLEVEL_2",
    "$ENGINE_PAR_SENDLEVEL_3",
    "$ENGINE_PAR_SENDLEVEL_4",
    "$ENGINE_PAR_SENDLEVEL_5",
    "$ENGINE_PAR_SENDLEVEL_6",
    "$ENGINE_PAR_SENDLEVEL_7",
    "$ENGINE_PAR_BITS",
    "$ENGINE_PAR_CH_DEPTH",
    "$ENGINE_PAR_CH_PHASE",
    "$ENGINE_PAR_CH_SPEED",
    "$ENGINE_PAR_CH_SPEED_UNIT",
    "$ENGINE_PAR_COMP_ATTACK",
    "$ENGINE_PAR_COMP_DECAY",
    "$ENGINE_PAR_DAMPING",
    "$ENGINE_PAR_DL_DAMPING",
    "$ENGINE_PAR_DL_FEEDBACK",
    "$ENGINE_PAR_DL_PAN",
    "$ENGINE_PAR_DL_TIME",
    "$ENGINE_PAR_DL_TIME_UNIT",
    "$ENGINE_PAR_DRIVE",
    "$ENGINE_PAR_FCOMP_ATTACK",
    "$ENGINE_PAR_FCOMP_INPUT",
    "$ENGINE_PAR_FCOMP_MAKEUP",
    "$ENGINE_PAR_FCOMP_MIX",
    "$ENGINE_PAR_FCOMP_RATIO",
    "$ENGINE_PAR_FCOMP_RELEASE",
    "$ENGINE_PAR_FILTER_BYPA",
    "$ENGINE_PAR_FILTER_BYPB",
    "$ENGINE_PAR_FILTER_BYPC",
    "$ENGINE_PAR_FILTER_GAIN",
    "$ENGINE_PAR_FILTER_RESB",
    "$ENGINE_PAR_FILTER_RESC",
    "$ENGINE_PAR_FILTER_SHIFTB",
    "$ENGINE_PAR_FILTER_SHIFTC",
    "$ENGINE_PAR_FILTER_TYPEA",
    "$ENGINE_PAR_FILTER_TYPEB",
    "$ENGINE_PAR_FILTER_TYPEC",
    "$ENGINE_PAR_FL_COLOR",
    "$ENGINE_PAR_FL_DEPTH",
    "$ENGINE_PAR_FL_FEEDBACK",
    "$ENGINE_PAR_FL_PHASE",
    "$ENGINE_PAR_FL_SPEED",
    "$ENGINE_PAR_FL_SPEED_UNIT",
    "$ENGINE_PAR_FREQUENCY",
    "$ENGINE_PAR_GN_GAIN",
    "$ENGINE_PAR_IRC_FREQ_HIGHPASS_ER",
    "$ENGINE_PAR_IRC_FREQ_HIGHPASS_LR",
    "$ENGINE_PAR_IRC_FREQ_LOWPASS_ER",
    "$ENGINE_PAR_IRC_FREQ_LOWPASS_LR",
    "$ENGINE_PAR_IRC_LENGTH_RATIO_ER",
    "$ENGINE_PAR_IRC_LENGTH_RATIO_LR",
    "$ENGINE_PAR_IRC_PREDELAY",
    "$ENGINE_PAR_IRC_REVERSE",
    "$ENGINE_PAR_IRC_ER_LR_BOUNDARY",
    "$ENGINE_PAR_IRC_AUTO_GAIN",
    "$ENGINE_PAR_NOISECOLOR",
    "$ENGINE_PAR_NOISELEVEL",
    "$ENGINE_PAR_PH_DEPTH",
    "$ENGINE_PAR_PH_FEEDBACK",
    "$ENGINE_PAR_PH_PHASE",
    "$ENGINE_PAR_PH_SPEED",
    "$ENGINE_PAR_PH_SPEED_UNIT",
    "$ENGINE_PAR_RATIO",
    "$ENGINE_PAR_RT_ACCEL_HI",
    "$ENGINE_PAR_RT_ACCEL_LO",
    "$ENGINE_PAR_RT_BALANCE",
    "$ENGINE_PAR_RT_DISTANCE",
    "$ENGINE_PAR_RT_MIX",
    "$ENGINE_PAR_RT_SPEED",
    "$ENGINE_PAR_RV2_PREDELAY",
    "$ENGINE_PAR_RV2_TIME",
    "$ENGINE_PAR_RV2_TYPE",
    "$ENGINE_PAR_RV2_SIZE",
    "$ENGINE_PAR_RV2_DAMPING",
    "$ENGINE_PAR_RV2_DIFF",
    "$ENGINE_PAR_RV2_MOD",
    "$ENGINE_PAR_RV2_STEREO",
    "$ENGINE_PAR_RV2_FREEZE",
    "$ENGINE_PAR_RV2_EQ_LOW_FREQ",
    "$ENGINE_PAR_RV2_EQ_LOW_GAIN",
    "$ENGINE_PAR_RV2_EQ_HIGH_FREQ",
    "$ENGINE_PAR_RV2_EQ_HIGH_GAIN",
    "$ENGINE_PAR_SCOMP_ATTACK",
    "$ENGINE_PAR_SCOMP_MAKEUP",
    "$ENGINE_PAR_SCOMP_MIX",
    "$ENGINE_PAR_SCOMP_RATIO",
    "$ENGINE_PAR_SCOMP_RELEASE",
    "$ENGINE_PAR_SCOMP_THRESHOLD",
    "$ENGINE_PAR_SEQ_HF_BELL",
    "$ENGINE_PAR_SEQ_HF_FREQ",
    "$ENGINE_PAR_SEQ_HF_GAIN",
    "$ENGINE_PAR_SEQ_HMF_FREQ",
    "$ENGINE_PAR_SEQ_HMF_GAIN",
    "$ENGINE_PAR_SEQ_HMF_Q",
    "$ENGINE_PAR_SEQ_LF_BELL",
    "$ENGINE_PAR_SEQ_LF_FREQ",
    "$ENGINE_PAR_SEQ_LF_GAIN",
    "$ENGINE_PAR_SEQ_LMF_FREQ",
    "$ENGINE_PAR_SEQ_LMF_GAIN",
    "$ENGINE_PAR_SEQ_LMF_Q",
    "$ENGINE_PAR_SHAPE",
    "$ENGINE_PAR_SK_BASS",
    "$ENGINE_PAR_SK_BRIGHT",
    "$ENGINE_PAR_SK_DRIVE",
    "$ENGINE_PAR_SK_MIX",
    "$ENGINE_PAR_SK_TONE",
    "$ENGINE_PAR_STEREO",
    "$ENGINE_PAR_STEREO_PAN",
    "$ENGINE_PAR_STEREO_RL",
    "$ENGINE_PAR_THRESHOLD",
    "$ENGINE_PAR_TP_GAIN",
    "$ENGINE_PAR_TP_HF_ROLLOFF",
    "$ENGINE_PAR_TP_WARMTH",
    "$ENGINE_PAR_TR_ATTACK",
    "$ENGINE_PAR_TR_SUSTAIN",
    "$ENGINE_PAR_TR_INPUT",
    "$ENGINE_PAR_TR_SMOOTH",
    "$ENGINE_PAR_SMOOTH",
    "$ENGINE_PAR_SPEED",
    "$ENGINE_PAR_SPEED_UNIT",
    "$ENGINE_PAR_START_CRITERIA_MODE",
    "$ENGINE_PAR_RELEASE_TRIGGER",
    "$ENGINE_PAR_PITCH_TRACKING",
    "$ENGINE_PAR_GROUP_SOLO",
    "$ENGINE_PAR_GROUP_MUTE",
    // EQ bands; appended so earlier ids keep their positions.
    "$ENGINE_PAR_FREQ1",
    "$ENGINE_PAR_FREQ2",
    "$ENGINE_PAR_FREQ3",
    "$ENGINE_PAR_BW1",
    "$ENGINE_PAR_BW2",
    "$ENGINE_PAR_BW3",
    "$ENGINE_PAR_GAIN1",
    "$ENGINE_PAR_GAIN2",
    "$ENGINE_PAR_GAIN3",
    // Parameters of effects and modulators KONTRA does not model yet.
    "$ENGINE_PAR_FORMANT_SIZE",
    "$ENGINE_PAR_FORMANT_TALK",
    "$ENGINE_PAR_INTMOD_FREQUENCY",
    "$ENGINE_PAR_INTMOD_PULSEWIDTH",
    "$ENGINE_PAR_JMP_BASS",
    "$ENGINE_PAR_JMP_MID",
    "$ENGINE_PAR_JMP_PREAMP",
    "$ENGINE_PAR_JMP_TREBLE",
    "$ENGINE_PAR_LFO_RAND",
    "$ENGINE_PAR_LFO_RECT",
    "$ENGINE_PAR_LFO_SAW",
    "$ENGINE_PAR_LFO_SINE",
    "$ENGINE_PAR_LFO_TRI",
    "$ENGINE_PAR_LIM_IN_GAIN",
    "$ENGINE_PAR_LIM_RELEASE",
    "$ENGINE_PAR_RV_PREDELAY",
    "$ENGINE_PAR_RV_SIZE",
];

pub fn engine_par_id(name: &str) -> Option<i32> {
    ENGINE_PARS
        .iter()
        .position(|n| *n == name)
        .map(|i| ENGINE_PAR_BASE + i as i32)
}

pub fn engine_par_name(id: i32) -> Option<&'static str> {
    ENGINE_PARS
        .get(usize::try_from(id.wrapping_sub(ENGINE_PAR_BASE)).ok()?)
        .copied()
}

/// Name for any symbolic value (control parameters, engine parameters). Script-local
/// automatic symbols are resolved by the program instead.
pub fn symbol_name(value: i32) -> Option<&'static str> {
    SYMBOLS
        .get(usize::try_from(value.wrapping_sub(SYMBOL_BASE)).ok()?)
        .copied()
        .or_else(|| engine_par_name(value))
}

pub fn symbol(name: &str) -> Option<i32> {
    SYMBOLS
        .iter()
        .position(|n| *n == name)
        .map(|i| SYMBOL_BASE + i as i32)
        .or_else(|| engine_par_id(name))
}
