- #1478: Swift builds use your lane's own warm build folders: `xcodebuild -derivedDataPath "$DROVER_DERIVED_DATA"`
  and `swift build --scratch-path "$DROVER_SWIFT_SCRATCH"` (the same for `swift test`), never Xcode's default
  DerivedData or the package's `.build`.
