//! KSP's builtins: name, signature, and the [`Action`] each one asks of the host.

use kontra_vm::Action;

/// How an argument is passed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg {
    /// Integer.
    I,
    /// Real.
    R,
    /// Anything, converted to text.
    S,
    /// Integer or real; every `N` of one call has the same type.
    N,
    /// A UI control, passed as its id.
    V,
    /// An integer or real array, passed as `base, len` of its cells.
    A,
    /// A bare word or string naming a key, passed as text.
    K,
}

/// What a builtin returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ret {
    Void,
    Int,
    Real,
    Text,
    /// A condition (0 or 1).
    Bool,
    /// The type of the `N` arguments.
    Num,
}

#[derive(Clone, Copy, Debug)]
pub struct Sig {
    pub args: &'static [Arg],
    /// How many trailing arguments may be left out (they pass 0).
    pub optional: u8,
    pub ret: Ret,
}

macro_rules! builtins {
    (
        specials { $($sid:ident $sname:literal [$($sarg:ident)*] $sopt:literal $sret:ident;)* }
        actions { $($id:ident $name:literal [$($arg:ident)*] $optional:literal $ret:ident;)* }
    ) => {
        /// A KSP builtin function, system variable (`$EVENT_ID`) or system array
        /// (`%KEY_DOWN`): either lowered by the compiler or an [`Action`].
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Builtin { $($sid,)* $($id,)* }

        impl Builtin {
            #[must_use]
            pub fn name(self) -> &'static str {
                match self { $(Self::$sid => $sname,)* $(Self::$id => $name,)* }
            }

            #[must_use]
            pub fn sig(self) -> Sig {
                match self {
                    $(Self::$sid => Sig { args: &[$(Arg::$sarg),*], optional: $sopt, ret: Ret::$sret },)*
                    $(Self::$id => Sig { args: &[$(Arg::$arg),*], optional: $optional, ret: Ret::$ret },)*
                }
            }

            /// The host action, for builtins that are not lowered to bytecode.
            #[must_use]
            pub fn action(self) -> Option<Action> {
                match self {
                    $(Self::$id => Some(Action::$id),)*
                    _ => None,
                }
            }

            fn exact(name: &str) -> Option<Self> {
                match name { $($sname => Some(Self::$sid),)* $($name => Some(Self::$id),)* _ => None }
            }

            /// Every builtin.
            pub const ALL: &[Self] = &[$(Self::$sid,)* $(Self::$id,)*];
        }
    };
}

impl Builtin {
    /// The builtin called `name`. Kontakt 2's underscore spellings
    /// (`_read_persistent_var`) name the same builtins.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::exact(name).or_else(|| Self::exact(name.strip_prefix('_')?))
    }
}

