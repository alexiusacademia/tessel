//! Where the device is, the satellites it hears, and which way it points.
//!
//! These are a phone's: on Android they come from the activity's Java
//! (`ui::android`), which listens to the system and keeps the latest of
//! each; the program reads them when it wants (in a timer, usually). The
//! prelude makes values of them (`Location`, `Satellite`): here they're
//! lists of numbers.
//!
//! On a computer there's nothing to read, unless `TESSEL_SENSORS=simulated`
//! says to make something up, for building an app's screens and for tests:
//! permissions are granted when asked, the device is at `TESSEL_LOCATION`
//! (`latitude,longitude,altitude`; Greenwich without it) pointing at
//! `TESSEL_HEADING` degrees, and hears a dozen satellites that stay put.

use crate::List;

/// A `[Float]` of `values`.
fn floats(values: &[f64]) -> *mut List {
    unsafe {
        let mut list = crate::tsl_list_new(&crate::codec::FLOAT_TYPE, values.len() as i64);
        for v in values {
            crate::tsl_list_push(&mut list, v as *const f64 as *const u8);
        }
        list
    }
}

#[cfg(not(target_os = "android"))]
mod device {
    use std::sync::atomic::{AtomicBool, Ordering};

    static GRANTED: AtomicBool = AtomicBool::new(false);
    static LOCATING: AtomicBool = AtomicBool::new(false);
    static COMPASS: AtomicBool = AtomicBool::new(false);

    fn simulated() -> bool {
        std::env::var("TESSEL_SENSORS").is_ok_and(|v| v == "simulated")
    }

    fn numbers(name: &str) -> Vec<f64> {
        std::env::var(name).map(|v| v.split(',').filter_map(|n| n.trim().parse().ok()).collect()).unwrap_or_default()
    }

    pub fn permission(_kind: i32, ask: bool) -> i32 {
        if !simulated() {
            return -1;
        }
        if ask {
            GRANTED.store(true, Ordering::Relaxed);
        }
        GRANTED.load(Ordering::Relaxed) as i32
    }

    pub fn location_start(_precise: bool, _interval: f64) -> bool {
        let started = simulated() && GRANTED.load(Ordering::Relaxed);
        LOCATING.store(started, Ordering::Relaxed);
        started
    }

    pub fn location_stop() {
        LOCATING.store(false, Ordering::Relaxed);
    }

    pub fn location() -> Vec<f64> {
        if !LOCATING.load(Ordering::Relaxed) {
            return Vec::new();
        }
        let at = numbers("TESSEL_LOCATION");
        let part = |i: usize, otherwise: f64| at.get(i).copied().unwrap_or(otherwise);
        // (Latitude, longitude, altitude, accuracy, speed, course, time,
        // how far off the altitude may be, and what told: the satellites.)
        vec![part(0, 51.4779), part(1, -0.0015), part(2, 46.0), 4.0, 0.0, f64::NAN, crate::stdlib::tsl_now(), 6.0, 0.0]
    }

    pub fn satellites() -> Vec<f64> {
        if !LOCATING.load(Ordering::Relaxed) {
            return Vec::new();
        }
        // System (1 GPS, 3 GLONASS, 5 BeiDou, 6 Galileo), number, azimuth,
        // elevation, signal, used, has its orbit, has the almanac, frequency.
        const SKY: [[f64; 6]; 12] = [
            [1.0, 2.0, 35.0, 62.0, 44.0, 1.0],
            [1.0, 5.0, 120.0, 41.0, 39.5, 1.0],
            [1.0, 13.0, 201.0, 18.0, 27.0, 1.0],
            [1.0, 15.0, 310.0, 75.0, 46.5, 1.0],
            [1.0, 29.0, 262.0, 9.0, 18.0, 0.0],
            [3.0, 7.0, 68.0, 33.0, 36.0, 1.0],
            [3.0, 21.0, 154.0, 12.0, 22.5, 0.0],
            [6.0, 4.0, 15.0, 51.0, 41.0, 1.0],
            [6.0, 11.0, 232.0, 28.0, 33.5, 1.0],
            [6.0, 36.0, 341.0, 5.0, 0.0, 0.0],
            [5.0, 19.0, 95.0, 22.0, 30.0, 1.0],
            [5.0, 27.0, 285.0, 47.0, 38.0, 1.0],
        ];
        let frequency = |system: f64| match system as i32 {
            3 => 1602.0,
            5 => 1561.098,
            _ => 1575.42,
        };
        SKY.iter().flat_map(|s| [s[0], s[1], s[2], s[3], s[4], s[5], 1.0, 1.0, frequency(s[0])]).collect()
    }

    pub fn compass_start() -> bool {
        COMPASS.store(simulated(), Ordering::Relaxed);
        simulated()
    }

    pub fn compass_stop() {
        COMPASS.store(false, Ordering::Relaxed);
    }

    pub fn heading() -> Vec<f64> {
        if !COMPASS.load(Ordering::Relaxed) {
            return Vec::new();
        }
        // (From magnetic north, how far that is from true north, and how
        // sure the compass is, 0 to 3.)
        vec![numbers("TESSEL_HEADING").first().copied().unwrap_or(0.0).rem_euclid(360.0), 0.5, 3.0]
    }
}

#[cfg(target_os = "android")]
use crate::ui::android as device;

/// What the program may do of `kind` (0: know exactly where the device is;
/// 1: about where): 1 if it may, 0 if not, 2 while the user is being asked,
/// -1 where there's nobody to ask. With `ask`, the user is asked if they
/// haven't said.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_permission(kind: i64, ask: u8) -> i64 {
    device::permission(kind as i32, ask != 0) as i64
}

/// `startLocation(precise:interval:)`: starts listening for where the
/// device is and for its satellites. False without the permission, or
/// where there's nothing to listen to.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_location_start(precise: u8, interval: f64) -> u8 {
    let interval = if interval.is_finite() { interval.clamp(0.1, 3600.0) } else { 1.0 };
    device::location_start(precise != 0, interval) as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_location_stop() {
    device::location_stop();
}

/// The latest place (see `device::location` for what's where), or nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_location() -> *mut List {
    floats(&device::location())
}

/// The satellites heard, 9 numbers each.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_satellites() -> *mut List {
    floats(&device::satellites())
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_compass_start() -> u8 {
    device::compass_start() as u8
}

#[unsafe(no_mangle)]
pub extern "C" fn tsl_compass_stop() {
    device::compass_stop();
}

/// Which way the device points, or nothing.
#[unsafe(no_mangle)]
pub extern "C" fn tsl_heading() -> *mut List {
    floats(&device::heading())
}
