//! Checks the V8 behaviour and the `v8` fork's changes that this crate's
//! safety argument (`crates/bindings/soundness.md`) relies on: an upgrade
//! or rebase that changes them fails the build instead of quietly
//! invalidating the argument.
//! The slot type itself (`v8::cppgc::TracedSlot`) lives in our fork, so a
//! changed signature there is a compile error, not something to check here.
//!
//! This finds the `v8` crate's sources through the workspace's path
//! dependency (our fork), or the cargo registry. Set `MACHOPO_V8_CRATE_DIR` to
//! override.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=MACHOPO_V8_CRATE_DIR");
    let v8_crate = v8_crate_dir();
    check(&v8_crate);
}

fn check(v8_crate: &Path) {
    let expect = |file: &str, snippets: &[&str]| {
        let path = v8_crate.join(file);
        println!("cargo:rerun-if-changed={}", path.display());
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("reading {}: {e}", path.display()));
        let text = normalize(&text);
        for snippet in snippets {
            assert!(
                text.contains(&normalize(snippet)),
                "{} no longer contains `{snippet}`; review crates/bindings/soundness.md against it",
                path.display()
            );
        }
    };

    // Our fork writes a new object's Rust data in `RustObj`'s constructor,
    // which cppgc runs before its "fully constructed" release store. That is
    // why a new object needs no fences before a marker reads it (soundness.md,
    // "Concurrent marking"); a rebase that loses it would bring the data race
    // back silently.
    expect(
        "src/support.h",
        &["RustObj(RustObjInit init, void* data) { init(this, data); }"],
    );
    expect(
        "src/binding.cc",
        &[
            "return cppgc::MakeGarbageCollected<RustObj>( heap->GetAllocationHandle(), cppgc::AdditionalBytes(size), init, data);",
        ],
    );
    expect("src/cppgc.rs", &["init_rust_obj::<T>,"]);
    expect(
        "v8/include/cppgc/allocation.h",
        &[
            "T* object = ::new (memory) T(std::forward<Args>(args)...); MakeGarbageCollectedTraitBase<T>::MarkObjectAsFullyConstructed(object);",
        ],
    );

    // `TracedSlot` reads the C++ object as one atomic pointer.
    expect(
        "v8/include/v8-handle-base.h",
        &["internal::Address* location_ = nullptr;"],
    );
    expect(
        "v8/include/v8-traced-handle.h",
        &[
            "void SetSlotThreadSafe(internal::Address* new_val) { reinterpret_cast<std::atomic<internal::Address*>*>(&slot())->store( new_val, std::memory_order_relaxed); }",
            "const internal::Address* GetSlotThreadSafe() const { return reinterpret_cast<const std::atomic<internal::Address*>*>(&slot()) ->load(std::memory_order_relaxed); }",
            // `Reset(isolate, local)` is an assigning store: it marks the
            // value if marking is running.
            "this->SetSlotThreadSafe(this->NewFromNonEmptyValue( isolate, *other, &this->slot(), internal::TracedReferenceStoreMode::kAssigningStore, internal::TracedReferenceHandling::kDefault));",
        ],
    );
    // `v8::Object::wrap` tags: the embedder's start at 1 and V8's own (for
    // the objects V8 itself wraps) at 0x6000. This crate's tags (the elise
    // heap's objects', `check_wrap_tags`'s probe) stay between, so with tag
    // checks (`check_wrap_tags` in `src/script_state.rs`) nothing V8 wraps
    // unwraps as a DOM object.
    expect(
        "v8/include/v8-sandbox.h",
        &[
            "kFirstObjectWrappableTag = 1,",
            "kFirstV8InternalTag = 0x6000,",
        ],
    );
    // How V8 makes `TracedSlot` safe to trace concurrently, and to destroy
    // from a finalizer.
    expect(
        "v8/src/handles/traced-handles-inl.h",
        &[
            "reinterpret_cast<std::atomic<Address>*>(&object_)->store( object.ptr(), std::memory_order_release);",
            "const bool needs_black_allocation = is_marking_ && store_mode != TracedReferenceStoreMode::kInitializingStore;",
            "if (needs_black_allocation) { WriteBarrier::MarkingFromTracedHandle(object); }",
        ],
    );
    expect(
        "v8/src/handles/traced-handles.cc",
        &[
            "if (is_sweeping_on_mutator_thread_) { return; }",
            "node.set_raw_object<AccessMode::ATOMIC>(kNullAddress); return;",
            "Tagged<Object>(reinterpret_cast<std::atomic<Address>*>(location)->load( std::memory_order_acquire));",
        ],
    );
    expect(
        "v8/include/v8-cppgc.h",
        &[
            "static void Trace(Visitor* visitor, const void* self) { static_cast<v8::JSVisitor*>(visitor)->Trace( *static_cast<const v8::TracedReference<T>*>(self)); }",
        ],
    );
    // `v8::Object::wrap` has a marking barrier: wrapping while marking is
    // running, after the wrapper was marked, still marks the wrapped object
    // (soundness.md, I14).
    expect(
        "v8/src/objects/cpp-heap-object-wrapper-inl.h",
        &[
            "WriteBarrier::ForCppHeapPointer( object_, object_->RawCppHeapPointerField(offset_), instance);",
        ],
    );
}

/// Collapses every run of whitespace to one space.
fn normalize(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The `v8` crate's directory when the workspace depends on it by path
/// (our rusty_v8 fork, `../third_party/rusty_v8`): the `path = "..."` of
/// the `v8 = ...` line in the workspace's `Cargo.toml`.
fn v8_path_dependency() -> Option<PathBuf> {
    let workspace = Path::new(&env::var("CARGO_MANIFEST_DIR").ok()?).join("../..");
    let manifest = workspace.join("Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with("v8 ="))?;
    let path = line.split("path = \"").nth(1)?.split('"').next()?;
    Some(workspace.join(path))
}

fn v8_crate_dir() -> PathBuf {
    if let Some(dir) = env::var_os("MACHOPO_V8_CRATE_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = v8_path_dependency() {
        return dir;
    }
    let version = v8_version();
    let cargo_home = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("USERPROFILE")
                .or_else(|| env::var_os("HOME"))
                .map(|h| Path::new(&h).join(".cargo"))
        })
        .expect("can't locate CARGO_HOME; set MACHOPO_V8_CRATE_DIR");
    let registry = cargo_home.join("registry").join("src");
    let entries = std::fs::read_dir(&registry).unwrap_or_else(|e| {
        panic!(
            "reading {}: {e}; set MACHOPO_V8_CRATE_DIR",
            registry.display()
        )
    });
    for entry in entries.flatten() {
        let candidate = entry.path().join(format!("v8-{version}"));
        if candidate
            .join("v8")
            .join("include")
            .join("v8-cppgc.h")
            .exists()
        {
            return candidate;
        }
    }
    panic!(
        "v8-{version} sources not found under {}; set MACHOPO_V8_CRATE_DIR",
        registry.display()
    );
}

/// The exact `v8` version pinned in the workspace manifest (`v8 = "=X"`).
fn v8_version() -> String {
    let manifest = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../Cargo.toml");
    println!("cargo:rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).expect("reading workspace Cargo.toml");
    text.lines()
        .find_map(|line| {
            line.trim()
                .strip_prefix("v8 = \"=")?
                .strip_suffix('"')
                .map(str::to_owned)
        })
        .expect("workspace Cargo.toml pins `v8 = \"=X.Y.Z\"`")
}
