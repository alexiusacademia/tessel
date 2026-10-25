//! A window's view shows only the frames Tessel gives it (so resizing the
//! window doesn't blink): its layer isn't redrawn by AppKit, keeps a frame
//! of the size before where it is, and has the window's background under
//! it. Runs with `TESSEL_NATIVE_TEST=1 cargo test -p tessel-runtime --test layer`.

fn main() {
    if std::env::var_os("TESSEL_NATIVE_TEST").is_none() || !cfg!(target_os = "macos") {
        println!("layer: skipped (set TESSEL_NATIVE_TEST=1 on macOS)");
        return;
    }
    #[cfg(target_os = "macos")]
    {
        let said = tessel_runtime::ui::check_layer().expect("layer");
        println!("layer: {said}");
        assert!(said.starts_with("policy 0, gravity topLeft, background [1.0, 0.0, 0.0, 1.0]"), "{said}");
    }
}
