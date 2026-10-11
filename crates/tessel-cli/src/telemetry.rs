//! What an installation of Tessel reports: that it was installed or
//! updated, and (at most once a day) that it's in use.
//!
//! Each report is one request to the count (`stats.syncster.dev`), with the
//! installation's random id, its version, and the system: nothing about the
//! person, the computer, or what Tessel is used for. "What Tessel reports"
//! in the installation guide says so to users, and has to stay true.
//!
//! What's kept is the `stats` file in Tessel's folder, a line for each of:
//!
//! - `id`: the installation's random id
//! - `reported`: the version the count was last told of
//! - `active`: the day (counted from 1970) it was last told Tessel was used
//! - `tried`: when that was last tried, in seconds, so a computer that's
//!   offline doesn't try at every command
//! - `noticed`: `1` once the user was told of all this
//! - `off`: `1` when they turned it off (`tessel telemetry off`)
//!
//! The install scripts read and write the same file.

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};

const PING: &str = "https://stats.syncster.dev/v1/ping";
const MORE: &str = "https://tessel-lang.syncster.dev/start/installation/#what-tessel-reports";

/// The `stats` file, as its lines by name (the ones this version doesn't
/// know are kept as they are).
#[derive(Default)]
struct Stats {
    lines: Vec<(String, String)>,
}

impl Stats {
    fn parse(text: &str) -> Stats {
        let lines = text.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string()));
        Stats { lines: lines.collect() }
    }

    fn get(&self, key: &str) -> &str {
        self.lines.iter().find(|(k, _)| k == key).map_or("", |(_, v)| v)
    }

    fn set(&mut self, key: &str, value: &str) {
        match self.lines.iter_mut().find(|(k, _)| k == key) {
            Some(line) => line.1 = value.to_string(),
            None => self.lines.push((key.to_string(), value.to_string())),
        }
    }

    /// The installation's id; none if the file has none the count would take.
    fn id(&self) -> Option<&str> {
        let id = self.get("id");
        ((16..=64).contains(&id.len()) && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')).then_some(id)
    }

    fn text(&self) -> String {
        self.lines.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
    }
}

fn file() -> Option<PathBuf> {
    crate::update::home().ok().map(|home| home.join("stats"))
}

fn read() -> Stats {
    Stats::parse(&file().and_then(|f| std::fs::read_to_string(f).ok()).unwrap_or_default())
}

fn write(stats: &Stats) -> bool {
    file().is_some_and(|f| std::fs::write(f, stats.text()).is_ok())
}

fn set(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.is_empty())
}

/// Why nothing is reported from here, if nothing is: what the user asked
/// (in this run's environment, or for good), or where Tessel is running.
fn silent(stats: &Stats) -> Option<&'static str> {
    if set("TESSEL_NO_STATS").is_some() {
        return Some("TESSEL_NO_STATS is set");
    }
    if set("DO_NOT_TRACK").is_some_and(|v| v != "0") {
        return Some("DO_NOT_TRACK is set");
    }
    if stats.get("off") == "1" {
        return Some("it was turned off with `tessel telemetry off`");
    }
    // Not people using Tessel: an automated build, a test with releases
    // from somewhere else, or a `tessel` being worked on. (`TESSEL_STATS_URL`
    // says where reports go instead, for the tests of this.)
    if set("TESSEL_STATS_URL").is_none() {
        if set("CI").is_some() {
            return Some("this is an automated build (CI is set)");
        }
        if set("TESSEL_RELEASES").is_some() || cfg!(debug_assertions) {
            return Some("this isn't a released Tessel");
        }
    }
    None
}

/// A random id written like a UUID. Nothing of the computer goes into it.
fn new_id() -> String {
    use std::hash::{BuildHasher, Hasher};
    // (`RandomState` is seeded from the system's random numbers.)
    let half = || std::collections::hash_map::RandomState::new().build_hasher().finish();
    let hex = format!("{:016x}{:016x}", half(), half());
    format!("{}-{}-{}-{}-{}", &hex[..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..])
}

/// The address of a report: `kind` is `install`, `update` (from version
/// `previous`) or `active`.
fn address(base: &str, kind: &str, id: &str, version: &str, previous: &str) -> String {
    let os = if cfg!(windows) { "windows" } else { std::env::consts::OS };
    let arch = if std::env::consts::ARCH == "aarch64" { "arm64" } else { "x86_64" };
    let previous = if previous.is_empty() { String::new() } else { format!("&prev={previous}") };
    format!("{base}?app=tessel&type={kind}&id={id}&v={version}&os={os}&arch={arch}{previous}")
}

