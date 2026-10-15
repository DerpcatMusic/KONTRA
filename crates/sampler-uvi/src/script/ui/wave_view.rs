//! Preserve documented requests without claiming an unavailable display service.
use super::*;

fn unsupported(out: &mut ui::Interface, index: usize, key: &str, reason: String) {
    out.unsupported.push(ui::Unsupported {
        widget: Some(ui::WidgetRef(index)),
        feature: format!("WaveView.{key}"),
        value: reason,
    });
}

pub(super) fn request(
    host: &ScriptHost,
    w: &impl UiRead,
    data: &Table,
    index: usize,
    out: &mut ui::Interface,
) -> ui::UviWaveViewRequest {
    let sample = match w.read("sample") {
        None | Some(Value::Nil) => {
            if matches!(
                data.raw_get::<Value>("sample_set"),
                Ok(Value::Boolean(true))
            ) {
                ui::UviWaveViewSample::Cleared
            } else {
                ui::UviWaveViewSample::Unspecified
            }
        }
        Some(Value::String(value)) => match value.to_str() {
            Ok(value) => ui::UviWaveViewSample::Manual(value.to_string()),
            Err(_) => {
                unsupported(out, index, "sample", "sample path is not UTF-8".into());
                ui::UviWaveViewSample::Unsupported {
                    type_name: "non-UTF-8 string".into(),
                }
            }
        },
        Some(value) => {
            let type_name = value.type_name().to_owned();
            unsupported(
                out,
                index,
                "sample",
                format!("sample path must be a string; got {type_name}"),
            );
            ui::UviWaveViewSample::Unsupported { type_name }
        }
    };
    // Keep the assignment context with this sample read before later color
    // getters may execute Lua and assign a different display sample.
    let script_directory = data.raw_get::<String>("sample_directory").ok();
    let mut requested_color = |key: &str| -> Option<ui::Rgba> {
        match w.read(key) {
            None | Some(Value::Nil) => None,
            Some(Value::String(value)) => {
                let parsed = value.to_str().ok().and_then(|value| color(&value));
                if parsed.is_none() {
                    unsupported(out, index, key, "unrepresented color string".into());
                }
                parsed
            }
            Some(value) => {
                unsupported(
                    out,
                    index,
                    key,
                    format!("color must be a string; got {}", value.type_name()),
                );
                None
            }
        }
    };
    let colors = ui::UviWaveViewColors {
        playback_cursor: requested_color("playbackCursorColour"),
        high_wave: requested_color("hiWaveColour"),
        low_wave: requested_color("lowWaveColour"),
        axis: requested_color("axisColour"),
        channel_separator: requested_color("channelSeparatorColour"),
        sample_name: requested_color("sampleNameColour"),
        sample_name_background: requested_color("sampleNameBackgroundColour"),
    };
    let binding = if matches!(
        data.raw_get::<Value>("waveview_binding_set"),
        Ok(Value::Boolean(true))
    ) {
        let target = data
            .raw_get::<Value>("waveview_binding")
            .unwrap_or(Value::Nil);
        let exact = if let Value::Table(target) = &target {
            let class = host.lua.globals().raw_get::<Table>("__element_mt").ok();
            let promoted = class.as_ref().is_some_and(|class| {
                target
                    .metatable()
                    .is_some_and(|mt| mt.to_pointer() == class.to_pointer())
            });
            unsigned(target, "__id")
                .filter(|_| promoted)
                .and_then(|element_id| {
                    let scope = host
                        .shared
                        .scopes
                        .borrow()
                        .get(element_id as usize)
                        .copied()
                        .flatten();
                    if let Some(super::super::Scope::Oscillator(node)) = scope {
                        Some(ui::UviWaveViewBinding::Oscillator {
                            element_id,
                            element_node: u32::try_from(node).ok()?,
                        })
                    } else {
                        None
                    }
                })
        } else {
            None
        };
        unsupported(out, index, "binding", "oscillator sample/playback-position display service unavailable; manual sample does not edit the oscillator".into());
        exact.unwrap_or_else(|| ui::UviWaveViewBinding::Unsupported {
            type_name: target.type_name().into(),
        })
    } else {
        ui::UviWaveViewBinding::Unspecified
    };
    // Native paint and bound playback remain diagnosed by the caller. The
    // manual provider uses only this separate source request, never a zone ID.
    ui::UviWaveViewRequest {
        sample,
        script_directory,
        binding,
        colors,
    }
}
