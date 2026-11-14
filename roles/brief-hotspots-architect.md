- This is a hotspots issue: convert each file it lists by the role's Hot files section (central lists become
  folders, generated files are regenerated, docs and CI are split per area). Each ADR ends with the exact
  `.drover/arch.toml` lines (`frozen` and `[[collect]]`, or `generated` and `generate`) the impl lane adds in
  the PR that converts the file.
