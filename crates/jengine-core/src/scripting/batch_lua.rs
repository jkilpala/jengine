//! Step 4 Slice 4B.1 — batch **Lua** systems via `mlua` (Luau backend).
//!
//! Mirrors [`crate::scripting::batch::BatchJsRuntime`] one-for-one in shape so
//! the bench numbers are literally apples-to-apples:
//!
//! - Same `update(ctx, q)` contract.
//! - Same `system.query.{read,write}` validation against the fixed
//!   walking-skeleton contract (`read=["Velocity2D"]`, `write=["Position2D"]`).
//! - Same per-call mirror-buffer model: fresh views are produced for every
//!   call, the script mutates the `write` views, the runtime copies them
//!   back into ECS columns.
//! - Same `FrameCounters` accounting (`batch_copy_in_*`, `batch_run_us`,
//!   `batch_copy_out_*`).
//!
//! ### Buffer modes (Slice 4B.1)
//!
//! Lua doesn't have `Float32Array`; there are several plausible ways to
//! expose a column to user code, each with very different per-element
//! cost. Slice 4B.1 ships **two** so the lessons doc can compare them
//! directly:
//!
//! | mode      | how the script reads `q.read.Velocity2D.x[i]`              |
//! |-----------|-------------------------------------------------------------|
//! | `Table`   | `q.read.Velocity2D.x[i]` — Lua table, **1-indexed**.        |
//! | `UserData`| `q.read.Velocity2D.x[i]` — `F32Buffer` userdata, **1-indexed** via `__index`/`__newindex`. |
//! | `Buffer`  | `buffer.readf32(q.read.Velocity2D.x, (i-1)*4)` — native Luau `buffer` (byte offsets). |
//!
//! Table and UserData modes 1-index from Lua because that's idiomatic;
//! the underlying Rust storage is 0-indexed and the metamethods do the
//! offset. `Buffer` mode uses raw byte offsets per the Luau `buffer`
//! library convention — scripts must compute `(i-1) * 4` themselves,
//! but get JIT-friendly direct memory access in return.
//!
//! Slice 4B.2 added the `Buffer` mode. Host-side it uses
//! [`mlua::Buffer`] populated via `lua.create_buffer(byte_slice)` and
//! read back with [`Buffer::to_vec`]; the f32 ⇄ u8 reinterpretation is
//! a sound, alignment-respecting transmute via `bytemuck`-style raw
//! slices (see `f32_slice_as_bytes` / `bytes_to_f32_slice` below).

#![cfg(all(not(target_arch = "wasm32"), feature = "native"))]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::time::Instant;

use mlua::{
    Buffer as LuaBuffer, Function as LuaFunction, Lua, MetaMethod, Result as LuaResult,
    Table as LuaTable, UserData, UserDataMethods, Value as LuaValue,
};

use crate::profiling::FrameCounters;
use crate::scripting::batch::{
    BatchError, BatchOptionalColumn, BatchReadColumn, BatchStateField, BatchSystemSpec,
    BatchWriteColumn,
};
use crate::scripting::prng::DetRng;

/// How `q.read.*` / `q.write.*` columns are exposed inside the Lua VM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchLuaBufferMode {
    /// Plain Lua tables, 1-indexed. Highest per-element cost (each
    /// access is a hashed table lookup) but zero metaprogramming setup.
    Table,
    /// `F32Buffer` userdata wrapping `Rc<RefCell<Vec<f32>>>` with
    /// `__index` / `__newindex` metamethods, also 1-indexed.
    UserData,
    /// Native Luau `buffer` populated host-side via `lua.create_buffer`.
    /// Scripts use the `buffer.readf32` / `buffer.writef32` global
    /// library with **byte offsets** (`(i-1) * 4` for element `i`).
    /// Host marshalling is a single contiguous memcpy in each
    /// direction; per-element script access is direct memory load/store
    /// (no metamethod, no hash lookup).
    Buffer,
}

/// Per-system bookkeeping after a successful load.
struct LoadedBatchScript {
    /// Per-script `update` function. In Luau the function captures its
    /// `_ENV` upvalue, so storing the function alone is enough to call
    /// it later in the same VM with the script's own globals.
    update: LuaFunction,
    #[allow(dead_code)]
    path: String,
    spec: Option<BatchSystemSpec>,
    /// Slice 5.4a: per-script reusable Lua-side handles for the
    /// generic [`BatchLuaRuntime::run_batch`] path. Lazily allocated
    /// on first call. Tables / Vecs / Buffers grow monotonically: if
    /// frame N has count=30000 and frame N+1 has count=12000, the
    /// underlying storage stays at length 30000 and the script honors
    /// `q.count = 12000`. Resizing only happens when count exceeds
    /// the current capacity.
    cache: RefCell<Option<BatchLuaCache>>,
}

/// Slice 5.4a: per-(script × buffer-mode) cached Lua handles. Built
/// once on first `run_batch` call, then reused every frame to avoid
/// `create_sequence_from` / `create_buffer` / `create_table` churn.
///
/// Layout mirrors the script's [`BatchSystemSpec`]:
/// - One [`CachedColumn`] per declared `read`, `write`, and
///   `read_optional` component, in spec order.
/// - One [`CachedStateColumn`] per declared `state` field, in
///   alphabetical schema order.
/// - The outer `q` / `read` / `write` / `read_optional` / `state` /
///   `ctx` tables are allocated once.
///
/// **Capacity**: `capacity` is the high-water-mark element count.
/// Inner storage (Lua sequence tables, `Vec<f32>`, Luau buffers) is
/// always at least this long. When a call's `count` exceeds it, the
/// cache grows to `count`. Tables never shrink.
struct BatchLuaCache {
    q: LuaTable,
    /// Outer `read` table — referenced via `q.read` from Lua so kept
    /// alive transitively. Held here only to make the layout explicit.
    #[allow(dead_code)]
    read: LuaTable,
    /// Outer `write` table — see [`Self::read`].
    #[allow(dead_code)]
    write: LuaTable,
    read_optional: LuaTable,
    state: LuaTable,
    ctx: LuaTable,
    read_cols: Vec<CachedColumn>,
    write_cols: Vec<CachedColumn>,
    optional_cols: Vec<CachedOptionalColumn>,
    state_cols: Vec<CachedStateColumn>,
    /// High-water-mark element count seen so far. Storage is sized to
    /// at least this.
    capacity: usize,
}

/// Slice 5.4a: per-mode storage for one declared read/write column.
/// `inner` is the `{x=, y=}` Lua table set on the outer `read` /
/// `write` outer table once at cache build time.
struct CachedColumn {
    name: String,
    inner: LuaTable,
    storage: ColumnStorage,
}

enum ColumnStorage {
    Table {
        x: LuaTable,
        y: LuaTable,
    },
    UserData {
        x: Rc<RefCell<Vec<f32>>>,
        y: Rc<RefCell<Vec<f32>>>,
    },
    Buffer {
        x: LuaBuffer,
        y: LuaBuffer,
    },
}

/// Slice 5.4a: per-mode storage for one declared `read_optional`
/// column. Same shape as [`CachedColumn`] but adds `present_last`
/// so the cache can elide redundant `read_optional[name] = nil` /
/// `= inner` writes when presence is unchanged across frames.
struct CachedOptionalColumn {
    column: CachedColumn,
    /// `true` if the previous call set this column on the
    /// `read_optional` outer table; `false` if it was nil. Drives the
    /// per-call presence transition.
    present_last: bool,
}

/// Slice 5.4a: per-mode storage for one declared state field.
struct CachedStateColumn {
    #[allow(dead_code)]
    name: String,
    storage: StateStorage,
}

enum StateStorage {
    Table(LuaTable),
    UserData(Rc<RefCell<Vec<f32>>>),
    Buffer(LuaBuffer),
}