/// Sends a report. Whether the count took it. Nothing is said if it didn't.
fn send(kind: &str, id: &str, version: &str, previous: &str) -> bool {
    let base = set("TESSEL_STATS_URL").unwrap_or_else(|| PING.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(std::time::Duration::from_secs(5))).build().into();
    agent.get(&address(&base, kind, id, version, previous)).call().is_ok()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// `tessel update` changed this installation to `target` (like `v0.1.3`):
/// the count is told, as an `update` from the version it was told before
/// (or an `install` when it wasn't told of any).
pub fn installed(target: &str) {
    let mut stats = read();
    let version = target.trim_start_matches('v');
    if silent(&stats).is_some() || stats.get("reported") == version {
        return;
    }
    let id = stats.id().map(str::to_string).unwrap_or_else(new_id);
    // The id is kept before anything is sent, and the version only once the
    // count has taken it: one that didn't arrive goes with the next update.
    stats.set("id", &id);
    if !write(&stats) {
        return;
    }
    let previous = stats.get("reported").to_string();
    if send(if previous.is_empty() { "install" } else { "update" }, &id, version, &previous) {
        stats.set("reported", version);
        write(&stats);
    }
}

fn notice() {
    eprintln!(
        "note: Tessel reports that it's in use, at most once a day: its version, your system\n      \
         (macOS or Windows), and a random id for this installation. Nothing about you, your\n      \
         files or your code. `tessel telemetry off` turns it off. More: {MORE}"
    );
}

/// A command is being run: the first time in a day, the count is told this
/// installation is in use. The telling is left to a process of its own
/// (`tessel telemetry send`), so the command neither waits for it nor ends
/// it by ending.
///
/// Nothing is reported before the user has been told, once, that it would
/// be (by the install script, or here, when there's a terminal to say it in).
pub fn used() {
    let Some(file) = file() else { return };
    // (An installation with no `stats` file isn't one the scripts made, and
    // may not be one to write into: a file is only made once there's a
    // terminal to say why.)
    let mut stats = Stats::parse(&std::fs::read_to_string(&file).unwrap_or_default());
    if silent(&stats).is_some() {
        return;
    }
    let (today, now) = ((now() / 86400).to_string(), now());
    if stats.get("active") == today {
        return;
    }
    if stats.get("noticed") != "1" {
        if std::io::stderr().is_terminal() {
            stats.set("noticed", "1");
            if write(&stats) {
                notice();
            }
        }
        return;
    }
    // (Not again for an hour, if it doesn't get through.)
    if stats.get("tried").parse::<u64>().is_ok_and(|at| now.saturating_sub(at) < 3600) {
        return;
    }
    stats.set("tried", &now.to_string());
    if !write(&stats) {
        return;
    }
    if let Ok(me) = std::env::current_exe() {
        let _ = Command::new(me).args(["telemetry", "send"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
    }
}

/// `tessel telemetry [status|on|off]` (and `send`, which `used` starts).
pub fn run(args: &[String]) -> ExitCode {
    let mut stats = read();
    match args.first().map(String::as_str) {
        Some("send") => {
            if silent(&stats).is_none() && stats.get("noticed") == "1" {
                let id = stats.id().map(str::to_string).unwrap_or_else(new_id);
                if send("active", &id, tessel_compiler::VERSION, "") {
                    // (Read again: the command that started this may have written since.)
                    let mut stats = read();
                    stats.set("id", &id);
                    stats.set("active", &(now() / 86400).to_string());
                    write(&stats);
                }
            }
            ExitCode::SUCCESS
        }
        Some(turn @ ("on" | "off")) => {
            stats.set("off", if turn == "off" { "1" } else { "0" });
            // (Whoever turns it on or off knows of it.)
            stats.set("noticed", "1");
            if !write(&stats) {
                eprintln!("tessel: can't write Tessel's `stats` file");
                return ExitCode::FAILURE;
            }
            println!("Tessel's reports are {turn}.");
            ExitCode::SUCCESS
        }
        None | Some("status") => {
            match silent(&stats) {
                Some(why) => println!("Tessel reports nothing from here: {why}."),
                None => println!(
                    "Tessel reports when it's installed or updated, and at most once a day that it's in use:\n\
                     its version, the system, and this installation's random id ({}).\n\
                     `tessel telemetry off` turns it off.",
                    stats.id().unwrap_or("none yet")
                ),
            }
            println!("More: {MORE}");
            ExitCode::SUCCESS
        }
        Some(_) => {
            eprintln!("usage: tessel telemetry [status|on|off]");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stats_file_keeps_what_it_holds() {
        let id = "0123abcd-0123-abcd-0123-abcd0123abcd";
        let mut stats = Stats::parse(&format!("id={id}\nreported=0.1.40\nlater=kept\n"));
        assert_eq!((stats.id(), stats.get("reported"), stats.get("active")), (Some(id), "0.1.40", ""));
        stats.set("reported", "0.2.0");
        stats.set("off", "1");
        assert_eq!(stats.text(), format!("id={id}\nreported=0.2.0\nlater=kept\noff=1\n"));
        // An id the count wouldn't take reads as none.
        assert_eq!(Stats::parse("id=me@example.com\n").id(), None);
        assert_eq!(Stats::parse("").id(), None);
        let made = new_id();
        assert_eq!(Stats::parse(&format!("id={made}\n")).id(), Some(made.as_str()));
        assert_ne!(made, new_id());
    }

    #[test]
    fn a_report_says_what_and_no_more() {
        let id = "0123abcd-0123-abcd-0123-abcd0123abcd";
        let system = format!("os={}&arch=", if cfg!(windows) { "windows" } else { std::env::consts::OS });
        let update = address("https://s/ping", "update", id, "0.1.41", "0.1.40");
        assert!(update.starts_with(&format!("https://s/ping?app=tessel&type=update&id={id}&v=0.1.41&{system}")), "{update}");
        assert!(update.ends_with("&prev=0.1.40"), "{update}");
        let active = address("https://s/ping", "active", id, "0.2.0", "");
        assert!(active.starts_with(&format!("https://s/ping?app=tessel&type=active&id={id}&v=0.2.0&{system}")), "{active}");
        assert!(!active.contains("prev"), "{active}");
    }

    #[test]
    fn it_can_be_turned_off() {
        // (Tests are never a released Tessel, so they report nothing anyway;
        // what the user asked for comes first among the reasons.)
        assert!(silent(&Stats::default()).is_some());
        if set("TESSEL_NO_STATS").is_none() && set("DO_NOT_TRACK").is_none() {
            assert_eq!(silent(&Stats::parse("off=1\n")), Some("it was turned off with `tessel telemetry off`"));
            assert_ne!(silent(&Stats::parse("off=0\n")), Some("it was turned off with `tessel telemetry off`"));
        }
    }
}
