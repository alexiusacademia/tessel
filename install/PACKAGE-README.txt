Tessel
======

A small, declarative, compiled language for desktop apps.
Documentation: https://tessel-lang.netlify.app

This folder contains:

  tessel (tessel.exe on Windows)       the Tessel command and IDE
  libtessel_runtime.a                  the runtime library linked into every
  (tessel_runtime.lib on Windows)      app; keep it next to `tessel`
  sdk/ (macOS)                         descriptions of the macOS libraries
                                       apps use, for tessel's built-in linker

Put this folder on your PATH, then:

  tessel                 open the Tessel IDE in the current folder
  tessel run app.tsl     compile and run a program
  tessel build app.tsl   make a standalone executable
  tessel help            all commands

On macOS, tessel has its own linker: nothing else is needed.
On Windows, building apps uses Microsoft's linker from the Visual Studio
Build Tools (C++ workload), for now.

The apps you build need nothing else to run.
