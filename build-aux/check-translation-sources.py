from pathlib import Path
import re
import sys


root = Path(__file__).resolve().parent.parent
catalog_sources = root / "po" / "POTFILES.in"
listed = {
    line.strip()
    for line in catalog_sources.read_text().splitlines()
    if line.strip() and not line.lstrip().startswith("#")
}
translation_call = re.compile(
    r"\b(?:gettext|gettext_f|gettext_noop|ngettext_f|pgettext|npgettext)\s*\("
)
required = {
    path.relative_to(root).as_posix()
    for path in (root / "src").rglob("*.rs")
    if translation_call.search(path.read_text())
}
required.update(
    {
        "data/io.github.moooossee.Moose.desktop",
        "data/io.github.moooossee.Moose.metainfo.xml.in",
        "data/io.github.moooossee.Moose.gschema.xml",
    }
)
missing = sorted(required - listed)
unavailable = sorted(path for path in listed if not (root / path).is_file())
if missing:
    print("Missing from po/POTFILES.in:", *missing, sep="\n", file=sys.stderr)
if unavailable:
    print("Missing translation source files:", *unavailable, sep="\n", file=sys.stderr)
if missing or unavailable:
    sys.exit(1)
print(f"Translation extraction covers all {len(required)} marked source files.")
