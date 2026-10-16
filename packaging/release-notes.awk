# Print the CHANGELOG section for version `v` as a GitHub Release body.
# GitHub renders every newline in a release body as a line break, so the
# CHANGELOG's hard-wrapped continuation lines are joined onto their item.
# Usage: awk -v v=0.1.0 -f packaging/release-notes.awk CHANGELOG.md
function flush() { if (buf != "") print buf; buf = "" }
$0 ~ "^## \\[" v "\\]" { on = 1; next }
on && /^## \[/ { exit }
!on { next }
/^[[:space:]]*$/ { flush(); print ""; next }
buf != "" && $0 !~ /^[[:space:]]*([-*#|]|```)/ { sub(/^[[:space:]]+/, ""); buf = buf " " $0; next }
{ flush(); buf = $0 }
END { flush() }
