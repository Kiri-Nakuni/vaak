# Native runtime staging

`scripts/stage-iron-vaak-native.sh <rust-target>` places the built library below
`runtimes/<rid>/native/`. `dotnet pack` includes files present there as NuGet runtime assets.

Generated binaries are not committed. Release artifacts must be built from the matching source commit,
tested on the target runtime, hashed, and attached to the release.
