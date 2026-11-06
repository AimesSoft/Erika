#!/usr/bin/env python3
"""Renames what `bern package` left in target/bern/dist to the release format:
Erika-<version>-<system>-<architecture>[-headless][-setup].<extension>"""
import pathlib
import re
import sys

system, headless = sys.argv[1], sys.argv[2] == "true"
dist = pathlib.Path("target/bern/dist")
version = re.search(r'^version = "(.+)"', pathlib.Path("Bern.toml").read_text(encoding="utf-8"), re.M).group(1)
arches = {
    "arm64-v8a": "arm64", "armeabi-v7a": "armv7", "aarch64": "arm64", "arm64": "arm64",
    "x86_64": "x64", "x86-64": "x64", "amd64": "x64", "x64": "x64", "armv7": "armv7",
}
for file in sorted(dist.iterdir()):
    if not file.is_file():
        continue
    name = file.name
    extension = "tar.gz" if name.endswith(".tar.gz") else file.suffix.lstrip(".")
    found = [arches[key] for key in sorted(arches, key=len, reverse=True)
             if re.search(rf"(^|[-_.]){re.escape(key)}([-_.]|$)", name)]
    arch = found[0] if found else sys.argv[3]
    parts = ["Erika", version, system, arch]
    if headless:
        parts.append("headless")
    if "-setup" in name:
        parts.append("setup")
    target = dist / ("-".join(parts) + "." + extension)
    if target != file:
        file.rename(target)
    print(target.name)