impl BatchLuaCache {
    /// Allocate the per-script cache once, sized to the largest column
    /// in the first call. All outer / inner / per-column tables are
    /// created here and never re-allocated; only the underlying f32
    /// storage may grow on subsequent calls (see [`Self::grow_to`]).
    fn new(
        lua: &Lua,
        mode: BatchLuaBufferMode,
        reads: &[BatchReadColumn<'_>],
        writes: &[BatchWriteColumn<'_>],
        optional_reads: &[BatchOptionalColumn<'_>],
        state_schema: &[BatchStateField],
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<Self, BatchError> {
        // Initial capacity = max element length across all input
        // columns this call. Optional reads contribute only when
        // `Some` (absent columns are zero-length here).
        let initial = reads
            .iter()
            .map(|c| c.x.len())
            .chain(writes.iter().map(|c| c.x.len()))
            .chain(optional_reads.iter().filter_map(|c| c.value.map(|(x, _): (&[f32], &[f32])| x.len())))
            .max()
            .unwrap_or(0);

        let q = lua.create_table().map_err(make_runtime_err)?;
        let read = lua.create_table().map_err(make_runtime_err)?;
        let write = lua.create_table().map_err(make_runtime_err)?;
        let read_optional = lua.create_table().map_err(make_runtime_err)?;
        let state = lua.create_table().map_err(make_runtime_err)?;
        let ctx = lua.create_table().map_err(make_runtime_err)?;
        q.raw_set("read", read.clone()).map_err(make_runtime_err)?;
        q.raw_set("write", write.clone()).map_err(make_runtime_err)?;
        q.raw_set("read_optional", read_optional.clone()).map_err(make_runtime_err)?;
        q.raw_set("state", state.clone()).map_err(make_runtime_err)?;

        let mut read_cols = Vec::with_capacity(reads.len());
        for c in reads {
            let col = CachedColumn::alloc(lua, mode, c.name, initial, make_runtime_err)?;
            read.raw_set(c.name, col.inner.clone()).map_err(make_runtime_err)?;
            read_cols.push(col);
        }
        let mut write_cols = Vec::with_capacity(writes.len());
        for c in writes {
            let col = CachedColumn::alloc(lua, mode, c.name, initial, make_runtime_err)?;
            write.raw_set(c.name, col.inner.clone()).map_err(make_runtime_err)?;
            write_cols.push(col);
        }
        let mut optional_cols = Vec::with_capacity(optional_reads.len());
        for c in optional_reads {
            // Allocate inner storage at full initial capacity so a
            // first-time `Some` doesn't need to grow on the spot.
            let col = CachedColumn::alloc(lua, mode, c.name, initial, make_runtime_err)?;
            optional_cols.push(CachedOptionalColumn {
                column: col,
                present_last: false,
            });
        }
        let mut state_cols = Vec::with_capacity(state_schema.len());
        for f in state_schema {
            let col = CachedStateColumn::alloc(lua, mode, &f.name, initial, &state, make_runtime_err)?;
            state_cols.push(col);
        }

        Ok(Self {
            q,
            read,
            write,
            read_optional,
            state,
            ctx,
            read_cols,
            write_cols,
            optional_cols,
            state_cols,
            capacity: initial,
        })
    }

    /// Grow every cached column's underlying storage to at least
    /// `new_capacity` elements. For Lua tables the growth happens
    /// implicitly via `raw_set`; for `Vec<f32>` storage we resize; for
    /// Luau `buffer`s we must allocate a new fixed-size buffer and
    /// rebind the inner table.
    fn grow_to(
        &mut self,
        lua: &Lua,
        mode: BatchLuaBufferMode,
        new_capacity: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        debug_assert!(new_capacity > self.capacity);
        for c in self.read_cols.iter_mut() {
            c.grow_to(lua, mode, new_capacity, make_runtime_err)?;
        }
        for c in self.write_cols.iter_mut() {
            c.grow_to(lua, mode, new_capacity, make_runtime_err)?;
        }
        for c in self.optional_cols.iter_mut() {
            c.column.grow_to(lua, mode, new_capacity, make_runtime_err)?;
        }
        for c in self.state_cols.iter_mut() {
            c.grow_to(lua, mode, new_capacity, &self.state, make_runtime_err)?;
        }
        self.capacity = new_capacity;
        Ok(())
    }

    /// Apply per-call presence transition for one optional column,
    /// then refill its data when present.
    fn refill_optional(
        &mut self,
        lua: &Lua,
        mode: BatchLuaBufferMode,
        i: usize,
        c: &BatchOptionalColumn<'_>,
        count: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        let opt = &mut self.optional_cols[i];
        debug_assert_eq!(opt.column.name, c.name);
        match c.value {
            Some((x, y)) => {
                // Storage was sized to `capacity` at alloc / grow time.
                opt.column.refill_inplace(lua, mode, x, y, count, make_runtime_err)?;
                if !opt.present_last {
                    self.read_optional
                        .raw_set(opt.column.name.clone(), opt.column.inner.clone())
                        .map_err(make_runtime_err)?;
                    opt.present_last = true;
                }
            }
            None => {
                if opt.present_last {
                    self.read_optional
                        .raw_set(opt.column.name.clone(), mlua::Value::Nil)
                        .map_err(make_runtime_err)?;
                    opt.present_last = false;
                }
            }
        }
        Ok(())
    }
}

impl CachedColumn {
    fn alloc(
        lua: &Lua,
        mode: BatchLuaBufferMode,
        name: &str,
        capacity: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<Self, BatchError> {
        let inner = lua.create_table().map_err(make_runtime_err)?;
        let storage = match mode {
            BatchLuaBufferMode::Table => {
                let x = lua
                    .create_sequence_from(std::iter::repeat_n(0.0_f32, capacity))
                    .map_err(make_runtime_err)?;
                let y = lua
                    .create_sequence_from(std::iter::repeat_n(0.0_f32, capacity))
                    .map_err(make_runtime_err)?;
                inner.raw_set("x", x.clone()).map_err(make_runtime_err)?;
                inner.raw_set("y", y.clone()).map_err(make_runtime_err)?;
                ColumnStorage::Table { x, y }
            }
            BatchLuaBufferMode::UserData => {
                let xb = F32Buffer(Rc::new(RefCell::new(vec![0.0_f32; capacity])));
                let yb = F32Buffer(Rc::new(RefCell::new(vec![0.0_f32; capacity])));
                let xh = xb.0.clone();
                let yh = yb.0.clone();
                inner.raw_set("x", xb).map_err(make_runtime_err)?;
                inner.raw_set("y", yb).map_err(make_runtime_err)?;
                ColumnStorage::UserData { x: xh, y: yh }
            }
            BatchLuaBufferMode::Buffer => {
                let zeros = vec![0.0_f32; capacity];
                let x = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                let y = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                inner.raw_set("x", x.clone()).map_err(make_runtime_err)?;
                inner.raw_set("y", y.clone()).map_err(make_runtime_err)?;
                ColumnStorage::Buffer { x, y }
            }
        };
        Ok(Self {
            name: name.to_string(),
            inner,
            storage,
        })
    }

    /// Grow underlying storage so it can hold at least `new_capacity`
    /// elements. For Table mode, we set slots `capacity+1..=new` to 0;
    /// for UserData (Vec<f32>) we resize; for Buffer we allocate a
    /// new fixed-length buffer and rebind the inner table.
    fn grow_to(
        &mut self,
        lua: &Lua,
        mode: BatchLuaBufferMode,
        new_capacity: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        match (&mut self.storage, mode) {
            (ColumnStorage::Table { x, y }, BatchLuaBufferMode::Table) => {
                // Grow the Lua sequence table by setting slots beyond
                // its current length. `refill_inplace` will then
                // overwrite slots 1..=count with caller data.
                let cur_len = x.raw_len();
                for i in (cur_len + 1)..=new_capacity {
                    x.raw_set(i as i64, 0.0_f32).map_err(make_runtime_err)?;
                    y.raw_set(i as i64, 0.0_f32).map_err(make_runtime_err)?;
                }
            }
            (ColumnStorage::UserData { x, y }, BatchLuaBufferMode::UserData) => {
                x.borrow_mut().resize(new_capacity, 0.0);
                y.borrow_mut().resize(new_capacity, 0.0);
            }
            (storage @ ColumnStorage::Buffer { .. }, BatchLuaBufferMode::Buffer) => {
                let zeros = vec![0.0_f32; new_capacity];
                let x_new = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                let y_new = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                self.inner
                    .raw_set("x", x_new.clone())
                    .map_err(make_runtime_err)?;
                self.inner
                    .raw_set("y", y_new.clone())
                    .map_err(make_runtime_err)?;
                *storage = ColumnStorage::Buffer { x: x_new, y: y_new };
            }
            _ => unreachable!("cache buffer mode must match runtime buffer mode"),
        }
        Ok(())
    }

    /// Copy `count` elements from caller-owned slices into the cached
    /// Lua-side storage. Storage is assumed to be sized >= count
    /// (caller must call [`Self::grow_to`] first).
    fn refill(
        &mut self,
        lua: &Lua,
        mode: BatchLuaBufferMode,
        src_x: &[f32],
        src_y: &[f32],
        count: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        self.refill_inplace(lua, mode, src_x, src_y, count, make_runtime_err)
    }

    fn refill_inplace(
        &mut self,
        lua: &Lua,
        _mode: BatchLuaBufferMode,
        src_x: &[f32],
        src_y: &[f32],
        count: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        debug_assert_eq!(src_x.len(), count);
        debug_assert_eq!(src_y.len(), count);
        match &mut self.storage {
            ColumnStorage::Table { x, y } => {
                // `create_sequence_from` is ~2x faster than a per-element
                // `raw_set` loop because it amortises the mlua FFI overhead
                // (one stack guard + one push_ref, then raw lua_rawseti per
                // element). Allocating a fresh sequence per call is the
                // tradeoff; profiling at 5k/10k entities shows this still
                // wins overall vs reusing a cached table with raw_set.
                let new_x = lua
                    .create_sequence_from(src_x[..count].iter().copied())
                    .map_err(make_runtime_err)?;
                let new_y = lua
                    .create_sequence_from(src_y[..count].iter().copied())
                    .map_err(make_runtime_err)?;
                self.inner.raw_set("x", new_x.clone()).map_err(make_runtime_err)?;
                self.inner.raw_set("y", new_y.clone()).map_err(make_runtime_err)?;
                *x = new_x;
                *y = new_y;
            }
            ColumnStorage::UserData { x, y } => {
                x.borrow_mut()[..count].copy_from_slice(src_x);
                y.borrow_mut()[..count].copy_from_slice(src_y);
            }
            ColumnStorage::Buffer { x, y } => {
                x.write_bytes(0, f32_slice_as_bytes(src_x));
                y.write_bytes(0, f32_slice_as_bytes(src_y));
            }
        }
        Ok(())
    }

    /// Copy `count` elements out of cached Lua-side storage into
    /// caller-owned slices. `dst_x.len()` is the count.
    ///
    /// Slice 5.4b — dirty masks: returns the number of bytes actually
    /// written. The caller pre-populates `dst_x` / `dst_y` with the
    /// pre-script ECS values (this is the existing read-modify-write
    /// invariant); we compare against those to elide writebacks when
    /// the script left a column unchanged. Granularity differs by
    /// mode:
    ///
    /// - **Table** mode iterates element-wise anyway (one `raw_get`
    ///   per slot), so we get **per-element** dirty granularity for
    ///   free — only changed slots are stored back.
    /// - **UserData** mode keeps the existing single bulk `copy_from_slice`
    ///   path; we do a **whole-column** `==` compare first and skip
    ///   the copy if every element matches. The compare is one
    ///   `memcmp` over `count * 4` bytes, which is much cheaper than
    ///   the copy it can elide.
    /// - **Buffer** mode already allocates a `Vec<u8>` via
    ///   `LuaBuffer::to_vec`; we reinterpret it as `&[f32]`, do the
    ///   same whole-column compare, and skip the final
    ///   `copy_from_slice` on match.
    ///
    /// Returns `(dirty_x_bytes, dirty_y_bytes)`. Sum is at most
    /// `count * 8`. NaN compares non-equal to itself, so a NaN column
    /// is always reported dirty (sound but conservative).
    fn copy_out(
        &self,
        dst_x: &mut [f32],
        dst_y: &mut [f32],
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(u64, u64), BatchError> {
        let count = dst_x.len();
        debug_assert_eq!(count, dst_y.len());
        let force = dirty_disabled();
        match &self.storage {
            ColumnStorage::Table { x, y } => {
                let mut dirty_x = 0u64;
                let mut dirty_y = 0u64;
                for i in 0..count {
                    let nx: f32 = x.raw_get((i + 1) as i64).map_err(make_runtime_err)?;
                    let ny: f32 = y.raw_get((i + 1) as i64).map_err(make_runtime_err)?;
                    // Bit-equality so the elision treats `-0.0` and `+0.0`
                    // as distinct (ECS observers may care). Skips NaN→NaN
                    // collapse too; that's the conservative direction.
                    if force || nx.to_bits() != dst_x[i].to_bits() {
                        dst_x[i] = nx;
                        dirty_x += std::mem::size_of::<f32>() as u64;
                    }
                    if force || ny.to_bits() != dst_y[i].to_bits() {
                        dst_y[i] = ny;
                        dirty_y += std::mem::size_of::<f32>() as u64;
                    }
                }
                Ok((dirty_x, dirty_y))
            }
            ColumnStorage::UserData { x, y } => {
                let xb = x.borrow();
                let yb = y.borrow();
                let xs = &xb[..count];
                let ys = &yb[..count];
                let dx = if force || dst_x != xs {
                    dst_x.copy_from_slice(xs);
                    std::mem::size_of_val(xs) as u64
                } else {
                    0
                };
                let dy = if force || dst_y != ys {
                    dst_y.copy_from_slice(ys);
                    std::mem::size_of_val(ys) as u64
                } else {
                    0
                };
                Ok((dx, dy))
            }
            ColumnStorage::Buffer { x, y } => {
                let dx = copy_buffer_into_f32_slice_dirty(x, dst_x);
                let dy = copy_buffer_into_f32_slice_dirty(y, dst_y);
                Ok((dx, dy))
            }
        }
    }
}

impl CachedStateColumn {
    fn alloc(
        lua: &Lua,
        mode: BatchLuaBufferMode,
        name: &str,
        capacity: usize,
        state_outer: &LuaTable,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<Self, BatchError> {
        let storage = match mode {
            BatchLuaBufferMode::Table => {
                let t = lua
                    .create_sequence_from(std::iter::repeat_n(0.0_f32, capacity))
                    .map_err(make_runtime_err)?;
                state_outer
                    .raw_set(name, t.clone())
                    .map_err(make_runtime_err)?;
                StateStorage::Table(t)
            }
            BatchLuaBufferMode::UserData => {
                let buf = F32Buffer(Rc::new(RefCell::new(vec![0.0_f32; capacity])));
                let rc = buf.0.clone();
                state_outer
                    .raw_set(name, buf)
                    .map_err(make_runtime_err)?;
                StateStorage::UserData(rc)
            }
            BatchLuaBufferMode::Buffer => {
                let zeros = vec![0.0_f32; capacity];
                let b = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                state_outer
                    .raw_set(name, b.clone())
                    .map_err(make_runtime_err)?;
                StateStorage::Buffer(b)
            }
        };
        Ok(Self {
            name: name.to_string(),
            storage,
        })
    }

    /// Grow underlying storage to at least `new_capacity` elements.
    /// For Buffer mode the underlying Luau buffer is fixed-size, so we
    /// recreate it and rebind the entry on the `state` outer table.
    /// For UserData mode the F32Buffer userdata sits in the `state`
    /// outer table holding a clone of the same Rc we kept; resizing
    /// the Vec via the Rc is enough.
    fn grow_to(
        &mut self,
        lua: &Lua,
        mode: BatchLuaBufferMode,
        new_capacity: usize,
        state_outer: &LuaTable,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        match (&mut self.storage, mode) {
            (StateStorage::Table(t), BatchLuaBufferMode::Table) => {
                let cur_len = t.raw_len();
                for i in (cur_len + 1)..=new_capacity {
                    t.raw_set(i as i64, 0.0_f32).map_err(make_runtime_err)?;
                }
            }
            (StateStorage::UserData(rc), BatchLuaBufferMode::UserData) => {
                rc.borrow_mut().resize(new_capacity, 0.0);
            }
            (storage @ StateStorage::Buffer(_), BatchLuaBufferMode::Buffer) => {
                let zeros = vec![0.0_f32; new_capacity];
                let new_buf = lua
                    .create_buffer(f32_slice_as_bytes(&zeros))
                    .map_err(make_runtime_err)?;
                state_outer
                    .raw_set(self.name.clone(), new_buf.clone())
                    .map_err(make_runtime_err)?;
                *storage = StateStorage::Buffer(new_buf);
            }
            _ => unreachable!("cache buffer mode must match runtime buffer mode"),
        }
        Ok(())
    }

    fn refill(
        &mut self,
        lua: &Lua,
        _mode: BatchLuaBufferMode,
        state_outer: &LuaTable,
        src: &[f32],
        count: usize,
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<(), BatchError> {
        debug_assert_eq!(src.len(), count);
        match &mut self.storage {
            StateStorage::Table(t) => {
                // See `CachedColumn::refill_inplace` for why we recreate
                // the sequence rather than overwriting via raw_set in a
                // loop.
                let new_t = lua
                    .create_sequence_from(src[..count].iter().copied())
                    .map_err(make_runtime_err)?;
                state_outer
                    .raw_set(self.name.clone(), new_t.clone())
                    .map_err(make_runtime_err)?;
                *t = new_t;
            }
            StateStorage::UserData(rc) => {
                rc.borrow_mut()[..count].copy_from_slice(src);
            }
            StateStorage::Buffer(b) => {
                b.write_bytes(0, f32_slice_as_bytes(src));
            }
        }
        Ok(())
    }

    /// Slice 5.4b — see [`CachedColumn::copy_out`] for the dirty-mask
    /// contract. Returns bytes actually written into `dst`.
    fn copy_out(
        &self,
        dst: &mut [f32],
        make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
    ) -> Result<u64, BatchError> {
        let count = dst.len();
        let force = dirty_disabled();
        match &self.storage {
            StateStorage::Table(t) => {
                let mut dirty = 0u64;
                for (i, slot) in dst.iter_mut().enumerate() {
                    let nv: f32 = t.raw_get((i + 1) as i64).map_err(make_runtime_err)?;
                    if force || nv.to_bits() != slot.to_bits() {
                        *slot = nv;
                        dirty += std::mem::size_of::<f32>() as u64;
                    }
                }
                Ok(dirty)
            }
            StateStorage::UserData(rc) => {
                let src = rc.borrow();
                let s = &src[..count];
                if force || dst != s {
                    dst.copy_from_slice(s);
                    Ok(std::mem::size_of_val(s) as u64)
                } else {
                    Ok(0)
                }
            }
            StateStorage::Buffer(b) => Ok(copy_buffer_into_f32_slice_dirty(b, dst)),
        }
    }
}

/// One Luau VM per process, shared across all batch Lua systems.
///
/// **Sandbox**: `lua.sandbox(true)` (matches `LuauAdapter`). Each script
/// is loaded into a fresh per-script env table that inherits globals via
/// `__index`, so two scripts can't accidentally trample each other's
/// `update` global.
pub struct BatchLuaRuntime {
    scripts: HashMap<String, LoadedBatchScript>,
    buffer_mode: BatchLuaBufferMode,
    lua: Lua,
    /// Slice 5.4a A/B toggle: when set (via env
    /// `JENGINE_BATCH_LUA_NO_CACHE=1`), the per-script handle cache is
    /// dropped at the end of every `run_batch` call so the next frame
    /// rebuilds from scratch — matching pre-5.4a behavior. Used only
    /// for back-to-back baseline captures.
    no_cache: bool,
    /// Slice 5.5c — deterministic RNG that backs the in-script
    /// `math.random` shim. Reseeded by [`Self::set_rng_seed`] before
    /// each `run_batch` invocation.
    rng: Rc<RefCell<DetRng>>,
}

impl BatchLuaRuntime {
    pub fn new(buffer_mode: BatchLuaBufferMode) -> Result<Self, BatchError> {
        // Its maths through `libm`, as every script's (`script_math`).
        crate::scripting::script_math::install_luau_table();
        let lua = Lua::new();
        let no_cache = std::env::var("JENGINE_BATCH_LUA_NO_CACHE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        // Slice 5.5c — install deterministic `math.random` shim that
        // matches the standard Lua signatures:
        //   math.random()      -> f64 in [0, 1)
        //   math.random(n)     -> int in [1, n]
        //   math.random(m, n)  -> int in [m, n]
        let rng = Rc::new(RefCell::new(DetRng::new(0)));
        let rng_for_lua = rng.clone();
        let rand_fn = lua
            .create_function(move |_, args: mlua::MultiValue| -> LuaResult<LuaValue> {
                let f = rng_for_lua.borrow_mut().next_f64_unit();
                match args.len() {
                    0 => Ok(LuaValue::Number(f)),
                    1 => {
                        let n = args[0].as_integer().ok_or_else(|| {
                            mlua::Error::RuntimeError(
                                "math.random(n): n must be an integer".into(),
                            )
                        })?;
                        if n < 1 {
                            return Err(mlua::Error::RuntimeError(
                                "math.random(n): n must be >= 1".into(),
                            ));
                        }
                        let v = (f * n as f64).floor() as i64 + 1;
                        Ok(LuaValue::Integer((v.min(n as i64)) as mlua::Integer))
                    }
                    _ => {
                        let m = args[0].as_integer().ok_or_else(|| {
                            mlua::Error::RuntimeError(
                                "math.random(m, n): m must be an integer".into(),
                            )
                        })?;
                        let n = args[1].as_integer().ok_or_else(|| {
                            mlua::Error::RuntimeError(
                                "math.random(m, n): n must be an integer".into(),
                            )
                        })?;
                        if n < m {
                            return Err(mlua::Error::RuntimeError(
                                "math.random(m, n): n must be >= m".into(),
                            ));
                        }
                        let span = (n as i64 - m as i64 + 1) as f64;
                        let v = m as i64 + (f * span).floor() as i64;
                        Ok(LuaValue::Integer((v.min(n as i64)) as mlua::Integer))
                    }
                }
            })
            .map_err(|e| BatchError::Init(format!("creating math.random shim: {e}")))?;
        let math: LuaTable = lua
            .globals()
            .get("math")
            .map_err(|e| BatchError::Init(format!("loading math global: {e}")))?;
        math.set("random", rand_fn)
            .map_err(|e| BatchError::Init(format!("installing math.random shim: {e}")))?;
        // Enable sandbox *after* installing the deterministic shim:
        // sandbox snapshots globals as read-only, so any subsequent
        // write to the `math` table would fail.
        lua.sandbox(true).map_err(|e| BatchError::Init(e.to_string()))?;
        Ok(Self { scripts: HashMap::new(), buffer_mode, lua, no_cache, rng })
    }

    /// Slice 5.5c — reseed the deterministic RNG. Worker calls this
    /// before each `run_batch` with a per-chunk seed.
    pub fn set_rng_seed(&self, seed: u64) {
        self.rng.borrow_mut().reseed(seed);
    }

    /// Slice 5.5c — current Lua allocator footprint in bytes.
    pub fn memory_bytes(&self) -> u64 {
        self.lua.used_memory() as u64
    }

    pub fn buffer_mode(&self) -> BatchLuaBufferMode {
        self.buffer_mode
    }

    pub fn is_loaded(&self, path: &str) -> bool {
        self.scripts.contains_key(path)
    }

    pub fn loaded_count(&self) -> usize {
        self.scripts.len()
    }

    /// Slice 5.1b: declared state schema for `path`, in the script's
    /// (alphabetically sorted) field order. Returns `Some(&[])` for
    /// loaded scripts that declared no state, `None` for unknown paths.
    /// Mirrors [`crate::scripting::batch::BatchJsRuntime::state_schema`].
    pub fn state_schema(&self, path: &str) -> Option<&[BatchStateField]> {
        self.scripts.get(path).map(|s| {
            s.spec
                .as_ref()
                .map(|sp| sp.state.as_slice())
                .unwrap_or(&[])
        })
    }

    /// Slice 5.2: declared optional read components. Mirrors
    /// [`crate::scripting::batch::BatchJsRuntime::read_optional_schema`].
    pub fn read_optional_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts.get(path).map(|s| {
            s.spec
                .as_ref()
                .map(|sp| sp.read_optional.as_slice())
                .unwrap_or(&[])
        })
    }

    /// Slice 5.3: declared `read` components for a loaded script.
    pub fn read_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts.get(path).map(|s| {
            s.spec
                .as_ref()
                .map(|sp| sp.read.as_slice())
                .unwrap_or(&[])
        })
    }

    /// Slice 5.3: declared `write` components for a loaded script.
    pub fn write_schema(&self, path: &str) -> Option<&[String]> {
        self.scripts.get(path).map(|s| {
            s.spec
                .as_ref()
                .map(|sp| sp.write.as_slice())
                .unwrap_or(&[])
        })
    }

    /// Load a Lua source as a batch system. The source executes in a
    /// fresh per-script env table that inherits globals via `__index`,
    /// so each script's `update` / `system` declarations are isolated.
    ///
    /// Same `system.query.{read,write}` validation rules as
    /// [`crate::scripting::batch::BatchJsRuntime::load`]: missing header
    /// triggers a one-shot warn; mismatched read/write fails loudly.
    pub fn load(&mut self, path: &str, source: &str) -> Result<(), BatchError> {
        let env = self
            .new_script_env()
            .map_err(|e| BatchError::Load { path: path.to_string(), message: e.to_string() })?;

        // Run the chunk against the per-script env. After exec, the env
        // table holds the script's top-level decls (`update`, `system`).
        self.lua
            .load(source)
            .set_name(path)
            .set_environment(env.clone())
            .exec()
            .map_err(|e| BatchError::Load { path: path.to_string(), message: e.to_string() })?;

        let update: LuaFunction = env.get("update").map_err(|_| BatchError::MissingUpdate {
            path: path.to_string(),
        })?;

        let spec = parse_system_spec(&env);
        if let Some(s) = &spec {
            validate_walking_skeleton_spec(path, s)?;
        } else {
            log::warn!(
                "[BatchScript:lua] '{}' has no `system` header — accepting with the default \
                 Velocity2D → Position2D contract. Add `system = {{ query = {{ \
                 read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}} }} }}` to silence this warn.",
                path
            );
        }

        self.scripts.insert(
            path.to_string(),
            LoadedBatchScript {
                update,
                path: path.to_string(),
                spec,
                cache: RefCell::new(None),
            },
        );
        Ok(())
    }

    fn new_script_env(&self) -> LuaResult<LuaTable> {
        let env = self.lua.create_table()?;
        let meta = self.lua.create_table()?;
        meta.set("__index", self.lua.globals())?;
        env.set_metatable(Some(meta));
        Ok(env)
    }

    /// Walking-skeleton update — same shape as the JS runtime. See
    /// [`crate::scripting::batch::BatchJsRuntime::run_velocity_to_position`].
    ///
    /// Slice 5.1b: `state` is one column per declared field on the
    /// script's `system.query.state` schema, in alphabetical order.
    /// Empty for scripts with no state. Each column has length `count`.
    /// Columns are exposed to the script as `q.state.<name>` using the
    /// runtime's [`BatchLuaBufferMode`] (Table/UserData/Buffer), then
    /// copied back into the caller's slices after `update` returns.
    pub fn run_velocity_to_position(
        &self,
        path: &str,
        dt: f32,
        velocity_x: &[f32],
        velocity_y: &[f32],
        position_x: &mut [f32],
        position_y: &mut [f32],
        state: &mut [Vec<f32>],
        // Slice 5.2: optional Acceleration2D column. Same contract as
        // BatchJsRuntime::run_velocity_to_position — Some when EVERY
        // entity in this batch has the component, None otherwise.
        accel: Option<(&[f32], &[f32])>,
        counters: &mut FrameCounters,
    ) -> Result<u64, BatchError> {
        debug_assert_eq!(velocity_x.len(), velocity_y.len());
        debug_assert_eq!(position_x.len(), position_y.len());
        debug_assert_eq!(velocity_x.len(), position_x.len());
        if let Some((ax, ay)) = accel {
            debug_assert_eq!(ax.len(), velocity_x.len(), "accel x column wrong length");
            debug_assert_eq!(ay.len(), velocity_x.len(), "accel y column wrong length");
        }

        let count = velocity_x.len();
        if count == 0 {
            return Ok(0);
        }

        let script = self
            .scripts
            .get(path)
            .ok_or_else(|| BatchError::MissingUpdate { path: path.to_string() })?;

        // Slice 5.1b: validate state column shape against the schema
        // *and* against `count`. Caller (the ECS dispatcher) is
        // responsible for both, so a mismatch is a host-side bug.
        let state_schema: &[BatchStateField] = script
            .spec
            .as_ref()
            .map(|s| s.state.as_slice())
            .unwrap_or(&[]);
        debug_assert_eq!(
            state.len(),
            state_schema.len(),
            "state column count must match schema (got {}, expected {})",
            state.len(),
            state_schema.len(),
        );
        for (i, col) in state.iter().enumerate() {
            debug_assert_eq!(
                col.len(),
                count,
                "state column #{i} has wrong length (got {}, expected {count})",
                col.len(),
            );
        }

        let lua = &self.lua;
        let path_owned = path.to_string();
        let make_runtime_err = |e: mlua::Error| BatchError::RunFailed {
            path: path_owned.clone(),
            message: e.to_string(),
        };

        // ── copy-in ──────────────────────────────────────────────────
        let copy_in_start = Instant::now();
        let (q, write_handles) = match self.buffer_mode {
            BatchLuaBufferMode::Table => {
                let vx = lua.create_sequence_from(velocity_x.iter().copied()).map_err(&make_runtime_err)?;
                let vy = lua.create_sequence_from(velocity_y.iter().copied()).map_err(&make_runtime_err)?;
                let px = lua.create_sequence_from(position_x.iter().copied()).map_err(&make_runtime_err)?;
                let py = lua.create_sequence_from(position_y.iter().copied()).map_err(&make_runtime_err)?;

                let read_vel = lua.create_table().map_err(&make_runtime_err)?;
                read_vel.set("x", vx).map_err(&make_runtime_err)?;
                read_vel.set("y", vy).map_err(&make_runtime_err)?;
                let read = lua.create_table().map_err(&make_runtime_err)?;
                read.set("Velocity2D", read_vel).map_err(&make_runtime_err)?;

                let write_pos = lua.create_table().map_err(&make_runtime_err)?;
                write_pos.set("x", px.clone()).map_err(&make_runtime_err)?;
                write_pos.set("y", py.clone()).map_err(&make_runtime_err)?;
                let write = lua.create_table().map_err(&make_runtime_err)?;
                write.set("Position2D", write_pos).map_err(&make_runtime_err)?;

                let q = lua.create_table().map_err(&make_runtime_err)?;
                q.set("count", count as i64).map_err(&make_runtime_err)?;
                q.set("read", read).map_err(&make_runtime_err)?;
                q.set("write", write).map_err(&make_runtime_err)?;
                (q, WriteHandles::Table { px, py })
            }
            BatchLuaBufferMode::UserData => {
                let vx_buf = F32Buffer::from_slice(velocity_x);
                let vy_buf = F32Buffer::from_slice(velocity_y);
                let px_buf = F32Buffer::from_slice(position_x);
                let py_buf = F32Buffer::from_slice(position_y);
                // Keep the `Rc<RefCell<Vec<f32>>>` clones for readback.
                let px_handle = px_buf.0.clone();
                let py_handle = py_buf.0.clone();

                let read_vel = lua.create_table().map_err(&make_runtime_err)?;
                read_vel.set("x", vx_buf).map_err(&make_runtime_err)?;
                read_vel.set("y", vy_buf).map_err(&make_runtime_err)?;
                let read = lua.create_table().map_err(&make_runtime_err)?;
                read.set("Velocity2D", read_vel).map_err(&make_runtime_err)?;

                let write_pos = lua.create_table().map_err(&make_runtime_err)?;
                write_pos.set("x", px_buf).map_err(&make_runtime_err)?;
                write_pos.set("y", py_buf).map_err(&make_runtime_err)?;
                let write = lua.create_table().map_err(&make_runtime_err)?;
                write.set("Position2D", write_pos).map_err(&make_runtime_err)?;

                let q = lua.create_table().map_err(&make_runtime_err)?;
                q.set("count", count as i64).map_err(&make_runtime_err)?;
                q.set("read", read).map_err(&make_runtime_err)?;
                q.set("write", write).map_err(&make_runtime_err)?;
                (q, WriteHandles::UserData { px_handle, py_handle })
            }
            BatchLuaBufferMode::Buffer => {
                // Reinterpret f32 columns as raw bytes for `create_buffer`.
                // `&[f32]` -> `&[u8]` is a sound non-aliasing read.
                let vx_buf = lua
                    .create_buffer(f32_slice_as_bytes(velocity_x))
                    .map_err(&make_runtime_err)?;
                let vy_buf = lua
                    .create_buffer(f32_slice_as_bytes(velocity_y))
                    .map_err(&make_runtime_err)?;
                let px_buf = lua
                    .create_buffer(f32_slice_as_bytes(position_x))
                    .map_err(&make_runtime_err)?;
                let py_buf = lua
                    .create_buffer(f32_slice_as_bytes(position_y))
                    .map_err(&make_runtime_err)?;
                // Clone the `Buffer` handles for readback (cheap: it's
                // a `ValueRef` clone, the underlying Luau buffer is
                // shared).
                let px_handle = px_buf.clone();
                let py_handle = py_buf.clone();

                let read_vel = lua.create_table().map_err(&make_runtime_err)?;
                read_vel.set("x", vx_buf).map_err(&make_runtime_err)?;
                read_vel.set("y", vy_buf).map_err(&make_runtime_err)?;
                let read = lua.create_table().map_err(&make_runtime_err)?;
                read.set("Velocity2D", read_vel).map_err(&make_runtime_err)?;

                let write_pos = lua.create_table().map_err(&make_runtime_err)?;
                write_pos.set("x", px_buf).map_err(&make_runtime_err)?;
                write_pos.set("y", py_buf).map_err(&make_runtime_err)?;
                let write = lua.create_table().map_err(&make_runtime_err)?;
                write.set("Position2D", write_pos).map_err(&make_runtime_err)?;

                let q = lua.create_table().map_err(&make_runtime_err)?;
                q.set("count", count as i64).map_err(&make_runtime_err)?;
                q.set("read", read).map_err(&make_runtime_err)?;
                q.set("write", write).map_err(&make_runtime_err)?;
                (q, WriteHandles::Buffer { px_handle, py_handle })
            }
        };

        // Slice 5.1b: build q.state.<name> with the same buffer mode as
        // read/write, and keep parallel handles for copy-back.
        let state_handles = build_state_columns(
            lua,
            self.buffer_mode,
            state_schema,
            state,
            &q,
            &make_runtime_err,
        )?;

        // Slice 5.2: build q.read_optional.Acceleration2D using the
        // same buffer mode (read-only — no handles needed for
        // copy-back). Always sets q.read_optional so scripts can
        // probe defensively even when the table is empty.
        build_read_optional_columns(
            lua,
            self.buffer_mode,
            accel,
            &q,
            &make_runtime_err,
        )?;

        let ctx_obj = lua.create_table().map_err(&make_runtime_err)?;
        ctx_obj.set("dt", dt).map_err(&make_runtime_err)?;

        let copy_in_us = copy_in_start.elapsed().as_micros() as u64;

        // ── run ──────────────────────────────────────────────────────
        let run_start = Instant::now();
        let _: LuaValue = script
            .update
            .call((ctx_obj, q))
            .map_err(&make_runtime_err)?;
        let run_us = run_start.elapsed().as_micros() as u64;

        // ── copy-out ─────────────────────────────────────────────────
        let copy_out_start = Instant::now();
        let f32_size = std::mem::size_of::<f32>() as u64;
        let mut dirty_bytes: u64 = 0;
        let force = dirty_disabled();
        match write_handles {
            WriteHandles::Table { px, py } => {
                for i in 0..count {
                    // Lua table is 1-indexed. Slice 5.4b: per-element
                    // dirty mask via bit-equality compare.
                    let nx: f32 = px.get((i + 1) as i64).map_err(&make_runtime_err)?;
                    let ny: f32 = py.get((i + 1) as i64).map_err(&make_runtime_err)?;
                    if force || nx.to_bits() != position_x[i].to_bits() {
                        position_x[i] = nx;
                        dirty_bytes += f32_size;
                    }
                    if force || ny.to_bits() != position_y[i].to_bits() {
                        position_y[i] = ny;
                        dirty_bytes += f32_size;
                    }
                }
            }
            WriteHandles::UserData { px_handle, py_handle } => {
                let xs = px_handle.borrow();
                let ys = py_handle.borrow();
                if force || position_x != &xs[..] {
                    position_x.copy_from_slice(&xs);
                    dirty_bytes += std::mem::size_of_val(&xs[..]) as u64;
                }
                if force || position_y != &ys[..] {
                    position_y.copy_from_slice(&ys);
                    dirty_bytes += std::mem::size_of_val(&ys[..]) as u64;
                }
            }
            WriteHandles::Buffer { px_handle, py_handle } => {
                dirty_bytes += copy_buffer_into_f32_slice_dirty(&px_handle, position_x);
                dirty_bytes += copy_buffer_into_f32_slice_dirty(&py_handle, position_y);
            }
        }
        // Slice 5.1b: scatter state columns back into the caller's
        // slices, in schema order. Slice 5.4b: same dirty-mask logic.
        match state_handles {
            StateHandles::Table(handles) => {
                for (col, t) in state.iter_mut().zip(handles.iter()) {
                    for (i, slot) in col.iter_mut().enumerate().take(count) {
                        let nv: f32 = t.get((i + 1) as i64).map_err(&make_runtime_err)?;
                        if force || nv.to_bits() != slot.to_bits() {
                            *slot = nv;
                            dirty_bytes += f32_size;
                        }
                    }
                }
            }
            StateHandles::UserData(handles) => {
                for (col, h) in state.iter_mut().zip(handles.iter()) {
                    let src = h.borrow();
                    if force || col.as_slice() != &src[..] {
                        col.copy_from_slice(&src);
                        dirty_bytes += std::mem::size_of_val(&src[..]) as u64;
                    }
                }
            }
            StateHandles::Buffer(handles) => {
                for (col, h) in state.iter_mut().zip(handles.iter()) {
                    dirty_bytes += copy_buffer_into_f32_slice_dirty(h, col);
                }
            }
        }
        let copy_out_us = copy_out_start.elapsed().as_micros() as u64;

        // Slice 5.1b: counters scale with declared state column count
        // so JS and Lua paths report comparable byte budgets.
        // Slice 5.2: optional accel adds 2 extra in-columns when present.
        let n_state = state.len() as u64;
        let n_opt = if accel.is_some() { 2u64 } else { 0 };
        let bytes_per_col = std::mem::size_of_val(velocity_x) as u64;
        counters.batch_copy_in_bytes += bytes_per_col * (4 + n_state + n_opt);
        counters.batch_copy_out_bytes += bytes_per_col * (2 + n_state);
        // Slice 5.4b: actual dirty bytes (post-skip).
        counters.batch_dirty_bytes += dirty_bytes;
        counters.batch_copy_in_us += copy_in_us;
        counters.batch_run_us += run_us;
        counters.batch_copy_out_us += copy_out_us;

        Ok(run_us)
    }

    /// Slice 5.3 generic dispatch: any subset of [`KNOWN_BATCH_COMPONENTS`]
    /// declared in `read` / `write` / `read_optional`. Same contract as
    /// [`BatchJsRuntime::run_batch`] — each batch is archetype-uniform
    /// so every `optional_reads[i].value` is either fully `Some` or
    /// fully `None` for this call.
    ///
    /// Slice 5.4a: reuses cached Lua-side handles across calls. The
    /// `q` / `read` / `write` / `read_optional` / `state` / per-column
    /// inner tables and per-mode storage (Lua sequence tables, F32Buffer
    /// `Vec<f32>`s, Luau `buffer`s) are allocated once per (script ×
    /// buffer-mode), then refilled in place each frame. Storage grows
    /// monotonically: a frame with `count=12000` after one with
    /// `count=30000` writes only slots 1..=12000 (Lua tables) /
    /// bytes 0..count*4 (Buffers) and leaves the rest stale; the script
    /// honors `q.count` so the stale tail is never observed.
    /// Slice 5.4c: `write_dirty` is an optional output channel for the
    /// dispatcher. When `write_dirty.len() == writes.len()`, the runtime
    /// fills slot `i` with `true` iff the script left write column `i`'s
    /// x **or** y axis non-bit-equal to its pre-script value (per-element
    /// in Table mode, whole-axis in UserData/Buffer modes — both
    /// collapse to the same column-level bool here). Pass `&mut []` to
    /// opt out.
    pub fn run_batch(
        &self,
        path: &str,
        dt: f32,
        reads: &[BatchReadColumn<'_>],
        writes: &mut [BatchWriteColumn<'_>],
        optional_reads: &[BatchOptionalColumn<'_>],
        state: &mut [Vec<f32>],
        write_dirty: &mut [bool],
        counters: &mut FrameCounters,
    ) -> Result<u64, BatchError> {
        debug_assert!(
            write_dirty.is_empty() || write_dirty.len() == writes.len(),
            "write_dirty must be empty or match writes.len()"
        );
        for d in write_dirty.iter_mut() {
            *d = false;
        }
        let count = reads
            .first()
            .map(|c| c.x.len())
            .or_else(|| writes.first().map(|c| c.x.len()))
            .or_else(|| optional_reads.iter().find_map(|c| c.value.map(|(x, _): (&[f32], &[f32])| x.len())))
            .unwrap_or(0);
        if count == 0 {
            return Ok(0);
        }

        let script = self
            .scripts
            .get(path)
            .ok_or_else(|| BatchError::MissingUpdate { path: path.to_string() })?;

        let state_schema: &[BatchStateField] = script
            .spec
            .as_ref()
            .map(|s| s.state.as_slice())
            .unwrap_or(&[]);
        debug_assert_eq!(state.len(), state_schema.len());

        let lua = &self.lua;
        let path_owned = path.to_string();
        let make_runtime_err = |e: mlua::Error| BatchError::RunFailed {
            path: path_owned.clone(),
            message: e.to_string(),
        };

        // ── ensure the per-script cache exists, then refill in place ──
        let mut cache_slot = script.cache.borrow_mut();

        let copy_in_start = Instant::now();

        // Slice 5.4a A/B toggle: when no_cache is set, drop any
        // existing cache so this frame rebuilds from scratch — this
        // keeps the BatchLuaCache::new cost inside the `cp_in`
        // measurement, faithfully matching pre-5.4a behavior.
        if self.no_cache {
            *cache_slot = None;
        }

        if cache_slot.is_none() {
            *cache_slot = Some(BatchLuaCache::new(
                lua,
                self.buffer_mode,
                reads,
                writes,
                optional_reads,
                state_schema,
                &make_runtime_err,
            )?);
        }
        let cache = cache_slot.as_mut().expect("cache just initialized");

        // Grow storage if this is the largest call we've seen yet.
        if count > cache.capacity {
            cache.grow_to(lua, self.buffer_mode, count, &make_runtime_err)?;
        }

        // Refill read / write / optional / state column data in place.
        for (i, c) in reads.iter().enumerate() {
            debug_assert_eq!(cache.read_cols[i].name, c.name);
            cache.read_cols[i].refill(lua, self.buffer_mode, c.x, c.y, count, &make_runtime_err)?;
        }
        for (i, c) in writes.iter().enumerate() {
            debug_assert_eq!(cache.write_cols[i].name, c.name);
            cache.write_cols[i].refill(lua, self.buffer_mode, c.x, c.y, count, &make_runtime_err)?;
        }
        for (i, c) in optional_reads.iter().enumerate() {
            cache.refill_optional(lua, self.buffer_mode, i, c, count, &make_runtime_err)?;
        }
        for (i, col) in state.iter().enumerate() {
            cache.state_cols[i].refill(lua, self.buffer_mode, &cache.state, col, count, &make_runtime_err)?;
        }

        cache.q.raw_set("count", count as i64).map_err(&make_runtime_err)?;
        cache.ctx.raw_set("dt", dt).map_err(&make_runtime_err)?;

        // Snapshot Lua values we hand to the script. `LuaTable: Clone`
        // is a cheap `ValueRef` clone (no Lua-side work).
        let ctx_obj = cache.ctx.clone();
        let q = cache.q.clone();

        let copy_in_us = copy_in_start.elapsed().as_micros() as u64;

        let run_start = Instant::now();
        let _: LuaValue = script
            .update
            .call((ctx_obj, q))
            .map_err(&make_runtime_err)?;
        let run_us = run_start.elapsed().as_micros() as u64;

        // ── copy-out write columns ──
        let copy_out_start = Instant::now();
        let mut dirty_bytes: u64 = 0;
        for (i, c) in writes.iter_mut().enumerate() {
            let (dx, dy) = cache.write_cols[i].copy_out(c.x, c.y, &make_runtime_err)?;
            dirty_bytes += dx + dy;
            if let Some(slot) = write_dirty.get_mut(i) {
                *slot = (dx + dy) > 0;
            }
        }
        for (i, col) in state.iter_mut().enumerate() {
            dirty_bytes += cache.state_cols[i].copy_out(col, &make_runtime_err)?;
        }
        let copy_out_us = copy_out_start.elapsed().as_micros() as u64;

        let bytes_per_axis = (count * std::mem::size_of::<f32>()) as u64;
        let opt_present = optional_reads.iter().filter(|c| c.value.is_some()).count() as u64;
        let read_axes = 2 * reads.len() as u64;
        let write_axes = 2 * writes.len() as u64;
        let opt_axes = 2 * opt_present;
        let state_cols = state_schema.len() as u64;
        counters.batch_copy_in_bytes +=
            bytes_per_axis * (read_axes + write_axes + opt_axes + state_cols);
        counters.batch_copy_out_bytes += bytes_per_axis * (write_axes + state_cols);
        // Slice 5.4b: actual dirty bytes (post-skip), as opposed to
        // `batch_copy_out_bytes` which is the would-be writeback budget.
        counters.batch_dirty_bytes += dirty_bytes;
        counters.batch_copy_in_us += copy_in_us;
        counters.batch_run_us += run_us;
        counters.batch_copy_out_us += copy_out_us;

        // Slice 5.4a A/B toggle: drop cache to force rebuild next frame.
        if self.no_cache {
            *cache_slot = None;
        }

        Ok(run_us)
    }
}

/// Walking-skeleton single-write handle (Velocity2D → Position2D).
/// Kept for `run_velocity_to_position` / worker pool back-compat.
enum WriteHandles {
    Table { px: LuaTable, py: LuaTable },
    UserData {
        px_handle: Rc<RefCell<Vec<f32>>>,
        py_handle: Rc<RefCell<Vec<f32>>>,
    },
    Buffer {
        px_handle: LuaBuffer,
        py_handle: LuaBuffer,
    },
}

/// Slice 5.1b: per-mode handles to the `q.state.<name>` columns, kept
/// alongside [`WriteHandles`] so the host can scatter mutations back
/// into the caller's `Vec<f32>` columns after `update` returns.
enum StateHandles {
    Table(Vec<LuaTable>),
    UserData(Vec<Rc<RefCell<Vec<f32>>>>),
    Buffer(Vec<LuaBuffer>),
}

/// Slice 5.1b: build `q.state.<name>` per buffer mode and return
/// parallel handles for copy-back. Sets `q.state` on `q` even when
/// the schema is empty (so scripts can probe `q.state` defensively).
fn build_state_columns(
    lua: &Lua,
    mode: BatchLuaBufferMode,
    schema: &[BatchStateField],
    state: &[Vec<f32>],
    q: &LuaTable,
    make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
) -> Result<StateHandles, BatchError> {
    let state_tbl = lua.create_table().map_err(make_runtime_err)?;
    let handles = match mode {
        BatchLuaBufferMode::Table => {
            let mut h: Vec<LuaTable> = Vec::with_capacity(schema.len());
            for (field, col) in schema.iter().zip(state.iter()) {
                let values: Vec<f32> = col.to_vec();
                let t = lua
                    .create_sequence_from(values)
                    .map_err(make_runtime_err)?;
                h.push(t.clone());
                state_tbl
                    .set(field.name.clone(), t)
                    .map_err(make_runtime_err)?;
            }
            StateHandles::Table(h)
        }
        BatchLuaBufferMode::UserData => {
            let mut h: Vec<Rc<RefCell<Vec<f32>>>> = Vec::with_capacity(schema.len());
            for (field, col) in schema.iter().zip(state.iter()) {
                let buf = F32Buffer::from_slice(col);
                h.push(buf.0.clone());
                state_tbl
                    .set(field.name.clone(), buf)
                    .map_err(make_runtime_err)?;
            }
            StateHandles::UserData(h)
        }
        BatchLuaBufferMode::Buffer => {
            let mut h: Vec<LuaBuffer> = Vec::with_capacity(schema.len());
            for (field, col) in schema.iter().zip(state.iter()) {
                let buf = lua
                    .create_buffer(f32_slice_as_bytes(col))
                    .map_err(make_runtime_err)?;
                h.push(buf.clone());
                state_tbl
                    .set(field.name.clone(), buf)
                    .map_err(make_runtime_err)?;
            }
            StateHandles::Buffer(h)
        }
    };
    q.set("state", state_tbl).map_err(make_runtime_err)?;
    Ok(handles)
}

/// Slice 5.2: build `q.read_optional.Acceleration2D` per buffer mode.
/// Always sets `q.read_optional` on `q` (an empty table when no
/// optional column is present) so scripts can probe defensively.
/// When `accel` is `None` the `Acceleration2D` field is left absent
/// (Lua `nil`) so scripts use `if q.read_optional.Acceleration2D then`.
/// Read-only — no readback handles are returned.
fn build_read_optional_columns(
    lua: &Lua,
    mode: BatchLuaBufferMode,
    accel: Option<(&[f32], &[f32])>,
    q: &LuaTable,
    make_runtime_err: &impl Fn(mlua::Error) -> BatchError,
) -> Result<(), BatchError> {
    let opt_tbl = lua.create_table().map_err(make_runtime_err)?;
    if let Some((ax, ay)) = accel {
        let accel_tbl = lua.create_table().map_err(make_runtime_err)?;
        match mode {
            BatchLuaBufferMode::Table => {
                let x = lua
                    .create_sequence_from(ax.iter().copied())
                    .map_err(make_runtime_err)?;
                let y = lua
                    .create_sequence_from(ay.iter().copied())
                    .map_err(make_runtime_err)?;
                accel_tbl.set("x", x).map_err(make_runtime_err)?;
                accel_tbl.set("y", y).map_err(make_runtime_err)?;
            }
            BatchLuaBufferMode::UserData => {
                let x = F32Buffer::from_slice(ax);
                let y = F32Buffer::from_slice(ay);
                accel_tbl.set("x", x).map_err(make_runtime_err)?;
                accel_tbl.set("y", y).map_err(make_runtime_err)?;
            }
            BatchLuaBufferMode::Buffer => {
                let x = lua
                    .create_buffer(f32_slice_as_bytes(ax))
                    .map_err(make_runtime_err)?;
                let y = lua
                    .create_buffer(f32_slice_as_bytes(ay))
                    .map_err(make_runtime_err)?;
                accel_tbl.set("x", x).map_err(make_runtime_err)?;
                accel_tbl.set("y", y).map_err(make_runtime_err)?;
            }
        }
        opt_tbl.set("Acceleration2D", accel_tbl).map_err(make_runtime_err)?;
    }
    q.set("read_optional", opt_tbl).map_err(make_runtime_err)?;
    Ok(())
}

/// Reinterpret a `&[f32]` as `&[u8]` for handing to `lua.create_buffer`.
/// Sound: every `f32` bit pattern is a valid sequence of 4 bytes, and we
/// only read.
fn f32_slice_as_bytes(s: &[f32]) -> &[u8] {
    // SAFETY: f32 has well-defined byte representation; we expose a
    // read-only slice with byte length `s.len() * 4`.
    unsafe { std::slice::from_raw_parts(s.as_ptr() as *const u8, std::mem::size_of_val(s)) }
}

/// Slice 5.4b A/B toggle: when env `JENGINE_BATCH_DIRTY_DISABLE=1`,
/// the dirty-mask `==` compare is bypassed and writebacks always copy
/// (mimicking slice 5.4a behaviour). Used only for back-to-back
/// baseline captures. Cached on first read so the per-call cost is a
/// single relaxed atomic load — both sides of the A/B pay it equally.
fn dirty_disabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("JENGINE_BATCH_DIRTY_DISABLE")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false)
    })
}

/// Slice 5.4b: dirty-aware variant of the legacy
/// `copy_buffer_into_f32_slice` helper (now removed). Allocates a
/// `Vec<u8>` via `LuaBuffer::to_vec` (same cost floor as the old
/// helper), then `==`-compares against `out` and only performs the
/// changed the column. Returns bytes written into `out` (0 on skip,
/// `n * 4` on copy).
fn copy_buffer_into_f32_slice_dirty(buf: &LuaBuffer, out: &mut [f32]) -> u64 {
    let bytes = buf.to_vec();
    let n = (bytes.len() / 4).min(out.len());
    // SAFETY: any 4 bytes form a valid f32 bit pattern.
    let src = unsafe {
        std::slice::from_raw_parts(bytes.as_ptr() as *const f32, bytes.len() / 4)
    };
    let s = &src[..n];
    let d = &mut out[..n];
    if dirty_disabled() || d != s {
        d.copy_from_slice(s);
        std::mem::size_of_val(s) as u64
    } else {
        0
    }
}

/// `__index`/`__newindex` userdata over a contiguous `Vec<f32>`.
///
/// Indexing is **1-based** to match Lua idiom; the metamethods translate
/// to the underlying 0-based `Vec` storage. The `Rc<RefCell<…>>` wrap is
/// required because `__newindex` needs interior mutability through an
/// `&this` borrow, and because the host needs a parallel handle to read
/// the mutated buffer back into ECS columns after `update` returns.
struct F32Buffer(Rc<RefCell<Vec<f32>>>);

impl F32Buffer {
    fn from_slice(s: &[f32]) -> Self {
        Self(Rc::new(RefCell::new(s.to_vec())))
    }
}

impl UserData for F32Buffer {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_meta_method(MetaMethod::Index, |_, this, idx: i64| -> LuaResult<f32> {
            let i = (idx - 1) as usize;
            let buf = this.0.borrow();
            buf.get(i).copied().ok_or_else(|| {
                mlua::Error::RuntimeError(format!(
                    "F32Buffer index {} out of bounds (len={})",
                    idx,
                    buf.len()
                ))
            })
        });
        methods.add_meta_method(
            MetaMethod::NewIndex,
            |_, this, (idx, val): (i64, f32)| -> LuaResult<()> {
                let i = (idx - 1) as usize;
                let mut buf = this.0.borrow_mut();
                let len = buf.len();
                if i < len {
                    buf[i] = val;
                    Ok(())
                } else {
                    Err(mlua::Error::RuntimeError(format!(
                        "F32Buffer __newindex {} out of bounds (len={})",
                        idx, len
                    )))
                }
            },
        );
        methods.add_meta_method(MetaMethod::Len, |_, this, ()| -> LuaResult<i64> {
            Ok(this.0.borrow().len() as i64)
        });
    }
}

/// Pull `system.query.{read,write,read_optional,state}` and
/// `system.tick_rate` out of the script's per-script env. Returns
/// `None` if `system` is absent or has no `query` subtable.
fn parse_system_spec(env: &LuaTable) -> Option<BatchSystemSpec> {
    let system: LuaTable = env.get("system").ok()?;
    let query: LuaTable = system.get("query").ok()?;
    let read = read_string_array(&query, "read").unwrap_or_default();
    let write = read_string_array(&query, "write").unwrap_or_default();
    let read_optional = read_string_array(&query, "read_optional").unwrap_or_default();
    let tick_rate: Option<u32> = system
        .get::<f64>("tick_rate")
        .ok()
        .and_then(|n| if n.is_finite() && n > 0.0 { Some(n.round() as u32) } else { None });
    let state = parse_state_schema(&query);
    Some(BatchSystemSpec { read, write, read_optional, tick_rate, state })
}

/// Slice 5.1b: parse `system.query.state = { name = { type = "f32",
/// default = 0 }, ... }` into a sorted [`BatchStateField`] list.
/// Mirrors `parse_state_schema` in `batch.rs` for the JS path.
fn parse_state_schema(query: &LuaTable) -> Vec<BatchStateField> {
    let Ok(state_tbl) = query.get::<LuaTable>("state") else {
        return Vec::new();
    };
    let mut out: Vec<BatchStateField> = Vec::new();
    for entry in state_tbl.pairs::<String, LuaTable>() {
        let Ok((name, field)) = entry else { continue; };
        // type: only "f32" supported this slice; warn+skip others so
        // future types (i32, bool) can land without breaking older
        // scripts.
        let ty: String = field.get("type").unwrap_or_else(|_| "f32".to_string());
        if ty != "f32" {
            log::warn!(
                "[BatchScript:lua] state field '{name}' has unsupported type '{ty}' \
                 (only 'f32' is supported in Slice 5.1b); field skipped.",
            );
            continue;
        }
        let default: f32 = field
            .get::<f64>("default")
            .ok()
            .map(|n| n as f32)
            .unwrap_or(0.0);
        out.push(BatchStateField { name, default });
    }
    // Stable on-disk layout: sort alphabetically by field name.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn read_string_array(t: &LuaTable, key: &str) -> Option<Vec<String>> {
    let arr: LuaTable = t.get(key).ok()?;
    let len = arr.raw_len();
    let mut out = Vec::with_capacity(len);
    for i in 1..=(len as i64) {
        if let Ok(s) = arr.get::<String>(i) {
            out.push(s);
        }
    }
    Some(out)
}

/// Delegate to the canonical Slice 5.3 validator in `batch.rs` so the
/// JS and Lua paths enforce identical rules (known component names,
/// total cap, cross-bucket uniqueness).
fn validate_walking_skeleton_spec(path: &str, spec: &BatchSystemSpec) -> Result<(), BatchError> {
    crate::scripting::batch::__validate_batch_spec(path, spec)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_table_rt() -> BatchLuaRuntime {
        BatchLuaRuntime::new(BatchLuaBufferMode::Table).unwrap()
    }
    fn make_userdata_rt() -> BatchLuaRuntime {
        BatchLuaRuntime::new(BatchLuaBufferMode::UserData).unwrap()
    }

    const HEADER: &str =
        "system = { query = { read = {\"Velocity2D\"}, write = {\"Position2D\"} } }\n";

    #[test]
    fn table_mode_integrates_velocity() {
        let mut rt = make_table_rt();
        rt.load(
            "table.lua",
            &format!(
                "{HEADER}\
                 function update(ctx, q)\n\
                   local px, py = q.write.Position2D.x, q.write.Position2D.y\n\
                   local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y\n\
                   for i = 1, q.count do\n\
                     px[i] = px[i] + vx[i] * ctx.dt\n\
                     py[i] = py[i] + vy[i] * ctx.dt\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32, -5.0, 0.0];
        let vy = vec![0.0_f32, 1.0, 2.0];
        let mut px = vec![0.0_f32, 0.0, 0.0];
        let mut py = vec![0.0_f32, 0.0, 0.0];
        rt.run_velocity_to_position("table.lua", 0.5, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert!((px[0] - 5.0).abs() < 1e-5, "px[0]={}", px[0]);
        assert!((px[1] - -2.5).abs() < 1e-5, "px[1]={}", px[1]);
        assert!((py[2] - 1.0).abs() < 1e-5, "py[2]={}", py[2]);
    }

    #[test]
    fn userdata_mode_integrates_velocity() {
        let mut rt = make_userdata_rt();
        rt.load(
            "ud.lua",
            &format!(
                "{HEADER}\
                 function update(ctx, q)\n\
                   local px, py = q.write.Position2D.x, q.write.Position2D.y\n\
                   local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y\n\
                   for i = 1, q.count do\n\
                     px[i] = px[i] + vx[i] * ctx.dt\n\
                     py[i] = py[i] + vy[i] * ctx.dt\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32, -5.0];
        let vy = vec![1.0_f32, 2.0];
        let mut px = vec![0.0_f32, 100.0];
        let mut py = vec![0.0_f32, 0.0];
        rt.run_velocity_to_position("ud.lua", 0.5, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert!((px[0] - 5.0).abs() < 1e-5, "px[0]={}", px[0]);
        assert!((px[1] - 97.5).abs() < 1e-5, "px[1]={}", px[1]);
        assert!((py[1] - 1.0).abs() < 1e-5, "py[1]={}", py[1]);
    }

    #[test]
    fn buffer_mode_integrates_velocity() {
        let mut rt = BatchLuaRuntime::new(BatchLuaBufferMode::Buffer).unwrap();
        rt.load(
            "buf.lua",
            &format!(
                "{HEADER}\
                 function update(ctx, q)\n\
                   local px, py = q.write.Position2D.x, q.write.Position2D.y\n\
                   local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y\n\
                   local dt = ctx.dt\n\
                   for i = 0, q.count - 1 do\n\
                     local off = i * 4\n\
                     buffer.writef32(px, off, buffer.readf32(px, off) + buffer.readf32(vx, off) * dt)\n\
                     buffer.writef32(py, off, buffer.readf32(py, off) + buffer.readf32(vy, off) * dt)\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![10.0_f32, -5.0, 0.0];
        let vy = vec![0.0_f32, 1.0, 2.0];
        let mut px = vec![0.0_f32, 0.0, 0.0];
        let mut py = vec![0.0_f32, 0.0, 0.0];
        rt.run_velocity_to_position("buf.lua", 0.5, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert!((px[0] - 5.0).abs() < 1e-5, "px[0]={}", px[0]);
        assert!((px[1] - -2.5).abs() < 1e-5, "px[1]={}", px[1]);
        assert!((py[2] - 1.0).abs() < 1e-5, "py[2]={}", py[2]);
        assert!(counters.batch_copy_in_us > 0 || counters.batch_run_us > 0);
    }

    #[test]
    fn rejects_mismatched_system_spec() {
        let mut rt = make_table_rt();
        let err = rt
            .load(
                "bad.lua",
                "system = { query = { read = {\"Health\"}, write = {\"Position2D\"} } }\n\
                 function update(ctx, q) end\n",
            )
            .unwrap_err();
        match err {
            BatchError::UnknownComponent { name, bucket, .. } => {
                assert_eq!(name, "Health");
                assert_eq!(bucket, "read");
            }
            other => panic!("expected UnknownComponent, got {other:?}"),
        }
    }

    #[test]
    fn accepts_script_without_system_header() {
        let mut rt = make_table_rt();
        rt.load("legacy.lua", "function update(ctx, q) end").unwrap();
        assert_eq!(rt.loaded_count(), 1);
    }

    #[test]
    fn empty_count_is_a_noop() {
        let mut rt = make_table_rt();
        rt.load("noop.lua", "function update(ctx, q) end").unwrap();
        let mut counters = FrameCounters::default();
        let vx: Vec<f32> = vec![];
        let vy: Vec<f32> = vec![];
        let mut px: Vec<f32> = vec![];
        let mut py: Vec<f32> = vec![];
        rt.run_velocity_to_position("noop.lua", 0.016, &vx, &vy, &mut px, &mut py, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(counters.batch_copy_in_bytes, 0);
    }

    #[test]
    fn multi_script_dispatch_keeps_scripts_isolated() {
        let mut rt = make_table_rt();
        rt.load(
            "double.lua",
            &format!(
                "{HEADER}\
                 function update(ctx, q)\n\
                   for i = 1, q.count do\n\
                     q.write.Position2D.x[i] = q.write.Position2D.x[i] + q.read.Velocity2D.x[i]\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        rt.load(
            "subtract.lua",
            &format!(
                "{HEADER}\
                 function update(ctx, q)\n\
                   for i = 1, q.count do\n\
                     q.write.Position2D.x[i] = q.write.Position2D.x[i] - q.read.Velocity2D.x[i]\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        assert_eq!(rt.loaded_count(), 2);

        let mut counters = FrameCounters::default();
        let vx = vec![5.0_f32];
        let vy = vec![0.0_f32];

        let mut px_a = vec![10.0_f32];
        let mut py_a = vec![0.0_f32];
        rt.run_velocity_to_position("double.lua", 1.0, &vx, &vy, &mut px_a, &mut py_a, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(px_a, vec![15.0]);

        let mut px_b = vec![10.0_f32];
        let mut py_b = vec![0.0_f32];
        rt.run_velocity_to_position("subtract.lua", 1.0, &vx, &vy, &mut px_b, &mut py_b, &mut [], None, &mut counters)
            .unwrap();
        assert_eq!(px_b, vec![5.0]);
    }

    // ── Slice 5.1b: typed state column tests ────────────────────────

    /// Header that declares one f32 state field `t` (default 0) so the
    /// state-mirror code paths are exercised across all three buffer
    /// modes.
    const HEADER_WITH_T: &str = "system = { query = { \
        read = {\"Velocity2D\"}, \
        write = {\"Position2D\"}, \
        state = { t = { type = \"f32\", default = 0 } } \
    } }\n";

    #[test]
    fn lua_state_schema_parses_and_sorts() {
        let mut rt = make_table_rt();
        rt.load(
            "sorted.lua",
            "system = { query = { \
               read = {\"Velocity2D\"}, \
               write = {\"Position2D\"}, \
               state = { \
                 zeta = { type = \"f32\", default = 9 }, \
                 alpha = { type = \"f32\", default = 1 }, \
                 mid = { type = \"f32\", default = 5 } \
               } \
             } }\n\
             function update(ctx, q) end\n",
        )
        .unwrap();
        let schema = rt.state_schema("sorted.lua").expect("loaded");
        let names: Vec<&str> = schema.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "mid", "zeta"]);
        assert_eq!(schema[0].default, 1.0);
        assert_eq!(schema[2].default, 9.0);
    }

    #[test]
    fn lua_state_empty_when_not_declared() {
        let mut rt = make_table_rt();
        rt.load(
            "none.lua",
            &format!("{HEADER}function update(ctx, q) end\n"),
        )
        .unwrap();
        let schema = rt.state_schema("none.lua").expect("loaded");
        assert!(schema.is_empty());
    }

    fn run_state_round_trip(mode: BatchLuaBufferMode, body: &str) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        rt.load("st.lua", &format!("{HEADER_WITH_T}{body}")).unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![1.0_f32, 2.0, 3.0];
        let vy = vec![0.0_f32, 0.0, 0.0];
        let mut px = vec![0.0_f32, 0.0, 0.0];
        let mut py = vec![0.0_f32, 0.0, 0.0];
        let mut state = vec![vec![10.0_f32, 20.0, 30.0]];
        rt.run_velocity_to_position(
            "st.lua",
            1.0,
            &vx,
            &vy,
            &mut px,
            &mut py,
            &mut state,
            None,
            &mut counters,
        )
        .unwrap();
        // Body adds 1 to each state entry and copies it into px.
        assert_eq!(state[0], vec![11.0, 21.0, 31.0], "mode={mode:?}");
        assert_eq!(px, vec![11.0, 21.0, 31.0], "mode={mode:?}");
    }

    #[test]
    fn lua_table_state_round_trip() {
        run_state_round_trip(
            BatchLuaBufferMode::Table,
            "function update(ctx, q)\n\
               for i = 1, q.count do\n\
                 q.state.t[i] = q.state.t[i] + 1\n\
                 q.write.Position2D.x[i] = q.state.t[i]\n\
               end\n\
             end\n",
        );
    }

    #[test]
    fn lua_userdata_state_round_trip() {
        run_state_round_trip(
            BatchLuaBufferMode::UserData,
            "function update(ctx, q)\n\
               for i = 1, q.count do\n\
                 q.state.t[i] = q.state.t[i] + 1\n\
                 q.write.Position2D.x[i] = q.state.t[i]\n\
               end\n\
             end\n",
        );
    }

    #[test]
    fn lua_buffer_state_round_trip() {
        run_state_round_trip(
            BatchLuaBufferMode::Buffer,
            "function update(ctx, q)\n\
               for i = 0, q.count - 1 do\n\
                 local off = i * 4\n\
                 local v = buffer.readf32(q.state.t, off) + 1\n\
                 buffer.writef32(q.state.t, off, v)\n\
                 buffer.writef32(q.write.Position2D.x, off, v)\n\
               end\n\
             end\n",
        );
    }

    #[test]
    fn lua_state_persists_across_invocations() {
        let mut rt = make_table_rt();
        rt.load(
            "persist.lua",
            &format!(
                "{HEADER_WITH_T}function update(ctx, q)\n\
                   for i = 1, q.count do\n\
                     q.state.t[i] = q.state.t[i] + ctx.dt\n\
                   end\n\
                 end\n"
            ),
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![0.0_f32; 2];
        let vy = vec![0.0_f32; 2];
        let mut px = vec![0.0_f32; 2];
        let mut py = vec![0.0_f32; 2];
        let mut state = vec![vec![0.0_f32; 2]];
        for _ in 0..3 {
            rt.run_velocity_to_position(
                "persist.lua",
                0.5,
                &vx,
                &vy,
                &mut px,
                &mut py,
                &mut state,
                None,
                &mut counters,
            )
            .unwrap();
        }
        // 3 ticks * 0.5 dt = 1.5 accumulated.
        assert!((state[0][0] - 1.5).abs() < 1e-5, "state={:?}", state[0]);
        assert!((state[0][1] - 1.5).abs() < 1e-5);
    }

    // ── Slice 5.2: optional reads ─────────────────────────────────────

    /// `read_optional = {'Acceleration2D'}` parses and surfaces via
    /// `read_optional_schema`. Mirrors the JS test of the same name.
    #[test]
    fn lua_parses_read_optional_schema() {
        let mut rt = make_table_rt();
        rt.load(
            "opt.lua",
            "system = { query = { read = {\"Velocity2D\"}, write = {\"Position2D\"},\n\
              read_optional = {\"Acceleration2D\"} } }\n\
             function update(ctx, q) end\n",
        )
        .unwrap();
        let schema = rt.read_optional_schema("opt.lua").expect("loaded");
        assert_eq!(schema, &["Acceleration2D".to_string()]);
        rt.load("none.lua", "function update(ctx, q) end\n").unwrap();
        assert_eq!(rt.read_optional_schema("none.lua").unwrap().len(), 0);
        assert!(rt.read_optional_schema("missing.lua").is_none());
    }

    /// Unknown components in `read_optional` are rejected with
    /// `UnknownComponent` carrying the offending name + bucket label.
    #[test]
    fn lua_rejects_unknown_optional_component() {
        let mut rt = make_table_rt();
        let err = rt
            .load(
                "bad_opt.lua",
                "system = { query = { read = {\"Velocity2D\"}, write = {\"Position2D\"},\n\
                  read_optional = {\"Health\"} } }\n\
                 function update(ctx, q) end\n",
            )
            .unwrap_err();
        match err {
            BatchError::UnknownComponent { name, bucket, .. } => {
                assert_eq!(name, "Health");
                assert_eq!(bucket, "read_optional");
            }
            other => panic!("expected UnknownComponent, got {other:?}"),
        }
    }

    /// Helper: run the same accel-using script against all three buffer
    /// modes so the per-mode `build_read_optional_columns` paths are
    /// covered by a single assertion set.
    fn run_with_accel_for_mode(mode: BatchLuaBufferMode, body: &str) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        rt.load(
            "use_accel.lua",
            &format!(
                "system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}},\n\
                  read_optional = {{\"Acceleration2D\"}} }} }}\n{body}"
            ),
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![1.0_f32, 2.0];
        let vy = vec![0.0_f32, 0.0];
        let mut px = vec![0.0_f32, 0.0];
        let mut py = vec![0.0_f32, 0.0];
        let ax = vec![10.0_f32, 100.0];
        let ay = vec![0.0_f32, 0.0];
        rt.run_velocity_to_position(
            "use_accel.lua", 1.0, &vx, &vy, &mut px, &mut py, &mut [],
            Some((&ax, &ay)), &mut counters,
        )
        .unwrap();
        assert!((px[0] - 11.0).abs() < 1e-5, "mode={mode:?} px[0]={}", px[0]);
        assert!((px[1] - 102.0).abs() < 1e-5, "mode={mode:?} px[1]={}", px[1]);
        let bpc = (2 * std::mem::size_of::<f32>()) as u64;
        assert_eq!(counters.batch_copy_in_bytes, bpc * 6, "mode={mode:?}");
    }

    #[test]
    fn lua_table_with_accel_uses_acceleration() {
        run_with_accel_for_mode(
            BatchLuaBufferMode::Table,
            "function update(ctx, q)\n\
               local a = q.read_optional.Acceleration2D\n\
               local px = q.write.Position2D.x\n\
               local vx = q.read.Velocity2D.x\n\
               for i = 1, q.count do\n\
                 px[i] = px[i] + (vx[i] + a.x[i]) * ctx.dt\n\
               end\n\
             end\n",
        );
    }

    #[test]
    fn lua_userdata_with_accel_uses_acceleration() {
        run_with_accel_for_mode(
            BatchLuaBufferMode::UserData,
            "function update(ctx, q)\n\
               local a = q.read_optional.Acceleration2D\n\
               local px = q.write.Position2D.x\n\
               local vx = q.read.Velocity2D.x\n\
               for i = 1, q.count do\n\
                 px[i] = px[i] + (vx[i] + a.x[i]) * ctx.dt\n\
               end\n\
             end\n",
        );
    }

    #[test]
    fn lua_buffer_with_accel_uses_acceleration() {
        run_with_accel_for_mode(
            BatchLuaBufferMode::Buffer,
            "function update(ctx, q)\n\
               local a = q.read_optional.Acceleration2D\n\
               for i = 0, q.count - 1 do\n\
                 local off = i * 4\n\
                 local v = buffer.readf32(q.read.Velocity2D.x, off)\n\
                 local ax = buffer.readf32(a.x, off)\n\
                 local p = buffer.readf32(q.write.Position2D.x, off)\n\
                 buffer.writef32(q.write.Position2D.x, off, p + (v + ax) * ctx.dt)\n\
               end\n\
             end\n",
        );
    }

    /// Without `accel`, `q.read_optional.Acceleration2D` is `nil` so a
    /// guarded script falls through its else-branch. Counters reflect
    /// only 4 in-columns.
    #[test]
    fn lua_without_accel_field_is_nil() {
        let mut rt = make_table_rt();
        rt.load(
            "use_accel.lua",
            "system = { query = { read = {\"Velocity2D\"}, write = {\"Position2D\"},\n\
              read_optional = {\"Acceleration2D\"} } }\n\
             function update(ctx, q)\n\
               if q.read_optional.Acceleration2D ~= nil then error('expected nil') end\n\
               local px = q.write.Position2D.x\n\
               local vx = q.read.Velocity2D.x\n\
               for i = 1, q.count do px[i] = px[i] + vx[i] * ctx.dt end\n\
             end\n",
        )
        .unwrap();
        let mut counters = FrameCounters::default();
        let vx = vec![3.0_f32];
        let vy = vec![0.0_f32];
        let mut px = vec![0.0_f32];
        let mut py = vec![0.0_f32];
        rt.run_velocity_to_position(
            "use_accel.lua", 2.0, &vx, &vy, &mut px, &mut py, &mut [],
            None, &mut counters,
        )
        .unwrap();
        assert!((px[0] - 6.0).abs() < 1e-5, "px[0]={}", px[0]);
        let bpc = std::mem::size_of::<f32>() as u64;
        assert_eq!(counters.batch_copy_in_bytes, bpc * 4);
    }

    /// Slice 5.4a: cache must grow on demand and never shrink. Run the
    /// same script with counts [100, 5_000, 1_000, 5_000] and assert
    /// (a) results are correct each frame, (b) the cache's reported
    /// capacity is the maximum count seen so far.
    fn assert_cache_grow_never_shrink(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        let path = "grow.lua";
        let body = match mode {
            BatchLuaBufferMode::Buffer => "function update(ctx, q)\n\
               local px, py = q.write.Position2D.x, q.write.Position2D.y\n\
               local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y\n\
               for i = 0, q.count - 1 do\n\
                 local off = i * 4\n\
                 buffer.writef32(px, off, buffer.readf32(px, off) + buffer.readf32(vx, off) * ctx.dt)\n\
                 buffer.writef32(py, off, buffer.readf32(py, off) + buffer.readf32(vy, off) * ctx.dt)\n\
               end\n\
             end\n",
            _ => "function update(ctx, q)\n\
               local px, py = q.write.Position2D.x, q.write.Position2D.y\n\
               local vx, vy = q.read.Velocity2D.x, q.read.Velocity2D.y\n\
               for i = 1, q.count do\n\
                 px[i] = px[i] + vx[i] * ctx.dt\n\
                 py[i] = py[i] + vy[i] * ctx.dt\n\
               end\n\
             end\n",
        };
        let src = format!(
            "system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}} }} }}\n{body}"
        );
        rt.load(path, &src).unwrap();

        let mut counters = FrameCounters::default();
        let mut high_water = 0_usize;
        for &n in &[100_usize, 5_000, 1_000, 5_000] {
            // Velocity = (1, -1), starting position (0, 0), dt = 1 → expect (n_iter, -n_iter).
            // Use a single fixed dt and one frame per call so the math is trivial.
            let vx: Vec<f32> = vec![1.0; n];
            let vy: Vec<f32> = vec![-1.0; n];
            let mut px: Vec<f32> = vec![0.0; n];
            let mut py: Vec<f32> = vec![0.0; n];
            let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
            let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
            rt.run_batch(path, 1.0, &reads, &mut writes, &[], &mut [], &mut [], &mut counters)
                .unwrap();
            // Every entity advanced by (1, -1).
            for i in 0..n {
                assert!((px[i] - 1.0).abs() < 1e-5, "px[{i}]={}, n={n}", px[i]);
                assert!((py[i] + 1.0).abs() < 1e-5, "py[{i}]={}, n={n}", py[i]);
            }
            high_water = high_water.max(n);
            // Inspect cache capacity directly — must equal high-water mark.
            let script = rt.scripts.get(path).unwrap();
            let cap = script.cache.borrow().as_ref().unwrap().capacity;
            assert_eq!(cap, high_water, "capacity must never shrink (mode={mode:?}, n={n})");
        }
    }

    #[test]
    fn cache_grows_and_never_shrinks_table() {
        assert_cache_grow_never_shrink(BatchLuaBufferMode::Table);
    }

    #[test]
    fn cache_grows_and_never_shrinks_userdata() {
        assert_cache_grow_never_shrink(BatchLuaBufferMode::UserData);
    }

    #[test]
    fn cache_grows_and_never_shrinks_buffer() {
        assert_cache_grow_never_shrink(BatchLuaBufferMode::Buffer);
    }

    /// Slice 5.4a: optional-column presence transitions (Some→None→Some)
    /// must propagate to the script via the cached `read_optional`
    /// table, without leaking stale handles when the column goes absent.
    fn assert_optional_presence_transitions(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        let path = "opt.lua";
        let body = match mode {
            BatchLuaBufferMode::Buffer => "function update(ctx, q)\n\
               local px = q.write.Position2D.x\n\
               local vx = q.read.Velocity2D.x\n\
               local ax = nil\n\
               if q.read_optional.Acceleration2D ~= nil then\n\
                 ax = q.read_optional.Acceleration2D.x\n\
               end\n\
               for i = 0, q.count - 1 do\n\
                 local off = i * 4\n\
                 local a = (ax ~= nil) and buffer.readf32(ax, off) or 0.0\n\
                 buffer.writef32(px, off, buffer.readf32(px, off) + (buffer.readf32(vx, off) + a) * ctx.dt)\n\
               end\n\
             end\n",
            _ => "function update(ctx, q)\n\
               local px = q.write.Position2D.x\n\
               local vx = q.read.Velocity2D.x\n\
               local ax = nil\n\
               if q.read_optional.Acceleration2D ~= nil then\n\
                 ax = q.read_optional.Acceleration2D.x\n\
               end\n\
               for i = 1, q.count do\n\
                 local a = (ax ~= nil) and ax[i] or 0.0\n\
                 px[i] = px[i] + (vx[i] + a) * ctx.dt\n\
               end\n\
             end\n",
        };
        let src = format!(
            "system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}},\n                                  read_optional = {{\"Acceleration2D\"}} }} }}\n{body}"
        );
        rt.load(path, &src).unwrap();
        let mut counters = FrameCounters::default();

        // Frame 1: optional present (a = 10) → px += (v + 10) * dt
        {
            let vx = vec![1.0_f32; 4];
            let vy = vec![0.0_f32; 4];
            let ax = vec![10.0_f32; 4];
            let ay = vec![0.0_f32; 4];
            let mut px = vec![0.0_f32; 4];
            let mut py = vec![0.0_f32; 4];
            let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
            let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
            let opts = vec![BatchOptionalColumn { name: "Acceleration2D", value: Some((&ax, &ay)) }];
            rt.run_batch(path, 1.0, &reads, &mut writes, &opts, &mut [], &mut [], &mut counters)
                .unwrap();
            assert!((px[0] - 11.0).abs() < 1e-5, "px[0]={} (mode={mode:?})", px[0]);
        }

        // Frame 2: optional absent → px += v * dt
        {
            let vx = vec![1.0_f32; 4];
            let vy = vec![0.0_f32; 4];
            let mut px = vec![0.0_f32; 4];
            let mut py = vec![0.0_f32; 4];
            let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
            let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
            let opts = vec![BatchOptionalColumn { name: "Acceleration2D", value: None }];
            rt.run_batch(path, 1.0, &reads, &mut writes, &opts, &mut [], &mut [], &mut counters)
                .unwrap();
            assert!((px[0] - 1.0).abs() < 1e-5, "px[0]={} after absent (mode={mode:?})", px[0]);
        }

        // Frame 3: optional present again with a = 5 → px += (v + 5) * dt
        {
            let vx = vec![2.0_f32; 4];
            let vy = vec![0.0_f32; 4];
            let ax = vec![5.0_f32; 4];
            let ay = vec![0.0_f32; 4];
            let mut px = vec![0.0_f32; 4];
            let mut py = vec![0.0_f32; 4];
            let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
            let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
            let opts = vec![BatchOptionalColumn { name: "Acceleration2D", value: Some((&ax, &ay)) }];
            rt.run_batch(path, 1.0, &reads, &mut writes, &opts, &mut [], &mut [], &mut counters)
                .unwrap();
            assert!((px[0] - 7.0).abs() < 1e-5, "px[0]={} after re-present (mode={mode:?})", px[0]);
        }
    }

    #[test]
    fn optional_presence_transitions_table() {
        assert_optional_presence_transitions(BatchLuaBufferMode::Table);
    }

    #[test]
    fn optional_presence_transitions_userdata() {
        assert_optional_presence_transitions(BatchLuaBufferMode::UserData);
    }

    #[test]
    fn optional_presence_transitions_buffer() {
        assert_optional_presence_transitions(BatchLuaBufferMode::Buffer);
    }

    // ── Slice 5.4b: dirty-mask copy_out tests ──────────────────────

    /// A no-op script returns identical write columns; `batch_dirty_bytes`
    /// must be 0 even though `batch_copy_out_bytes` reflects the full
    /// would-be writeback.
    fn assert_noop_writes_no_dirty(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        let body = match mode {
            BatchLuaBufferMode::Buffer => "function update(ctx, q) end\n",
            _ => "function update(ctx, q) end\n",
        };
        rt.load(
            "noop.lua",
            &format!("system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}} }} }}\n{body}"),
        )
        .unwrap();

        let n = 16usize;
        let vx: Vec<f32> = (0..n).map(|i| i as f32).collect();
        let vy: Vec<f32> = vec![0.5; n];
        let mut px: Vec<f32> = (0..n).map(|i| (n - i) as f32).collect();
        let mut py: Vec<f32> = vec![-2.0; n];
        let px_before = px.clone();
        let py_before = py.clone();

        let mut counters = FrameCounters::default();
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
        rt.run_batch("noop.lua", 1.0, &reads, &mut writes, &[], &mut [], &mut [], &mut counters)
            .unwrap();

        assert_eq!(px, px_before, "noop must not mutate x (mode={mode:?})");
        assert_eq!(py, py_before, "noop must not mutate y (mode={mode:?})");
        assert!(
            counters.batch_copy_out_bytes > 0,
            "would-be writeback must be nonzero (mode={mode:?})"
        );
        assert_eq!(
            counters.batch_dirty_bytes, 0,
            "dirty bytes must be 0 when script wrote nothing (mode={mode:?})"
        );
    }

    #[test]
    fn dirty_mask_noop_writes_zero_table() {
        assert_noop_writes_no_dirty(BatchLuaBufferMode::Table);
    }
    #[test]
    fn dirty_mask_noop_writes_zero_userdata() {
        assert_noop_writes_no_dirty(BatchLuaBufferMode::UserData);
    }
    #[test]
    fn dirty_mask_noop_writes_zero_buffer() {
        assert_noop_writes_no_dirty(BatchLuaBufferMode::Buffer);
    }

    /// A script that only modifies a subset of entities reports
    /// `batch_dirty_bytes < batch_copy_out_bytes`. Per-element
    /// granularity (Table mode) gives an exact dirty count; per-column
    /// granularity (UserData / Buffer modes) reports the whole column
    /// dirty when any element changed, so the test only asserts strict
    /// inequality + correct values.
    fn assert_partial_writes_partial_dirty(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        // Touch only entity index 0 (Lua 1-indexed). Y axis is never
        // touched so it should always be skipped end-to-end.
        let body = match mode {
            BatchLuaBufferMode::Buffer => "function update(ctx, q)\n\
               buffer.writef32(q.write.Position2D.x, 0, 999.0)\n\
             end\n",
            _ => "function update(ctx, q)\n\
               q.write.Position2D.x[1] = 999.0\n\
             end\n",
        };
        rt.load(
            "partial.lua",
            &format!("system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\"}} }} }}\n{body}"),
        )
        .unwrap();

        let n = 32usize;
        let vx: Vec<f32> = vec![0.0; n];
        let vy: Vec<f32> = vec![0.0; n];
        let mut px: Vec<f32> = vec![0.0; n];
        let mut py: Vec<f32> = vec![7.0; n];
        let py_before = py.clone();

        let mut counters = FrameCounters::default();
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        let mut writes = vec![BatchWriteColumn { name: "Position2D", x: &mut px, y: &mut py }];
        rt.run_batch("partial.lua", 1.0, &reads, &mut writes, &[], &mut [], &mut [], &mut counters)
            .unwrap();

        assert_eq!(px[0], 999.0, "entity 0 must update (mode={mode:?})");
        for slot in &px[1..] {
            assert_eq!(*slot, 0.0, "untouched entities stay 0 (mode={mode:?})");
        }
        assert_eq!(py, py_before, "y axis untouched (mode={mode:?})");

        let total_out = counters.batch_copy_out_bytes;
        let dirty = counters.batch_dirty_bytes;
        assert!(dirty > 0, "at least one slot was changed (mode={mode:?})");
        assert!(
            dirty < total_out,
            "partial write must be < full writeback (mode={mode:?}, dirty={dirty}, total={total_out})"
        );

        match mode {
            // Table mode is per-element: exactly one f32 (4 bytes)
            // changed across both x and y.
            BatchLuaBufferMode::Table => assert_eq!(dirty, 4),
            // UserData / Buffer modes report the whole x axis dirty
            // (one column = n * 4 bytes); y axis still skipped.
            _ => {
                let bytes_per_axis = (n * std::mem::size_of::<f32>()) as u64;
                assert_eq!(dirty, bytes_per_axis, "x dirty, y skipped (mode={mode:?})");
            }
        }
    }

    #[test]
    fn dirty_mask_partial_writes_table() {
        assert_partial_writes_partial_dirty(BatchLuaBufferMode::Table);
    }
    #[test]
    fn dirty_mask_partial_writes_userdata() {
        assert_partial_writes_partial_dirty(BatchLuaBufferMode::UserData);
    }
    #[test]
    fn dirty_mask_partial_writes_buffer() {
        assert_partial_writes_partial_dirty(BatchLuaBufferMode::Buffer);
    }

    // ── Slice 5.4c: per-column write_dirty out-channel ──────────────

    /// A noop script must leave `write_dirty[i]` `false` for every
    /// column across all three buffer modes.
    fn assert_write_dirty_noop_all_false(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        rt.load(
            "noop.lua",
            "system = { query = { read = {\"Velocity2D\"}, write = {\"Position2D\",\"Acceleration2D\"} } }\n\
             function update(ctx, q) end\n",
        )
        .unwrap();
        let n = 8usize;
        let vx = vec![1.0_f32; n];
        let vy = vec![2.0_f32; n];
        let mut px = vec![10.0_f32; n];
        let mut py = vec![20.0_f32; n];
        let mut ax = vec![30.0_f32; n];
        let mut ay = vec![40.0_f32; n];
        let mut counters = FrameCounters::default();
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        let mut writes = vec![
            BatchWriteColumn { name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice() },
            BatchWriteColumn { name: "Acceleration2D", x: ax.as_mut_slice(), y: ay.as_mut_slice() },
        ];
        // Pre-seed with `true` to verify the runtime clears them.
        let mut dirty = [true, true];
        rt.run_batch("noop.lua", 1.0, &reads, &mut writes, &[], &mut [], &mut dirty, &mut counters)
            .unwrap();
        assert_eq!(dirty, [false, false], "noop must leave all columns clean (mode={mode:?})");
    }

    #[test]
    fn write_dirty_noop_table() {
        assert_write_dirty_noop_all_false(BatchLuaBufferMode::Table);
    }
    #[test]
    fn write_dirty_noop_userdata() {
        assert_write_dirty_noop_all_false(BatchLuaBufferMode::UserData);
    }
    #[test]
    fn write_dirty_noop_buffer() {
        assert_write_dirty_noop_all_false(BatchLuaBufferMode::Buffer);
    }

    /// A script that writes only the first column must mark only that
    /// slot dirty; the second column stays clean.
    fn assert_write_dirty_marks_only_touched(mode: BatchLuaBufferMode) {
        let mut rt = BatchLuaRuntime::new(mode).unwrap();
        let body = match mode {
            BatchLuaBufferMode::Buffer => "function update(ctx, q)\n\
                 buffer.writef32(q.write.Position2D.x, 0, 999.0)\n\
               end\n",
            _ => "function update(ctx, q)\n\
                 q.write.Position2D.x[1] = 999.0\n\
               end\n",
        };
        rt.load(
            "partial.lua",
            &format!("system = {{ query = {{ read = {{\"Velocity2D\"}}, write = {{\"Position2D\",\"Acceleration2D\"}} }} }}\n{body}"),
        )
        .unwrap();
        let n = 8usize;
        let vx = vec![0.0_f32; n];
        let vy = vec![0.0_f32; n];
        let mut px = vec![1.0_f32; n];
        let mut py = vec![2.0_f32; n];
        let mut ax = vec![3.0_f32; n];
        let mut ay = vec![4.0_f32; n];
        let mut counters = FrameCounters::default();
        let reads = vec![BatchReadColumn { name: "Velocity2D", x: &vx, y: &vy }];
        let mut writes = vec![
            BatchWriteColumn { name: "Position2D", x: px.as_mut_slice(), y: py.as_mut_slice() },
            BatchWriteColumn { name: "Acceleration2D", x: ax.as_mut_slice(), y: ay.as_mut_slice() },
        ];
        let mut dirty = [false, false];
        rt.run_batch("partial.lua", 1.0, &reads, &mut writes, &[], &mut [], &mut dirty, &mut counters)
            .unwrap();
        assert_eq!(dirty, [true, false], "only Position2D was written (mode={mode:?})");
        assert_eq!(px[0], 999.0, "Position2D x[0] updated (mode={mode:?})");
        assert_eq!(ax, vec![3.0_f32; n], "Acceleration2D untouched (mode={mode:?})");
        assert_eq!(ay, vec![4.0_f32; n], "Acceleration2D untouched (mode={mode:?})");
    }

    #[test]
    fn write_dirty_marks_only_touched_table() {
        assert_write_dirty_marks_only_touched(BatchLuaBufferMode::Table);
    }
    #[test]
    fn write_dirty_marks_only_touched_userdata() {
        assert_write_dirty_marks_only_touched(BatchLuaBufferMode::UserData);
    }
    #[test]
    fn write_dirty_marks_only_touched_buffer() {
        assert_write_dirty_marks_only_touched(BatchLuaBufferMode::Buffer);
    }
}
