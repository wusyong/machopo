//! wasm GC objects on cppgc: our rusty_v8 fork's `WasmGcObject`, a C++ GC
//! object with a type index, a length and its fields, which C++ traces by
//! the type table below (as wasmtime's `GcTypeLayouts` would lay it out).
//!
//! Reference fields are 32-bit cppgc `Member`s: `Ref` to a wasm object,
//! `HostRef` to a Rust GC object (a DOM node), `AnyRef` to a wasm object or
//! an i31 (`value << 1 | 1`; wasm objects are 16-aligned, so a compressed
//! pointer to one, `address >> 3`, has its low bit clear).
//!
//! Every object also carries an `id`, for the checks only: in a field for
//! structs, as the i31 in element 0 for arrays.

use std::ffi::{CStr, c_void};
use std::ptr::NonNull;
use std::sync::Mutex;

use v8::cppgc::wasm_gc::{
    self, AnyRef, RefField, RefKind, WasmGcObject, WasmGcType, WasmMemberSlot, WeakObjectTableSlot,
};
use v8::cppgc::{
    AllocationHandle, GarbageCollected, GetRustObj, UnsafePtr, Visitor, make_garbage_collected_on,
};

/// Every finalized node's id (nodes are Rust objects; wasm objects have no
/// finalizer).
pub static FINALIZED: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// `(struct (field $id i32) (field $a (ref null $any)) (field $b anyref)
/// (field $n externref))`
pub const S: u32 = 0;
/// `(array (mut anyref))`, element 0 holding the id.
pub const A: u32 = 1;
/// `(struct (field $x f64) (field $v i32) (field $id i32))`: no references.
pub const P: u32 = 2;

static S_REFS: [RefField; 3] = [
    RefField {
        offset: 4,
        kind: RefKind::Ref,
    },
    RefField {
        offset: 8,
        kind: RefKind::AnyRef,
    },
    RefField {
        offset: 12,
        kind: RefKind::HostRef,
    },
];

static TYPES: [WasmGcType; 3] = [
    WasmGcType {
        refs: S_REFS.as_ptr(),
        ref_count: S_REFS.len(),
        elem_size: 0,
        elem_kind: RefKind::NoRef,
    },
    WasmGcType {
        refs: std::ptr::null(),
        ref_count: 0,
        elem_size: 4,
        elem_kind: RefKind::AnyRef,
    },
    WasmGcType {
        refs: std::ptr::null(),
        ref_count: 0,
        elem_size: 0,
        elem_kind: RefKind::NoRef,
    },
];

pub fn init() {
    // SAFETY: before any object; the offsets are within each type's fields.
    unsafe { wasm_gc::set_types(&TYPES) }
}

pub enum Any {
    Null,
    I31(i32),
    Obj(Obj),
}

/// A wasm object. A plain pointer: whoever holds one knows it is alive.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Obj(NonNull<WasmGcObject>);

impl Obj {
    /// `struct.new_default` / `array.new_default`, with the id written.
    ///
    /// # Safety
    ///
    /// As for any new GC object: the pointer is stored in a reference or a
    /// root before the next GC can finish.
    pub unsafe fn new(handle: &AllocationHandle, ty: u32, len: u32, id: u32) -> Obj {
        let bytes = match ty {
            A => 4 * len as usize,
            _ => 16,
        };
        // SAFETY: per the caller; the fields fit the type.
        let obj = Obj(unsafe { WasmGcObject::new(handle, ty, len, bytes) });
        match ty {
            S => obj.set_i32(0, id as i32),
            P => obj.set_i32(12, id as i32),
            // SAFETY: just made, so alive.
            _ => unsafe { obj.set_any(0, Any::I31(id as i32)) },
        }
        obj
    }

    pub fn as_ptr(self) -> *const c_void {
        self.0.as_ptr().cast()
    }

    pub fn ty(self) -> u32 {
        // SAFETY: the holder knows it is alive.
        unsafe { WasmGcObject::ty(self.0) }
    }

    pub fn len(self) -> u32 {
        // SAFETY: as above.
        unsafe { WasmGcObject::length(self.0) }
    }

    pub fn id(self) -> u32 {
        match self.ty() {
            S => self.i32(0) as u32,
            P => self.i32(12) as u32,
            // SAFETY: as above.
            _ => match unsafe { self.get_any(0) } {
                Any::I31(id) => id as u32,
                _ => panic!("array without its id"),
            },
        }
    }

    fn field(self, offset: u32) -> *mut u8 {
        WasmGcObject::fields(self.0).wrapping_add(offset as usize)
    }

    // Plain fields: only the heap's thread touches them (markers don't).

    pub fn i32(self, offset: u32) -> i32 {
        // SAFETY: an aligned i32 field of a live object.
        unsafe { (self.field(offset) as *const i32).read() }
    }

    pub fn set_i32(self, offset: u32, value: i32) {
        // SAFETY: as above.
        unsafe { (self.field(offset) as *mut i32).write(value) }
    }

    pub fn f64(self, offset: u32) -> f64 {
        // SAFETY: an 8-aligned f64 field (the fields start 8-aligned).
        unsafe { (self.field(offset) as *const f64).read() }
    }

