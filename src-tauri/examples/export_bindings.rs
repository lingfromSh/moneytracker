//! Regenerate `src/lib/bindings.ts` without launching the GUI.
//!
//!     cargo run -p moneytracker --example export_bindings

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "src/lib/bindings.ts".into());
    let builder = moneytracker_lib::build_specta();
    builder
        .export(specta_typescript::Typescript::default(), &out)
        .expect("failed to export TypeScript bindings");
    println!("wrote {out}");
}
