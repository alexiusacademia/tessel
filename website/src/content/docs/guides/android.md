---
title: Building for Android
description: Turn a Tessel app into an Android package - what to install, tessel android doctor, building an .apk and an .aab, signing for Google Play, and how an app behaves on a phone.
---

A Tessel app can be built as an Android app from the same code: the views,
`state`, timers, `await` and the rest work as they do on a computer. This
page covers what to install, how to build and try an app, how it behaves on
a phone, and how to make the package Google Play takes.

Android support is new. Read [What doesn't work yet](#what-doesnt-work-yet)
before planning an app around it.

## What you need

Tessel compiles the app itself. Putting it in a package uses Android's own
tools, so they have to be installed:

- the **Android SDK**, with its build tools and a platform;
- the **Android NDK**;
- a **JDK** (Java), which signs packages.

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
  [`.contextMenu`](/ui/menus-windows/), as a right click does.
- **The system's bars.** The app is laid out between the status bar at the
  top and the navigation bar at the bottom, and clear of a camera cutout.
- **The keyboard.** Tapping a text field brings up the on-screen keyboard,
  and the layout shrinks to the space above it.
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

Design for a narrow screen: a row of five buttons that fits a desktop window
doesn't fit a phone. `HStack`s that may be too wide can go in a `Scroll`, or
be `VStack`s.

## What doesn't work yet

- **Text input is basic.** Letters, digits, space and backspace from the
  on-screen keyboard work. Autocorrect, swipe typing, emoji, and letters
  with accents entered by holding a key have not been made to work.
- **Dialogs.** `openFile`, `saveFile`, `chooseFolder` and `openFiles` give
  no file, and `alert` and `confirm` show nothing (`confirm` answers
  `false`). Use a [sheet](/ui/dialogs/) of your own for questions.
- **The clipboard** only works inside the app.
- **Extra windows** (`Window`) and **printing** aren't available.
- **Menu bars and ribbons** are drawn as on a desktop, which is too small
  to use comfortably with a finger.
- **Only one finger** is followed: there's no pinch to zoom.
- Building Android apps has been tried on macOS. It's written to work on
  Windows too, but that hasn't been tested.

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