    pub fn set_f64(self, offset: u32, value: f64) {
        // SAFETY: as above.
        unsafe { (self.field(offset) as *mut f64).write(value) }
    }

    /// A `Ref` field := a wasm object, or null.
    ///
    /// # Safety
    ///
    /// On the heap's thread; `target` alive on the same heap.
    pub unsafe fn set_ref(self, offset: u32, target: Option<Obj>) {
        // SAFETY: per the caller.
        unsafe { wasm_gc::set_ref(self.field(offset), target.map(|o| o.0)) }
    }

    /// # Safety
    ///
    /// On the heap's thread, no GC before the result is used.
    pub unsafe fn get_ref(self, offset: u32) -> Option<Obj> {
        // SAFETY: per the caller.
        unsafe { wasm_gc::get_ref(self.field(offset)) }.map(Obj)
    }

    /// A `HostRef` field := a node, or null.
    ///
    /// # Safety
    ///
    /// As `set_ref`.
    pub unsafe fn set_node(self, offset: u32, target: Option<&UnsafePtr<Node>>) {
        // SAFETY: per the caller.
        unsafe { wasm_gc::set_host_ref(self.field(offset), target) }
    }

    /// The node a `HostRef` field points to, as a raw pointer to compare
    /// with `node_ptr`.
    ///
    /// # Safety
    ///
    /// As `get_ref`.
    pub unsafe fn get_node(self, offset: u32) -> *const c_void {
        // SAFETY: per the caller.
        unsafe { wasm_gc::get_host_ref(self.field(offset)) as *const c_void }
    }

    /// An `AnyRef` field (or array element) := a wasm object, an i31 or
    /// null.
    ///
    /// # Safety
    ///
    /// As `set_ref`.
    pub unsafe fn set_any(self, offset: u32, value: Any) {
        let slot = self.field(offset);
        // SAFETY: per the caller.
        unsafe {
            match value {
                Any::Null => wasm_gc::set_ref(slot, None),
                Any::I31(i) => wasm_gc::set_i31(slot, i),
                Any::Obj(obj) => wasm_gc::set_ref(slot, Some(obj.0)),
            }
        }
    }

    /// # Safety
    ///
    /// As `get_ref`.
    pub unsafe fn get_any(self, offset: u32) -> Any {
        // SAFETY: per the caller.
        match unsafe { wasm_gc::get_any(self.field(offset)) } {
            AnyRef::Null => Any::Null,
            AnyRef::I31(i) => Any::I31(i),
            AnyRef::Object(obj) => Any::Obj(Obj(obj)),
        }
    }

    pub fn raw(self) -> NonNull<WasmGcObject> {
        self.0
    }
}

/// A node's address, as `Obj::get_node` returns it.
pub fn node_ptr(node: &UnsafePtr<Node>) -> *const c_void {
    node.get_rust_obj() as *const c_void
}

/// A DOM node's stand-in, a Rust GC object as `dom`'s nodes are; its
/// `wasm` slot is, say, a listener's closure, so wasm objects and nodes
/// form cycles.
pub struct Node {
    id: u32,
    pub wasm: WasmMemberSlot,
}

impl Drop for Node {
    fn drop(&mut self) {
        FINALIZED.lock().unwrap().push(self.id);
    }
}

impl Node {
    pub fn id(&self) -> u32 {
        self.id
    }
}

// SAFETY: `trace` visits the one slot.
unsafe impl GarbageCollected for Node {
    fn trace(&self, visitor: &mut Visitor) {
        // SAFETY: a field of this object, from its trace.
        unsafe { self.wasm.trace(visitor) }
    }

    fn get_name(&self) -> &'static CStr {
        c"Node"
    }
}

/// # Safety
///
/// As `Obj::new`'s.
pub unsafe fn new_node(handle: &AllocationHandle, id: u32) -> UnsafePtr<Node> {
    // SAFETY: per the caller.
    unsafe {
        make_garbage_collected_on(
            handle,
            Node {
                id,
                wasm: WasmMemberSlot::empty(),
            },
        )
    }
}

/// The roots: a Rust GC object, held by one `Persistent`, with a fixed
/// number of slots (a slot never moves while a marker may read it), and a
/// weak table.
pub struct Roots {
    pub slots: [WasmMemberSlot; 32],
    pub table: WeakObjectTableSlot,
}

// SAFETY: `trace` visits every slot.
unsafe impl GarbageCollected for Roots {
    fn trace(&self, visitor: &mut Visitor) {
        // SAFETY (all): fields of this object, from its trace.
        for slot in &self.slots {
            unsafe { slot.trace(visitor) }
        }
        unsafe { self.table.trace(visitor) }
    }

    fn get_name(&self) -> &'static CStr {
        c"Roots"
    }
}

/// # Safety
///
/// As `Obj::new`'s.
pub unsafe fn new_roots(handle: &AllocationHandle) -> UnsafePtr<Roots> {
    // SAFETY: per the caller.
    unsafe {
        make_garbage_collected_on(
            handle,
            Roots {
                slots: std::array::from_fn(|_| WasmMemberSlot::empty()),
                table: WeakObjectTableSlot::empty(),
            },
        )
    }
}
