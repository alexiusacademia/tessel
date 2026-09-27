Tessel
======

A small, declarative, compiled language for desktop apps.
Documentation: https://tessel-lang.netlify.app

This folder contains:

  tessel (tessel.exe on Windows)       the Tessel command and IDE
  libtessel_runtime.a                  the runtime library linked into every
  (tessel_runtime.lib on Windows)      app; keep it next to `tessel`
  sdk/                                 what tessel's built-in linker needs:
                                       descriptions of the macOS libraries
                                       apps use, or on Windows the MinGW-w64
                                       C runtime and Windows import libraries

Put this folder on your PATH, then:

  tessel                 open the Tessel IDE in the current folder
  tessel run app.tsl     compile and run a program
  tessel build app.tsl   make a standalone executable
  tessel help            all commands

tessel has its own linker, so nothing else needs to be installed: no
Xcode, no Visual Studio. The apps you build need nothing else to run.

