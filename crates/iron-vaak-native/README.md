# iron-vaak-native

`iron-vaak-ffi` is the `unsafe`-free execution/codec core. This sibling crate is the only raw-pointer
C ABI shim. It exports the functions declared in `../iron-vaak-ffi/include/iron_vaak_v0.h`.

- Context, prepared program, runner, and diagnostic set are opaque 64-bit handles.
- Handles may be released by a .NET `SafeHandle` finalizer thread.
- Different runners can run concurrently; one runner returns `BUSY` on overlapping entry.
- Every export catches Rust panic. A runner panic produces a report and poisons only that runner.
- Variable output uses length-checked, all-or-zero copy calls.
- No managed pointer, Unity object, `lua_State`, Lua stack index, or callback is retained.

The native layer produces an unapplied Patch. Revision, capability, and object-generation checks are
the host's responsibility immediately before main-thread commit.