builtins! {
    specials {
        Exit "exit" [] 0 Void;
        Continue "continue" [] 0 Void;
        Inc "inc" [I] 0 Void;
        Dec "dec" [I] 0 Void;
        Wait "wait" [I] 0 Void;
        IntToReal "int_to_real" [I] 0 Real;
        Real "real" [I] 0 Real;
        RealToInt "real_to_int" [R] 0 Int;
        Int "int" [R] 0 Int;
        ShLeft "sh_left" [I I] 0 Int;
        ShRight "sh_right" [I I] 0 Int;
        GetUiId "get_ui_id" [V] 0 Int;
        MakePersistent "make_persistent" [V] 0 Void;
        MakeInstrPersistent "make_instr_persistent" [V] 0 Void;
        WatchVar "watch_var" [V] 0 Void;
        WatchArrayIdx "watch_array_idx" [A I] 0 Void;
        NumElements "num_elements" [A] 0 Int;
    }
    actions {
        // Arithmetic ([`crate::pure`]).
        Abs "abs" [N] 0 Num;
        Min "min" [N N] 0 Num;
        Max "max" [N N] 0 Num;
        InRange "in_range" [N N N] 0 Bool;
        Sgn "sgn" [N] 0 Int;
        Signbit "signbit" [N] 0 Int;
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
        // Arrays ([`crate::pure`]).
        Search "search" [A N I I] 2 Int;
        Sort "sort" [A I I I] 2 Void;
        ArrayEqual "array_equal" [A A] 0 Bool;
        // Host state.
        Random "random" [I I] 0 Int;
        MsToTicks "ms_to_ticks" [I] 0 Int;
        TicksToMs "ticks_to_ms" [I] 0 Int;
        LoadArray "load_array" [A I] 0 Int;
        SaveArray "save_array" [A I] 0 Int;
        LoadArrayStr "load_array_str" [A S] 0 Int;
        SaveArrayStr "save_array_str" [A S] 0 Int;
        ReadPersistentVar "read_persistent_var" [V] 0 Void;
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
        FadeOut "fade_out" [I I I I] 1 Void;
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
        WaitTicks "wait_ticks" [I] 0 Void;
        WaitAsync "wait_async" [I] 0 Void;
        StopWait "stop_wait" [I I] 0 Void;
        ResetKspTimer "reset_ksp_timer" [] 0 Void;
        SetListener "set_listener" [I I] 0 Void;
        ChangeListenerPar "change_listener_par" [I I] 0 Void;
        // MIDI file object.
        MfGetFirst "mf_get_first" [I] 0 Void;
        MfGetLast "mf_get_last" [I] 0 Void;
        MfGetNext "mf_get_next" [I] 0 Void;
        MfGetPrev "mf_get_prev" [I] 0 Void;
        MfGetNextAt "mf_get_next_at" [I I] 0 Void;
        MfGetPrevAt "mf_get_prev_at" [I I] 0 Void;
        MfGetId "mf_get_id" [] 0 Int;
        MfGetCommand "mf_get_command" [] 0 Int;
        MfSetCommand "mf_set_command" [I] 0 Void;
        MfGetByteOne "mf_get_byte_one" [] 0 Int;
        MfSetByteOne "mf_set_byte_one" [I] 0 Void;
        MfGetByteTwo "mf_get_byte_two" [] 0 Int;
        MfSetByteTwo "mf_set_byte_two" [I] 0 Void;
        MfGetChannel "mf_get_channel" [] 0 Int;
        MfSetChannel "mf_set_channel" [I] 0 Void;
        MfGetPos "mf_get_pos" [] 0 Int;
        MfSetPos "mf_set_pos" [I] 0 Void;
        MfGetLength "mf_get_length" [] 0 Int;
        MfSetLength "mf_set_length" [I] 0 Void;
        MfGetTrackIdx "mf_get_track_idx" [] 0 Int;
        MfSetTrackIdx "mf_set_track_idx" [I] 0 Void;
        MfGetEventPar "mf_get_event_par" [I I] 0 Int;
        MfSetEventPar "mf_set_event_par" [I I I] 0 Void;
        MfGetNumTracks "mf_get_num_tracks" [] 0 Int;
        MfGetBufferSize "mf_get_buffer_size" [] 0 Int;
        MfSetBufferSize "mf_set_buffer_size" [I] 0 Int;
        MfInsertEvent "mf_insert_event" [I I I I I] 0 Int;
        MfRemoveEvent "mf_remove_event" [I] 0 Void;
        MfGetMark "mf_get_mark" [I I] 0 Int;
        MfSetMark "mf_set_mark" [I I I] 0 Void;
        MfSetExportArea "mf_set_export_area" [S I I I I] 0 Int;
        MfSetNumExportAreas "mf_set_num_export_areas" [I] 0 Void;
        MfCopyExportArea "mf_copy_export_area" [I] 0 Void;
        MfReset "mf_reset" [] 0 Int;
        MfInsertFile "mf_insert_file" [S I I I] 0 Int;
        SaveMidiFile "save_midi_file" [S] 0 Int;
        LoadMidiFile "load_midi_file" [S] 0 Int;
        MfGetLastFilename "mf_get_last_filename" [] 0 Text;
        ByTrack "by_track" [I] 0 Int;
        // Groups, zones, modules and engine parameters.
        FindGroup "find_group" [S] 0 Int;
        GetGroupIdx "get_group_idx" [S] 0 Int;
        GroupName "group_name" [I] 0 Text;
        GetNumZones "get_num_zones" [] 0 Int;
        GetZoneId "get_zone_id" [I] 0 Int;
        GetSampleLength "get_sample_length" [I] 0 Int;
        GetZonePar "get_zone_par" [I I] 0 Int;
        SetZonePar "set_zone_par" [I I I] 0 Int;
        PurgeGroup "purge_group" [I I] 0 Int;
        GetPurgeState "get_purge_state" [I] 0 Int;
        FindMod "find_mod" [I S] 0 Int;
        FindTarget "find_target" [I I S] 0 Int;
        GetModIdx "get_mod_idx" [I S] 0 Int;
        GetTargetIdx "get_target_idx" [I I S] 0 Int;
        GetEnginePar "get_engine_par" [I I I I] 0 Int;
        GetEngineParDisp "get_engine_par_disp" [I I I I] 0 Text;
        GetEngineParDispExt "get_engine_par_disp_ext" [I I I I I] 0 Text;
        SetEnginePar "set_engine_par" [I I I I I] 0 Int;
        GetEngineParM "get_engine_par_m" [I I I I] 0 Int;
        GetEngineParDispM "get_engine_par_disp_m" [I I I I] 0 Text;
        SetEngineParM "set_engine_par_m" [I I I I I] 0 Int;
        GetVoiceLimit "get_voice_limit" [I] 0 Int;
        SetVoiceLimit "set_voice_limit" [I I] 0 Int;
        OutputChannelName "output_channel_name" [I] 0 Text;
        GetScale "get_scale" [I A] 0 Void;
        GetScaleName "get_scale_name" [I I] 0 Text;
        SubscribeAsync "subscribe_async" [I I I] 0 Void;
        SetGlobalKey "set_global_key" [I] 0 Void;
        SetGlobalScale "set_global_scale" [I] 0 Void;
        LoadIrSample "load_ir_sample" [S I I] 0 Int;
        FindZone "find_zone" [S] 0 Int;
        LoadNativeUi "load_native_ui" [S] 0 Void;
        LoadPerformanceView "load_performance_view" [S] 0 Void;
        // User interface.
        AttachLevelMeter "attach_level_meter" [I I I I I] 0 Void;
        SetControlPar "set_control_par" [I I I] 0 Void;
        SetControlParStr "set_control_par_str" [I I S] 0 Void;
        SetControlParReal "set_control_par_real" [I I R] 0 Void;
        SetControlParArr "set_control_par_arr" [I I I I] 0 Void;
        SetControlParStrArr "set_control_par_str_arr" [I I S I] 0 Void;
        SetControlParRealArr "set_control_par_real_arr" [I I R I] 0 Void;
        GetControlPar "get_control_par" [I I] 0 Int;
        GetControlParStr "get_control_par_str" [I I] 0 Text;
        GetControlParReal "get_control_par_real" [I I] 0 Real;
        GetControlParArr "get_control_par_arr" [I I I] 0 Int;
        GetControlParStrArr "get_control_par_str_arr" [I I I] 0 Text;
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
        GetMenuItemStr "get_menu_item_str" [I I] 0 Text;
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
        GetFolder "get_folder" [I] 0 Text;
        FsGetFilename "fs_get_filename" [I I] 0 Text;
        FsNavigate "fs_navigate" [I I] 0 Void;
        // NKS2 navigation. The KSP manual has no entry for these three: its
        // Kontakt 7.6 changelog only says "Support for NKS2 parameters and
        // navigation definition through new commands and control parameters:
        // see NKS2 SDK documentation", which this repo does not hold. The
        // engine has no NKS2 navigation model either, so it leaves them
        // unanswered rather than guessing at a layout nothing could read.
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
        GetKeyName "get_key_name" [I] 0 Text;
        GetKeyType "get_key_type" [I] 0 Int;
        GetKeyTriggerstate "get_key_triggerstate" [I] 0 Int;
        SetKeyrange "set_keyrange" [I I S] 0 Void;
        RemoveKeyrange "remove_keyrange" [I] 0 Void;
        // Diagnostics.
        Message "message" [S] 0 Void;
        DisableLogging "disable_logging" [I] 0 Void;
        // Program global storage, shared by every script slot.
        PgsCreateKey "pgs_create_key" [K I] 0 Void;
        PgsKeyExists "pgs_key_exists" [K] 0 Bool;
        PgsSetKeyVal "pgs_set_key_val" [K I I] 0 Void;
        PgsGetKeyVal "pgs_get_key_val" [K I] 0 Int;
        PgsCreateStrKey "pgs_create_str_key" [K] 0 Void;
        PgsStrKeyExists "pgs_str_key_exists" [K] 0 Bool;
        PgsSetStrKeyVal "pgs_set_str_key_val" [K S] 0 Void;
        PgsGetStrKeyVal "pgs_get_str_key_val" [K] 0 Text;
        // System variables, read in the running callback.
        EventId "$EVENT_ID" [] 0 Int;
        EventNote "$EVENT_NOTE" [] 0 Int;
        EventVelocity "$EVENT_VELOCITY" [] 0 Int;
        NoteHeld "$NOTE_HELD" [] 0 Int;
        CcNum "$CC_NUM" [] 0 Int;
        PitchBend "$PITCH_BEND" [] 0 Int;
        PolyAtNum "$POLY_AT_NUM" [] 0 Int;
        RpnAddress "$RPN_ADDRESS" [] 0 Int;
        RpnValue "$RPN_VALUE" [] 0 Int;
        MidiChannel "$MIDI_CHANNEL" [] 0 Int;
        EngineUptime "$ENGINE_UPTIME" [] 0 Int;
        KspTimer "$KSP_TIMER" [] 0 Int;
        CallbackType "$NI_CALLBACK_TYPE" [] 0 Int;
        CallbackId "$NI_CALLBACK_ID" [] 0 Int;
        SignalType "$NI_SIGNAL_TYPE" [] 0 Int;
        AsyncId "$NI_ASYNC_ID" [] 0 Int;
        AsyncExitStatus "$NI_ASYNC_EXIT_STATUS" [] 0 Int;
        DurationQuarter "$DURATION_QUARTER" [] 0 Int;
        DurationEighth "$DURATION_EIGHTH" [] 0 Int;
        DurationSixteenth "$DURATION_SIXTEENTH" [] 0 Int;
        DurationQuarterTriplet "$DURATION_QUARTER_TRIPLET" [] 0 Int;
        DurationEighthTriplet "$DURATION_EIGHTH_TRIPLET" [] 0 Int;
        DurationSixteenthTriplet "$DURATION_SIXTEENTH_TRIPLET" [] 0 Int;
        DurationBar "$DURATION_BAR" [] 0 Int;
        SongPosition "$NI_SONG_POSITION" [] 0 Int;
        SignatureNum "$SIGNATURE_NUM" [] 0 Int;
        SignatureDenom "$SIGNATURE_DENOM" [] 0 Int;
        TransportRunning "$NI_TRANSPORT_RUNNING" [] 0 Int;
        Bpm "$NI_BPM" [] 0 Int;
        Tempo "$NI_TEMPO" [] 0 Int;
        CurrentScriptSlot "$CURRENT_SCRIPT_SLOT" [] 0 Int;
        UiIdVar "$NI_UI_ID" [] 0 Int;
        PlayedVoicesTotal "$PLAYED_VOICES_TOTAL" [] 0 Int;
        PlayedVoicesInst "$PLAYED_VOICES_INST" [] 0 Int;
        DistanceBarStart "$DISTANCE_BAR_START" [] 0 Int;
        NumGroups "$NUM_GROUPS" [] 0 Int;
        NumZones "$NUM_ZONES" [] 0 Int;
        NumOutputChannels "$NUM_OUTPUT_CHANNELS" [] 0 Int;
        MouseOverControl "$NI_MOUSE_OVER_CONTROL" [] 0 Int;
        ControlParIdx "$NI_CONTROL_PAR_IDX" [] 0 Int;
        MouseEventType "$NI_MOUSE_EVENT_TYPE" [] 0 Int;
        DateYear "$NI_DATE_YEAR" [] 0 Int;
        DateMonth "$NI_DATE_MONTH" [] 0 Int;
        DateDay "$NI_DATE_DAY" [] 0 Int;
        TimeHour "$NI_TIME_HOUR" [] 0 Int;
        TimeMinute "$NI_TIME_MINUTE" [] 0 Int;
        TimeSecond "$NI_TIME_SECOND" [] 0 Int;
        // System arrays: the index is the argument.
        KeyDown "%KEY_DOWN" [I] 0 Int;
        Cc "%CC" [I] 0 Int;
        CcTouched "%CC_TOUCHED" [I] 0 Int;
        PolyAt "%POLY_AT" [I] 0 Int;
        GroupsSelected "%GROUPS_SELECTED" [I] 0 Int;
        GroupsAffected "%GROUPS_AFFECTED" [I] 0 Int;
        KeyDownOct "%KEY_DOWN_OCT" [I] 0 Int;
        EventParAt "%EVENT_PAR" [I] 0 Int;
    }
}

impl Builtin {
    /// Length of a system array, for `num_elements`.
    #[must_use]
    pub fn array_len(self) -> Option<u32> {
        Some(match self {
            Self::KeyDown | Self::PolyAt => 128,
            Self::Cc | Self::CcTouched => 130,
            Self::KeyDownOct => 12,
            Self::GroupsSelected | Self::GroupsAffected => 4096,
            Self::EventParAt => 4,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_resolve_and_actions_match() {
        for &b in Builtin::ALL {
            assert_eq!(Builtin::from_name(b.name()), Some(b));
            if let Some(action) = b.action() {
                assert_eq!(format!("{action:?}"), format!("{b:?}"));
            }
        }
        assert_eq!(Builtin::Exit.action(), None);
        assert_eq!(
            Builtin::from_name("_read_persistent_var"),
            Some(Builtin::ReadPersistentVar)
        );
        assert_eq!(Builtin::from_name("no_such_thing"), None);
    }
}
