Tessel
======

A small, declarative, compiled language for desktop apps.
Documentation: https://tessel-lang.netlify.app

This folder contains:

  tessel (tessel.exe on Windows)       the Tessel command and IDE
  libtessel_runtime.a                  the runtime library linked into every
  (tessel_runtime.lib on Windows)      app; keep it next to `tessel`

Put this folder on your PATH, then:

  tessel                 open the Tessel IDE in the current folder
  tessel run app.tsl     compile and run a program
  tessel build app.tsl   make a standalone executable
  tessel help            all commands

Building apps uses your system's linker:
  macOS:   Apple's Command Line Tools (xcode-select --install)
  Windows: Visual Studio Build Tools with the C++ workload

The apps you build need nothing else to run.
