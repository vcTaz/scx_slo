// Generates src/bpf.rs, intf.h and main.bpf.c from scx_rustland_core and
// compiles the BPF side. Don't edit the generated files: they are rewritten
// on every build (see docs/TAXONOMY.md, R-bpfgen).
fn main() {
    scx_rustland_core::RustLandBuilder::new()
        .unwrap()
        .build()
        .unwrap();
}
