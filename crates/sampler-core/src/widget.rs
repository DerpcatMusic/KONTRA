//! Atomic typed UI admission; scalar and array/text widgets share this path.
use crate::{
    BehaviorId, ControlContext, ControlId, ControlValue, ControlWrite, Error, PlanId, Prepared,
    Runtime, ScriptInstanceId, Text,
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidgetStorage {
    Control(ControlId),
    Cells {
        offset: u32,
        len: u32,
        real: bool,
        min: f64,
        max: f64,
    },
    FileSelection {
        offset: u32,
    },
    Texts {
        offset: u32,
        len: u32,
    },
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetDefinition {
    pub id: ControlId,
    pub source_slot: u8,
    pub ui_id: i32,
    pub instance: ScriptInstanceId,
    pub storage: WidgetStorage,
    pub program: Option<usize>,
    pub stage: usize,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WidgetValue {
    Integer(i64),
    Real(f64),
    Text(Text),
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetEdit {
    pub id: ControlId,
    pub index: u32,
    pub value: WidgetValue,
    pub interaction: WidgetInteraction,
}
/// Service event codes resolved by the KSP compiler and UI producer together.
/// These encode named event semantics; they are not opaque per-script symbols.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum WidgetEventType {
    LeftButtonDown = 0,
    LeftButtonUp = 1,
    Drag = 2,
    Drop = 3,
    DndDrag = 4,
    DndDrop = 5,
}
impl WidgetEventType {
    pub fn ksp_constant(name: &str) -> Option<Self> {
        Some(match name.trim_start_matches('$') {
            "NI_MOUSE_EVENT_TYPE_LEFT_BUTTON_DOWN" => Self::LeftButtonDown,
            "NI_MOUSE_EVENT_TYPE_LEFT_BUTTON_UP" => Self::LeftButtonUp,
            "NI_MOUSE_EVENT_TYPE_DRAG" => Self::Drag,
            "NI_MOUSE_EVENT_TYPE_DROP" => Self::Drop,
            "NI_MOUSE_EVENT_TYPE_DND_DRAG" => Self::DndDrag,
            "NI_MOUSE_EVENT_TYPE_DND_DROP" => Self::DndDrop,
            _ => return None,
        })
    }
}
/// Metadata retained by the callback, including across waits. `index` is the
/// edited cell; `cursor` is the producer's cursor/selection position. Modifiers
/// use bit 0 shift, bit 1 control, bit 2 alt. Event is the native mouse-event code.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WidgetInteraction {
    pub index: u32,
    pub cursor: u32,
    pub modifiers: u8,
    pub event: i32,
    pub event_par: [i32; 4],
}
/// Maximum cells in one mouse move/paste. One accepted transaction fires one
/// ui_control callback, even when it crosses many columns. An oversized batch
/// returns InvalidInput before any write or callback; never split one gesture
/// into transactions (that would change callback semantics). Producers coalesce
/// repeated cell indices and reject gestures beyond this explicit ceiling.
/// ponytail: 4096 cells per gesture; increase after measuring a larger table.
pub const WIDGET_EDIT_CAPACITY: usize = 4096;

impl Prepared {
    pub fn widget_definitions(&self) -> &[WidgetDefinition] {
        &self.widgets
    }

    pub fn with_widgets(mut self, mut widgets: Vec<WidgetDefinition>) -> Result<Self, Error> {
        widgets.sort_by_key(|w| w.id);
        if widgets.windows(2).any(|w| w[0].id == w[1].id) {
            return Err(Error::InvalidInput);
        }
        for w in &widgets {
            let bank = self
                .script_initial
                .get(usize::from(w.instance.0))
                .ok_or(Error::InvalidInput)?;
            match w.storage {
                WidgetStorage::Control(id) => {
                    self.control_index(id)?;
                }
                WidgetStorage::Cells {
                    offset,
                    len,
                    min,
                    max,
                    ..
                } => {
                    if len == 0
                        || offset
                            .checked_add(len)
                            .is_none_or(|end| end as usize > bank.cells.len())
                        || !min.is_finite()
                        || !max.is_finite()
                        || min > max
                    {
                        return Err(Error::InvalidInput);
                    }
                }
                WidgetStorage::FileSelection { offset } => {
                    if offset as usize >= bank.texts.len() {
                        return Err(Error::InvalidInput);
                    }
                }
                WidgetStorage::Texts { offset, len } => {
                    if len == 0
                        || offset
                            .checked_add(len)
                            .is_none_or(|end| end as usize > bank.texts.len())
                    {
                        return Err(Error::InvalidInput);
                    }
                }
            }
            if let Some(p) = w.program {
                let p = self.programs.get(p).ok_or(Error::InvalidInput)?;
                if p.requires_note
                    || p.requires_controller
                    || p.script_instance != Some(w.instance)
                    || w.stage >= self.stages.len()
                {
                    return Err(Error::InvalidInput);
                }
            }
        }
        self.widgets = widgets.into_boxed_slice();
        Ok(self)
    }
}
impl Runtime {
    pub fn widget_definitions(&self, plan: PlanId) -> Result<&[WidgetDefinition], Error> {
        Ok(&self
            .plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .widgets)
    }

    /// Resolve source KSP slot and native UI id without exposing runtime packing.
    pub fn widget_id(&self, plan: PlanId, slot: u8, ui_id: i32) -> Result<ControlId, Error> {
        self.plans
            .get(plan.0)
            .ok_or(Error::StaleHandle)?
            .prepared
            .widgets
            .iter()
            .find(|w| w.source_slot == slot && w.ui_id == ui_id)
            .map(|w| w.id)
            .ok_or(Error::InvalidInput)
    }
    /// Capture on the audio owner into producer-owned storage. Validate the whole
    /// request before touching the buffer; one coherent revision covers it.
    pub fn capture_widgets(
        &self,
        plan: PlanId,
        output: &mut [WidgetEdit],
    ) -> Result<(usize, u64), Error> {
        if output.len() > WIDGET_EDIT_CAPACITY {
            return Err(Error::InvalidInput);
        }
        let revision = self.control_revision(plan)?;
        for edit in output.iter() {
            self.widget_value(plan, edit.id, edit.index)?;
        }
        for edit in output.iter_mut() {
            edit.value = self.widget_value(plan, edit.id, edit.index)?;
        }
        Ok((output.len(), revision))
    }
    pub fn widget_value(
        &self,
        plan: PlanId,
        id: ControlId,
        index: u32,
    ) -> Result<WidgetValue, Error> {
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let widgets = &generation.prepared.widgets;
        let w = &widgets[widgets
            .binary_search_by_key(&id, |w| w.id)
            .map_err(|_| Error::InvalidInput)?];
        let bank = &generation.scripts[usize::from(w.instance.0)];
        Ok(match w.storage {
            WidgetStorage::Control(control) if index == 0 => {
                match self.control_value(plan, control)? {
                    ControlValue::Integer(v) => WidgetValue::Integer(v),
                    ControlValue::Real(v) => WidgetValue::Real(v),
                    ControlValue::Toggle(v) => WidgetValue::Integer(i64::from(v)),
                }
            }
            WidgetStorage::Cells {
                offset, len, real, ..
            } if index < len => {
                let v = bank.cells[(offset + index) as usize];
                if real {
                    WidgetValue::Real(f64::from_bits(v as u64))
                } else {
                    WidgetValue::Integer(v)
                }
            }
            WidgetStorage::FileSelection { offset } if index == 0 => {
                WidgetValue::Text(bank.texts[offset as usize])
            }
            WidgetStorage::Texts { offset, len } if index < len => {
                WidgetValue::Text(bank.texts[(offset + index) as usize])
            }
            _ => return Err(Error::InvalidInput),
        })
    }
    pub fn invoke_widget(
        &mut self,
        context: ControlContext,
        plan: PlanId,
        expected_revision: Option<u64>,
        edits: &[WidgetEdit],
    ) -> Result<(u64, Option<BehaviorId>), Error> {
        if edits.is_empty()
            || edits.len() > WIDGET_EDIT_CAPACITY
            || edits.iter().any(|e| e.id != edits[0].id)
            || context.origin.group >= 16
            || context.origin.channel >= 16
            || context.channels == 0
        {
            return Err(Error::InvalidInput);
        }
        let performance = self.performance_index(context.performance)?;
        self.apply_due();
        let generation = self.plans.get(plan.0).ok_or(Error::StaleHandle)?;
        let widgets = &generation.prepared.widgets;
        let w = widgets[widgets
            .binary_search_by_key(&edits[0].id, |w| w.id)
            .map_err(|_| Error::InvalidInput)?];
        let revision = self.control_revision(plan)?;
        if expected_revision.is_some_and(|r| r != revision) {
            return Err(Error::RevisionConflict);
        }
        let mut indices = [0u32; WIDGET_EDIT_CAPACITY];
        for (index, edit) in edits.iter().enumerate() {
            indices[index] = edit.index;
        }
        indices[..edits.len()].sort_unstable();
        if indices[..edits.len()].windows(2).any(|w| w[0] == w[1])
            || edits[0].interaction.modifiers > 7
        {
            return Err(Error::InvalidInput);
        }
        let mut scalar = None;
        for e in edits {
            if e.interaction != edits[0].interaction {
                return Err(Error::InvalidInput);
            }
            match (w.storage, e.value) {
                (WidgetStorage::Control(id), WidgetValue::Integer(v))
                    if e.index == 0 && edits.len() == 1 =>
                {
                    scalar = Some(ControlWrite {
                        id,
                        value: match self.control_definition(plan, id)?.domain {
                            crate::ControlDomain::Toggle if v == 0 || v == 1 => {
                                ControlValue::Toggle(v != 0)
                            }
                            crate::ControlDomain::Integer { .. } => ControlValue::Integer(v),
                            _ => return Err(Error::InvalidInput),
                        },
                    })
                }
                (WidgetStorage::Control(id), WidgetValue::Real(v))
                    if e.index == 0 && edits.len() == 1 && v.is_finite() =>
                {
                    scalar = Some(ControlWrite {
                        id,
                        value: ControlValue::Real(v),
                    })
                }
                (
                    WidgetStorage::Cells {
                        len,
                        real: false,
                        min,
                        max,
                        ..
                    },
                    WidgetValue::Integer(v),
                ) if e.index < len && (v as f64) >= min && (v as f64) <= max => {}
                (
                    WidgetStorage::Cells {
                        len,
                        real: true,
                        min,
                        max,
                        ..
                    },
                    WidgetValue::Real(v),
                ) if e.index < len && v.is_finite() && v >= min && v <= max => {}
                (WidgetStorage::FileSelection { .. }, WidgetValue::Text(_))
                    if e.index == 0 && edits.len() == 1 => {}
                (WidgetStorage::Texts { len, .. }, WidgetValue::Text(_)) if e.index < len => {}
                _ => return Err(Error::InvalidInput),
            }
        }
        let event = crate::behavior::PlanContext::Control(crate::control::ControlEvent {
            performance,
            origin: context.origin,
            channels: context.channels,
            stage: w.stage,
            interaction: WidgetInteraction {
                index: edits[0].index,
                ..edits[0].interaction
            },
        });
        if let Some(program) = w.program {
            self.validate_plan_context(plan, program, event)?;
        }
        if let Some(write) = scalar {
            self.edit_controls_now(plan, expected_revision, &[write])?;
        } else {
            let generation = self.plans.get_mut(plan.0).unwrap();
            let next = revision.checked_add(1).ok_or(Error::Capacity)?;
            let bank = &mut generation.scripts[usize::from(w.instance.0)];
            for e in edits {
                match (w.storage, e.value) {
                    (WidgetStorage::Cells { offset, .. }, WidgetValue::Integer(v)) => {
                        bank.cells[(offset + e.index) as usize] = v
                    }
                    (WidgetStorage::Cells { offset, .. }, WidgetValue::Real(v)) => {
                        bank.cells[(offset + e.index) as usize] = v.to_bits() as i64
                    }
                    (WidgetStorage::FileSelection { offset }, WidgetValue::Text(v)) => {
                        bank.texts[offset as usize] = v
                    }
                    (WidgetStorage::Texts { offset, .. }, WidgetValue::Text(v)) => {
                        bank.texts[(offset + e.index) as usize] = v
                    }
                    _ => unreachable!(),
                }
            }
            generation.controls.revision = next;
        }
        let callback = match w.program {
            Some(program) => Some(
                self.start_plan_context(plan, program, event)
                    .expect("preflighted widget callback admission"),
            ),
            None => None,
        };
        Ok((self.control_revision(plan)?, callback))
    }
}
