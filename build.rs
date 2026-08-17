// build.rs – compiles app.rc (icon + file properties) into the executable.
//
// Only meaningful on Windows; the cfg keeps `cargo check` working elsewhere.

fn main() {
    #[cfg(windows)]
    {
        println!("cargo:rerun-if-changed=app.rc");
        println!("cargo:rerun-if-changed=src/icon.ico");

        // `manifest_required` turns a missing/broken resource compiler into a
        // hard build error instead of a silently icon-less binary.
        embed_resource::compile("app.rc", embed_resource::NONE)
            .manifest_required()
            .expect("failed to compile app.rc");
    }
}
