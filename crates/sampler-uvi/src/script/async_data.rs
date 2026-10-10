//! Explicit JSON file reads. Only bytes cross the worker boundary; Lua stays on its owner.

use super::{Shared, lua_error};
use mlua::{Function, Lua, MultiValue, Thread, UserData, UserDataFields, UserDataMethods, Value};
use std::{
    fs::File,
    io::Read,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
};

const MAX_PENDING: usize = 64;
const MAX_BYTES: usize = 8 << 20;
const PENDING: u8 = 0;
const RUNNING: u8 = 1;
const FINISHED: u8 = 2;
const CANCELLED: u8 = 3;

#[derive(Default)]
struct Status {
    state: AtomicU8,
    cancel: AtomicBool,
    success: AtomicBool,
}

struct Task {
    id: u64,
    status: Arc<Status>,
}

impl UserData for Task {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, task| Ok(task.id));
        fields.add_field_method_get("finished", |_, task| {
            Ok(task.status.state.load(Ordering::Acquire) >= FINISHED)
        });
        fields.add_field_method_get("success", |_, task| {
            Ok(task.status.state.load(Ordering::Acquire) == FINISHED
                && task.status.success.load(Ordering::Acquire))
        });
        fields.add_field_method_get("progress", |_, task| {
            Ok(if task.status.state.load(Ordering::Acquire) >= FINISHED {
                1.0
            } else {
                0.0
            })
        });
        fields.add_field_method_get("state", |_, task| {
            Ok(match task.status.state.load(Ordering::Acquire) {
                PENDING => "pending",
                RUNNING => "running",
                FINISHED => "finished",
                _ => "cancelled",
            })
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("cancel", |_, task, ()| {
            if task.status.state.load(Ordering::Acquire) < FINISHED {
                task.status.cancel.store(true, Ordering::Release);
            }
            Ok(())
        });
    }
}

struct Request {
    id: u64,
    path: String,
    status: Arc<Status>,
}

struct Pending {
    id: u64,
    status: Arc<Status>,
    thread: Thread,
}

struct Worker {
    requests: SyncSender<Request>,
    replies: Receiver<(u64, Result<Option<Vec<u8>>, &'static str>)>,
    acknowledge: SyncSender<()>,
}

#[derive(Default)]
pub(super) struct DataLoads {
    worker: Option<Worker>,
    pending: Vec<Pending>,
}

impl DataLoads {
    fn worker(&mut self) -> mlua::Result<&Worker> {
        if self.worker.is_none() {
            let (requests, incoming) = mpsc::sync_channel::<Request>(MAX_PENDING);
            // Publish before waking the parked owner; wait for receipt before the next read.
            let (outgoing, replies) = mpsc::sync_channel(1);
            let (acknowledge, received) = mpsc::sync_channel(1);
            let owner = std::thread::current();
            std::thread::Builder::new()
                .name("uvi-data-io".into())
                .spawn(move || {
                    while let Ok(request) = incoming.recv() {
                        let data = if request.status.cancel.load(Ordering::Acquire) {
                            Ok(None)
                        } else {
                            request.status.state.store(RUNNING, Ordering::Release);
                            read_data(&request.path)
                        };
                        if outgoing.send((request.id, data)).is_err() {
                            break;
                        }
                        owner.unpark();
                        if received.recv().is_err() {
                            break;
                        }
                    }
                })
                .map_err(mlua::Error::external)?;
            self.worker = Some(Worker {
                requests,
                replies,
                acknowledge,
            });
        }
        self.worker
            .as_ref()
            .ok_or_else(|| mlua::Error::runtime("loadData worker unavailable"))
    }

    pub(super) fn stop(&mut self) {
        for pending in self.pending.drain(..) {
            pending.status.cancel.store(true, Ordering::Release);
            pending.status.state.store(CANCELLED, Ordering::Release);
        }
        // No join on file I/O. Closing replies/acknowledgements releases a publishing worker.
        self.worker = None;
    }

