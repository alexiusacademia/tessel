---
title: Building for Android
description: Turn a Tessel app into an Android package - what to install, tessel android doctor, building an .apk and an .aab, signing for Google Play, and how an app behaves on a phone.
---

A Tessel app can be built as an Android app from the same code: the views,
`state`, timers, `await` and the rest work as they do on a computer. This
page covers what to install, how to build and try an app, how it behaves on
a phone, and how to make the package Google Play takes, and how to read the phone's
location, satellites and compass.

Android support is new. Read [What doesn't work yet](#what-doesnt-work-yet)
before planning an app around it.

## What you need

Tessel compiles the app itself. Putting it in a package uses Android's own
tools, so they have to be installed:

- the **Android SDK**, with its build tools and a platform;
- the **Android NDK**;
- a **JDK** (Java), which signs packages and compiles the small Java part
  every Android app has. A Java *runtime* alone isn't enough.

The simplest way to get all three is to install
[Android Studio](https://developer.android.com/studio), then open its SDK
Manager and add the NDK. Without Android Studio, Google's `sdkmanager`
installs the same parts:

```sh
sdkmanager "platform-tools" "platforms;android-36" "build-tools;36.0.0" "ndk;28.2.13676358"
```

Tessel looks for the SDK in the usual place for your system, or in the
folder `ANDROID_HOME` names. To see what it found, run:

```sh
tessel android doctor
```

```text
Building Android apps (`tessel build --android`) needs:
  ok       Android SDK: /Users/ada/Library/Android/sdk
  ok       build tools: /Users/ada/Library/Android/sdk/build-tools/36.0.0
  ok       platform: Android API 36 (…/platforms/android-36/android.jar)
  ok       NDK: …/ndk/28.2.13676358/toolchains/llvm/prebuilt/darwin-x86_64/bin/clang
  ok       Java: /opt/homebrew/opt/openjdk@17/bin/java
  later    Tessel's runtime for Android: the first Android build downloads it (about 50 MB)
To run an app:
  ok       connected: emulator-5554; install with `adb install -r <app>.apk`
Everything needed to build is here.
```

Anything missing is listed with the command that installs it.

Tessel's own part for Android isn't in the package you installed, since most
people never need it. The first Android build downloads it (about 50 MB),
once for each version of Tessel.

## Building and trying an app

```sh
tessel build counter.tsl --android
```

```text
built counter.apk
```

That's an `.apk`: a package you can install on a phone or an emulator. With
one connected (`tessel android doctor` says so), install and start it with
Android's `adb`:

```sh
adb install -r counter.apk
```

The app is then in the phone's app list, under its name.

What the app prints with `print` goes to the phone's log, which you can
watch from your computer:

```sh
adb logcat -s tessel
```

The app's name, id, version and icon come from
[`tessel.toml`](/start/project/) and `icon.png`, as for other builds. Two
things to know:

- The **id** names the app on the phone and in Google Play, and can never
  change once the app is published. Set it yourself, like
  `id = "com.example.tally"`: at least two parts, each starting with a
  letter. (A `-` in it becomes `_`, which is what Android allows.)
- The **version** must go up with every update you publish. Android compares
  versions by a number, which Tessel works out from `version`
  (`1.4.2` becomes 1004002), or takes from `build` if that's a whole number.

Apps run on Android 11 and later, on phones and tablets with 64-bit ARM
processors (which is all that Google Play's current rules allow new apps to
require).

## How an app behaves on a phone

The window is the whole screen; the `width:` and `height:` of the `app` are
ignored. Beyond that:

- **Touch.** A tap is a click. Dragging inside a `Scroll` (or a list, or a
  code editor) scrolls it, and it keeps moving for a moment when you let
  go. Sliders and views with `.onDrag` are dragged as with a mouse.
- **Long press.** Holding a finger on a view opens its
  [`.contextMenu`](/ui/menus-windows/), as a right click does. Menus are
  drawn larger than on a computer, for fingers.
- **Pinch.** Two fingers moving apart or together reach a view's
  [`.onPointer`](/reference/modifiers/#onpointer) as a pinch, as on a
  trackpad.
- **The system's bars.** The app is laid out between the status bar at the
  top and the navigation bar at the bottom, and clear of a camera cutout.
- **The keyboard.** Tapping a text field brings up the on-screen keyboard,
  with its suggestions, corrections, swipe typing and emoji. It can
  correct any word in the field, not only the one being typed. The layout
  shrinks to the space above the keyboard, and a field in a `Scroll` is
  scrolled into view if the keyboard would cover it. A `secure` field is
  typed as a password, and a code editor without corrections. The
  keyboard's Enter key runs a field's `.onSubmit`.
- **Dialogs and the clipboard.** [`alert` and `confirm`](/ui/dialogs/) are
  the system's dialogs, and `copyToClipboard` and `clipboardText` use the
  phone's clipboard. If the user leaves the app while a `confirm` is open,
  it's answered `false`.
- **Back.** The Back button (or gesture) closes what's on top: a sheet or
  popover, an open menu, the keyboard. With nothing open, it leaves the
  app, which keeps running in the background.
- **Dark theme.** An app follows the phone's theme, and changes with it
  while running. `app Name(appearance: .dark)` (or `.light`) keeps one.
- **Rotation.** The app is laid out again for the new shape.
- **In the background.** The app keeps its `state` while it's in the
  background. If the system ends it to free memory, it starts fresh the
  next time, so save what matters when it changes, not when the app closes.

Relative file paths are in the app's own private folder, the only place an
Android app may write freely.

## Files the user chooses

[`openFile`, `openFiles` and `saveFile`](/ui/dialogs/) open Android's own
file chooser, and give paths as they do on a computer:

```tessel
Button("Open…") {
    if let path = openFile(title: "Open", types: ["txt"]) {
        text = readFile(path) ?? ""
    }
}
```

Android's documents aren't files with paths, though: they may be in the
cloud, or belong to another app. So Tessel makes a copy of a chosen
document in the app's cache, and the path is the copy's. Reading works as
usual. When the app writes to that path (with `writeFile`, `writeData`,
`writeImage` and the like), what's written goes to the document too, so
"Save" in an editor works as it does on a computer. This holds in later
runs of the app as well, for as long as the document is there.

A few things differ from a computer:

- The chooser has no title, and it filters by `types` only when Android
  knows them all (`txt`, `png`, `pdf`…). With an extension of your own, it
  shows every file.
- The path says nothing about where the document is. Show the file's name
  to the user, not its folder.
- The system may clear an app's cache when the phone is short of space. A
  path remembered from an earlier run can then be gone, like a file that
  was deleted: ask for the file again.
- `chooseFolder` gives no folder. An app can't walk a phone's folders by
  their paths.

Design for a narrow screen: a row of five buttons that fits a desktop window
doesn't fit a phone. `HStack`s that may be too wide can go in a `Scroll`, or
be `VStack`s.

## Where the phone is

An app can ask where the phone is, which navigation satellites it hears,
and which way it points:

```tessel
app Sky {
    state sky: [Satellite] = []
    state place = "Waiting for the satellites…"

    VStack(spacing: 8) {
        Text(place)
        Text("{sky.filter { s in s.used }.count} of {sky.count} satellites used")
        Button("Start") {
            if await requestPermission(.location) {
                startLocation()
                startCompass()
            }
        }
    }
    .onAppear {
        every(seconds: 1) {
            sky = satellites()
            if let l = location() {
                place = "{l.latitude.formatted(5)}, {l.longitude.formatted(5)}"
            }
        }
    }
}
```

[Location, satellites and the compass](/reference/stdlib/#location-satellites-and-the-compass)
has every function and field. What's particular to a phone:

- **The permission.** [`requestPermission(.location)`](/reference/stdlib/#requestpermission)
  shows Android's own question, where the user chooses between the precise
  and the approximate place, and between this once and every time. A
  package says what it may ask for: `tessel build --android` adds the
  location permissions to an app that calls `startLocation`, and to no
  other, so an app that doesn't use the location doesn't say it might.
- **Only in front.** The phone is listened to while the app is on screen.
  An app doesn't follow the phone from the background.
- **The first answer** can take from a second to a minute outdoors, and
  indoors the satellites may not be heard at all. Until then `location()`
  gives the phone's last known place, if it has one: look at its `time`.
- **The compass** needs no permission. Its `geographic` heading needs the
  location, to know how far magnetic north is from true north there.
- **On Google Play,** an app that asks for the location has to say why in
  its listing and its privacy policy.

### Trying it on a computer

A computer has no satellites to hear, so there these functions give
nothing. To build an app's screens without a phone, run it with
`TESSEL_SENSORS=simulated`: permissions are granted when asked, the device
is in one place and hears a dozen satellites that stay where they are.

```sh
TESSEL_SENSORS=simulated TESSEL_LOCATION=14.5995,120.9842,12 TESSEL_HEADING=45 tessel run sky.tsl
```

`TESSEL_LOCATION` is the latitude, longitude and altitude (Greenwich
without it), and `TESSEL_HEADING` the compass heading in degrees.

On the Android emulator, the location comes from its **Location** panel
(or `adb emu geo fix <longitude> <latitude>`). Its satellites are made up:
all at azimuth and elevation 0.

## PDFs

[`writePDF`](/reference/stdlib/#writepdf) writes PDFs on a phone as it does
on a computer, with one difference: the text is drawn as shapes, so it
looks exactly as on screen but can't be selected or searched in the PDF.
(A phone's fonts are of a kind that holds every weight in one file, which
a PDF can't carry as it is.) Emoji in color are left out.

## What doesn't work yet

- **Extra windows** (`Window`) and **printing** aren't available.
- **Menu bars and ribbons** are drawn, but they're designed for a wide
  window and a mouse. On a phone, prefer buttons and context menus.
- **Choosing a folder** (`chooseFolder`): see above.
- **The location in the background**, and other sensors and permissions
  (the camera, the microphone, notifications).
- Apps have run on the emulator and on one phone (a Pixel 10a), built
  from macOS. Building from Windows hasn't been tried.

## Publishing on Google Play

Google Play takes an **app bundle** (`.aab`), signed with a key of your own.

**1. Make a key**, once, and keep it safe: every update of the app must be
signed with the same key.

```sh
keytool -genkeypair -keystore upload.jks -alias upload -keyalg RSA -keysize 2048 -validity 10000
```

It asks for a password and a few details. (`keytool` comes with the JDK.)
Don't put the keystore in a public repository.

**2. Name the key** in `tessel.toml`:

```toml
[app]
name = "Tally"
id = "com.example.tally"
version = "1.0.0"

[android]
keystore = "upload.jks"
key = "upload"
```

`keystore` is a path relative to `tessel.toml`, and `key` is the alias given
to `keytool`.

**3. Build the bundle**, with the keystore's password in the environment
(passwords never go in `tessel.toml`):

```sh
TESSEL_ANDROID_KEYSTORE_PASSWORD='…' tessel build . --android --bundle
```

```text
built tally.aab
```

If the key has a different password from the keystore, set
`TESSEL_ANDROID_KEY_PASSWORD` too.

**4. Upload** the `.aab` in the [Google Play Console](https://play.google.com/console).
The first time, Play asks you to enrol in Play App Signing; the key you made
is then your *upload key*.

With `[android]` in `tessel.toml`, `tessel build --android` signs the `.apk`
with your key too. Without it, packages are signed with Android's debug
key: fine for trying the app, but Google Play doesn't take them.
