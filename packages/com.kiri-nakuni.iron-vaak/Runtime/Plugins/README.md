# Native plugins

Run `scripts/stage-iron-vaak-native.sh <rust-target>` to populate the platform directory used by
Unity and the matching NuGet runtime asset directory. Generated native binaries are not committed.

For a Unity release, verify each staged plugin's import settings in the Editor and run an actual
IL2CPP Player fixture. iOS uses the static library and `__Internal`; desktop and Android load
`iron_vaak_native` by name.
