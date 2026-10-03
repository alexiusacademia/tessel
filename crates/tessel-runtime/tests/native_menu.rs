//! The macOS menu bar, built with AppKit (see `ui::check_menu_bar`). Runs
//! with `TESSEL_NATIVE_TEST=1 cargo test -p tessel-runtime --test native_menu`.

fn main() {
    #[cfg(target_os = "macos")]
    if std::env::var_os("TESSEL_NATIVE_TEST").is_some() {
        tessel_runtime::ui::check_menu_bar();
        println!("native menu bar: ok");
        return;
    }
    println!("native menu bar: skipped (set TESSEL_NATIVE_TEST=1 on macOS)");
}
