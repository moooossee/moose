import os
import shutil
import subprocess
import sys
from pathlib import Path


source_root = Path(sys.argv[1])
build_root = Path(sys.argv[2])
profile = sys.argv[3]
gui_enabled = sys.argv[4] == "true"
style_path = sys.argv[5]
output = Path(sys.argv[6])
target_dir = build_root / "target"
binary_dir = "release" if profile == "release" else "debug"
binary_name = "moose-gui" if gui_enabled else "moose"
environment = os.environ.copy()
environment["MOOSE_STYLE_PATH"] = style_path

command = [
    "cargo",
    "build",
    "--manifest-path",
    str(source_root / "Cargo.toml"),
    "--target-dir",
    str(target_dir),
    "--bin",
    binary_name,
    "--profile",
    profile,
]

if gui_enabled:
    command.extend(["--features", "gui"])

subprocess.run(command, check=True, env=environment)
shutil.copy2(target_dir / binary_dir / binary_name, output)
