//! Video with the system's player (see `ui::check_video`): a 2-second clip,
//! played muted. Runs with `TESSEL_NATIVE_TEST=1 cargo test -p tessel-runtime --test media`.

fn main() {
    if std::env::var_os("TESSEL_NATIVE_TEST").is_none() {
        println!("media: skipped (set TESSEL_NATIVE_TEST=1)");
        return;
    }
    let clip = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/media/clip.mp4");
    match tessel_runtime::ui::check_video(clip) {
        Ok(said) => {
            println!("media: {said}");
            assert!(said.contains("picture 320x180"), "{said}");
            assert!(said.contains("2.0 s long"), "{said}");
            assert!(said.contains("then at 1.5 s"), "{said}");
        }
        Err(e) => panic!("media: {e}"),
    }
}