    pub(super) fn poll(&mut self, lua: &Lua, shared: &Shared) -> mlua::Result<()> {
        let Some(worker) = &self.worker else {
            return Ok(());
        };
        // One payload per owner turn bounds transient Lua/JSON memory and preserves event service.
        if let Ok((id, data)) = worker.replies.try_recv() {
            let _ = worker.acknowledge.try_send(());
            let Some(index) = self.pending.iter().position(|pending| pending.id == id) else {
                return Ok(());
            };
            let pending = self.pending.swap_remove(index);
            let cancelled = pending.status.cancel.load(Ordering::Acquire);
            pending
                .status
                .success
                .store(!cancelled && data.is_ok(), Ordering::Release);
            pending.status.state.store(
                if cancelled { CANCELLED } else { FINISHED },
                Ordering::Release,
            );
            if cancelled {
                return Ok(());
            }
            let args = match data {
                Ok(Some(bytes)) => {
                    MultiValue::from_vec(vec![Value::String(lua.create_string(bytes)?)])
                }
                Ok(None) => return Ok(()), // Vendor: unreadable is successful, but has no callback.
                Err(error) => {
                    MultiValue::from_vec(vec![Value::Nil, Value::String(lua.create_string(error)?)])
                }
            };
            shared
                .deferred
                .borrow_mut()
                .push((pending.thread, args, None));
        }
        Ok(())
    }
}

fn read_data(path: &str) -> Result<Option<Vec<u8>>, &'static str> {
    // Reject devices/directories before opening; never read an unbounded special file.
    let Ok(metadata) = std::fs::metadata(path) else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    if metadata.len() > MAX_BYTES as u64 {
        return Err("loadData file exceeds 8 MiB limit");
    }
    let Ok(file) = File::open(path) else {
        return Ok(None);
    };
    let Ok(metadata) = file.metadata() else {
        return Ok(None);
    };
    if !metadata.is_file() {
        return Ok(None);
    }
    let mut bytes = Vec::new();
    if file
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return Ok(None);
    }
    if bytes.len() > MAX_BYTES {
        return Err("loadData file exceeds 8 MiB limit");
    }
    Ok(Some(bytes))
}

fn decoded(lua: &Lua, shared: &Shared, value: serde_json::Value) -> mlua::Result<Value> {
    shared.consume_work()?;
    Ok(match value {
        serde_json::Value::Null => Value::Nil,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => Value::Number(
            value
                .as_f64()
                .ok_or_else(|| mlua::Error::runtime("loadData JSON number out of range"))?,
        ),
        serde_json::Value::String(value) => Value::String(lua.create_string(value)?),
        serde_json::Value::Array(values) => {
            let table = lua.create_table_with_capacity(values.len(), 0)?;
            for (index, value) in values.into_iter().enumerate() {
                table.raw_set(index + 1, decoded(lua, shared, value)?)?;
            }
            Value::Table(table)
        }
        serde_json::Value::Object(values) => {
            let table = lua.create_table_with_capacity(0, values.len())?;
            for (key, value) in values {
                table.raw_set(key, decoded(lua, shared, value)?)?;
            }
            Value::Table(table)
        }
    })
}

pub(super) fn install(lua: &Lua, shared: &std::rc::Rc<Shared>) -> mlua::Result<()> {
    let s = shared.clone();
    let decode = lua.create_function(move |lua, bytes: mlua::String| {
        let value = serde_json::from_slice(&bytes.as_bytes())
            .map_err(|error| mlua::Error::runtime(format!("loadData invalid JSON: {error}")))?;
        decoded(lua, &s, value)
    })?;
    // Decode in a resumable completion coroutine, not inside the caller's protected call.
    let completion: Function = lua.load(
        "return function(callback, decode) return function(bytes, failure) if failure then error(failure) end; local data = decode(bytes); if callback then callback(data) end end end",
    ).set_name("loadData-completion").eval()?;
    let s = shared.clone();
    lua.globals().raw_set(
        "loadData",
        lua.create_function(move |lua, (path, callback): (String, Option<Function>)| {
            if path.len() > 4096 || path.contains('\0') || !Path::new(&path).is_absolute() {
                return Err(mlua::Error::runtime(
                    "loadData requires an explicit absolute path of at most 4096 bytes",
                ));
            }
            let mut loads = s.data_loads.borrow_mut();
            if loads.pending.len() >= MAX_PENDING {
                return Err(mlua::Error::runtime("loadData pending queue full"));
            }
            let id = s.next_id();
            let status = Arc::new(Status::default());
            let task = lua.create_userdata(Task {
                id,
                status: status.clone(),
            })?;
            let handler: Function = completion.call((callback, decode.clone()))?;
            let thread = lua.create_thread(handler)?;
            loads
                .worker()?
                .requests
                .try_send(Request {
                    id,
                    path,
                    status: status.clone(),
                })
                .map_err(|_| mlua::Error::runtime("loadData worker queue unavailable"))?;
            loads.pending.push(Pending { id, status, thread });
            Ok(task)
        })?,
    )
}

pub(super) fn poll(host: &super::ScriptHost) {
    if let Err(error) = host
        .shared
        .data_loads
        .borrow_mut()
        .poll(&host.lua, &host.shared)
    {
        host.shared.find("lua error", &lua_error(error));
    }
}
