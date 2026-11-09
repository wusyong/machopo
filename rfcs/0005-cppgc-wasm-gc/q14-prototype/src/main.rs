//! Q14: what wasmtime 49 emits today for wasm GC field accesses, per
//! collector (null, DRC, copying), to compare with what a cppgc collector
//! would need (decompressing a cppgc pointer instead of `base + index` and
//! a bounds check; a marking barrier on stores). Compiles the module below
//! for this machine and disassembles each function with `llvm-objdump`
//! (Homebrew's LLVM, or `OBJDUMP`).

use std::process::Command;

use wasmtime::{Collector, Config, Engine};

const WAT: &str = r#"
(module
  (type $s (struct (field $i (mut i32)) (field $r (mut (ref null $s))) (field $a (mut anyref))))
  (type $a (array (mut anyref)))
  ;; struct.get of an i32 field, of a known non-null struct
  (func (export "get_i32") (param (ref $s)) (result i32)
    (struct.get $s $i (local.get 0)))
  ;; struct.get of a reference field
  (func (export "get_ref") (param (ref $s)) (result (ref null $s))
    (struct.get $s $r (local.get 0)))
  ;; struct.set of a reference field (the write barrier)
  (func (export "set_ref") (param (ref $s)) (param (ref null $s))
    (struct.set $s $r (local.get 0) (local.get 1)))
  ;; array.get of an anyref element (bounds checked by length)
  (func (export "array_get") (param (ref $a)) (param i32) (result anyref)
    (array.get $a (local.get 0) (local.get 1)))
  ;; struct.new (allocation)
  (func (export "new") (result (ref $s))
    (struct.new_default $s))
)
"#;

fn main() -> wasmtime::Result<()> {
    let objdump = std::env::var("OBJDUMP")
        .unwrap_or_else(|_| "/opt/homebrew/opt/llvm/bin/llvm-objdump".into());
    for (name, collector) in [
        ("null", Collector::Null),
        ("drc", Collector::DeferredReferenceCounting),
        ("copying", Collector::Copying),
    ] {
        let mut config = Config::new();
        config
            .wasm_gc(true)
            .wasm_function_references(true)
            .collector(collector);
        let engine = Engine::new(&config)?;
        let elf = engine.precompile_module(WAT.as_bytes())?;
        let path = std::env::temp_dir().join(format!("q14-{name}.cwasm"));
        std::fs::write(&path, elf)?;
        let out = Command::new(&objdump)
            .args(["-d", "--no-show-raw-insn"])
            .arg(&path)
            .output()?;
        println!("===== {name} =====");
        print!("{}", String::from_utf8_lossy(&out.stdout));
    }
    Ok(())
}
