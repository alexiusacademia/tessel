//! A web view of the system's (see `ui::check_web_view`): a page loaded into
//! it, without a window. Runs with `TESSEL_NATIVE_TEST=1 cargo test -p tessel-runtime --test web`.

fn main() {
    if std::env::var_os("TESSEL_NATIVE_TEST").is_none() || !cfg!(target_os = "macos") {
        println!("web: skipped (set TESSEL_NATIVE_TEST=1 on macOS)");
        return;
    }
    let said = tessel_runtime::ui::check_web_view("<title>Tessel page</title><p>Hello</p>").expect("web");
    println!("web: {said}");
    assert!(said.contains("\"Tessel page\""), "{said}");
    // (Laid out 10 points from the top of a 200-point view: 10 from the bottom, unflipped.)
    assert!(said.contains("frame 10 10 300 180"), "{said}");
}
