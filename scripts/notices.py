#!/usr/bin/env python3
"""Write THIRD-PARTY-NOTICES.txt: the licences and copyright notices of the fonts and Rust
crates built into Drift, which their licences ask to go with every copy.

    python scripts/notices.py           rewrite the file
    python scripts/notices.py --check   fail if it is out of date (CI runs this)

Needs cargo-about 0.9 (cargo install cargo-about --locked --features cli). It reads the
crates from Cargo.lock for every platform in about.toml, leaving out build scripts and test
tools, which aren't part of the programs. The fonts come from the epaint_default_fonts
crate: egui draws the interface with them, and Drift embeds two of them in the PDFs it
exports.
"""

import json
import os
import re
import struct
import subprocess
import sys
import textwrap

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
OUTPUT = os.path.join(ROOT, "THIRD-PARTY-NOTICES.txt")
WIDTH = 92
RULE = "=" * WIDTH

# The fonts in epaint_default_fonts, with the licence file that covers each. The crate's own
# licence field leaves out Hack's and emoji-icon-font's, so they're listed here by hand.
FONTS = [
    (
        "Ubuntu Light",
        "Ubuntu-Light.ttf",
        "Ubuntu Font Licence 1.0",
        "UFL.txt",
        "the interface's text, and the sans text in exported PDFs.",
    ),
    (
        "Hack Regular",
        "Hack-Regular.ttf",
        "MIT, and the Bitstream Vera Licence",
        "Hack-Regular.txt",
        "the interface's monospaced text, and the monospaced text in exported PDFs.",
    ),
    (
        "Noto Emoji",
        "NotoEmoji-Regular.ttf",
        "SIL Open Font License 1.1",
        "OFL.txt",
        "emoji in the interface.",
    ),
    (
        "emoji-icon-font",
        "emoji-icon-font.ttf",
        "MIT",
        "emoji-icon-font-mit-license.txt",
        "symbols in the interface.",
    ),
]

# Crates ported from other projects whose licences ask for their notices to be kept, though
# the crates don't carry them: crate name -> (project, its repository, its licence id,
# the licence file in scripts/upstream).
UPSTREAM = {
    "acadrust": (
        "ACadSharp",
        "https://github.com/DomCR/ACadSharp",
        "MIT",
        "ACadSharp-LICENSE.txt",
    ),
}

# Licence-template lines that mention copyright without stating one.
PLACEHOLDER = re.compile(
    r"\[yyyy\]|\{yyyy\}|<year>|\[year\]|\[fullname\]|\[name of copyright owner\]|<copyright holders?>",
    re.I,
)
COPYRIGHT_LINE = re.compile(r"^(Copyright|COPYRIGHT)\s*(\(c\)|\(C\)|©|\d|[A-Z])|^©")
# The files in a crate that state its copyright.
NOTICE_FILE = re.compile(r"(LICEN[CS]E|COPYING|COPYRIGHT|NOTICE)([-._].*)?$", re.I)
# The ones worth reproducing in full: some explain which licence covers what.
EXTRA_FILE = re.compile(r"(COPYRIGHT|NOTICE)(\.(txt|md))?$", re.I)


def run(*args):
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, encoding="utf-8")
    if result.returncode != 0:
        sys.exit(f"{' '.join(args)} failed:\n{result.stderr}")
    return result.stdout


def clean(text):
    """A licence text with Unix line endings, no trailing spaces and no outer blank lines."""
    lines = [line.rstrip() for line in text.replace("\r\n", "\n").replace("\r", "\n").split("\n")]
    return "\n".join(lines).strip("\n")


def copyright_lines(text):
    found = []
    for line in clean(text).split("\n"):
        line = line.strip()
        if COPYRIGHT_LINE.match(line) and not PLACEHOLDER.search(line):
            # The licence wording ("copyright notice", "copyright holders") isn't a notice.
            if re.match(r"copyright (notice|holder|owner|licen[cs]e|and|statement)", line, re.I):
                continue
            line = re.sub(r"\s+", " ", line)
            if line not in found:
                found.append(line)
    return found


def font_copyright(path):
    """The copyright string (name ID 0) from a TrueType font's name table."""
    with open(path, "rb") as f:
        data = f.read()
    num_tables = struct.unpack(">H", data[4:6])[0]
    for i in range(num_tables):
        tag, _, offset, _ = struct.unpack(">4sIII", data[12 + 16 * i : 28 + 16 * i])
        if tag != b"name":
            continue
        _, count, strings = struct.unpack(">HHH", data[offset : offset + 6])
        found = {}
        for j in range(count):
            platform, encoding, _, name_id, length, start = struct.unpack(
                ">HHHHHH", data[offset + 6 + 12 * j : offset + 18 + 12 * j]
            )
            if name_id != 0:
                continue
            raw = data[offset + strings + start : offset + strings + start + length]
            if platform == 3 or platform == 0:
                found.setdefault("unicode", raw.decode("utf-16-be"))
            elif platform == 1 and encoding == 0:
                found.setdefault("mac", raw.decode("mac_roman"))
        text = found.get("unicode") or found.get("mac")
        return re.sub(r"\s+", " ", text).strip() if text else None
    return None


def wrap(text, indent=""):
    return textwrap.fill(
        text, WIDTH, initial_indent=indent, subsequent_indent=indent, break_on_hyphens=False
    )


def heading(title):
    return f"{RULE}\n{title}\n{RULE}"


def shipped():
    """The crates built into Drift on some platform: each target's normal dependencies.

    cargo-about merges the targets' dependency graphs, so a crate that one dependency
    needs only on a platform where that dependency itself is never built would slip in."""
    with open(os.path.join(ROOT, "about.toml"), encoding="utf-8") as f:
        config = f.read()
    block = re.search(r"^targets\s*=\s*\[(.*?)\]", config, re.S | re.M)
    if not block:
        sys.exit("no targets in about.toml")
    crates = set()
    for target in re.findall(r'"([^"]+)"', block.group(1)):
        tree = run(
            "cargo", "tree", "--locked", "--offline", "--workspace", "--target", target,
            "--edges", "normal", "--prefix", "none", "--format", "{p}",
        )
        for line in tree.splitlines():
            parts = line.split()
            if len(parts) >= 2 and parts[1].startswith("v"):
                crates.add((parts[0], parts[1][1:]))
    return crates


def generate():
    run("cargo", "fetch", "--locked")
    about = json.loads(run("cargo", "about", "generate", "--format", "json", "--locked", "--offline"))
    keep = shipped()
    about["crates"] = [
        c for c in about["crates"] if (c["package"]["name"], c["package"]["version"]) in keep
    ]
    for licence in about["licenses"]:
        licence["used_by"] = [
            u for u in licence["used_by"] if (u["crate"]["name"], u["crate"]["version"]) in keep
        ]
    about["licenses"] = [licence for licence in about["licenses"] if licence["used_by"]]

    # Each crate: the licence it's used under, the texts, and its copyright notices.
    crates = {}
    for crate in about["crates"]:
        package = crate["package"]
        crates[(package["name"], package["version"])] = {
            "package": package,
            "declared": package.get("license") or "(see licence file)",
            "texts": [],
        }
    texts = {}
    for licence in about["licenses"]:
        body = clean(licence["text"])
        key = (licence["id"], body)
        entry = texts.setdefault(key, {"name": licence["name"], "id": licence["id"], "users": []})
        for user in licence["used_by"]:
            ident = (user["crate"]["name"], user["crate"]["version"])
            if ident not in entry["users"]:
                entry["users"].append(ident)
            crates[ident]["texts"].append(key)

    # The licences of the projects some crates are ported from (see UPSTREAM).
    upstream_texts = {}
    for name, (project, _, licence_id, licence_file) in UPSTREAM.items():
        users = [ident for ident in crates if ident[0] == name]
        if not users:
            continue
        with open(os.path.join(ROOT, "scripts", "upstream", licence_file), encoding="utf-8") as f:
            body = clean(f.read())
        upstream_texts[name] = (body, copyright_lines(body))
        key = (licence_id, body)
        entry = texts.setdefault(
            key, {"name": f"{project}'s licence", "id": licence_id, "users": []}
        )
        entry["users"].extend(u for u in users if u not in entry["users"])

    fonts_crate = next(
        (c["package"] for c in about["crates"] if c["package"]["name"] == "epaint_default_fonts"),
        None,
    )
    if fonts_crate is None:
        sys.exit("epaint_default_fonts isn't a dependency any more: update FONTS in this script")
    fonts_dir = os.path.join(os.path.dirname(fonts_crate["manifest_path"]), "fonts")

    out = []
    out.append(heading("Third-party notices for Drift"))
    out.append("")
    out.append(
        wrap(
            "Drift is built with the open-source fonts and Rust crates listed here. Each is used "
            "under the licence named for it, and their licences ask that these notices go with "
            "every copy of Drift. The full licence texts are in part 3. Drift's own licence is in "
            "the files next to this one."
        )
    )
    out.append("")
    out.append(
        wrap(
            "This file is made by scripts/notices.py from Cargo.lock "
            f"(epaint_default_fonts {fonts_crate['version']} for the fonts). Don't edit it by hand."
        )
    )
    out.append("")
    out.append("")

    # Part 1: fonts.
    out.append(heading("1. Fonts"))
    for name, ttf, licence, licence_file, use in FONTS:
        ttf_path = os.path.join(fonts_dir, ttf)
        licence_path = os.path.join(fonts_dir, licence_file)
        for path in (ttf_path, licence_path):
            if not os.path.exists(path):
                sys.exit(f"{path} is missing: update FONTS in this script")
        out.append("")
        out.append(f"{name} ({ttf})")
        out.append("-" * len(out[-1]))
        out.append(wrap(f"Used for: {use}"))
        out.append(wrap(f"Licence: {licence}"))
        copyright_text = font_copyright(ttf_path)
        if copyright_text:
            out.append(wrap(f"From the font: {copyright_text}"))
        out.append("")
        with open(licence_path, encoding="utf-8") as f:
            out.append(clean(f.read()))
    out.append("")
    out.append("")

    # Part 2: crates.
    out.append(heading(f"2. Rust crates ({len(crates)})"))
    out.append("")
    out.append(
        wrap(
            "Each crate with its version, the licence its authors declare, the licence Drift uses "
            "it under where they offer a choice, and its copyright notices (from its licence "
            "files, or its listed authors where those files name none)."
        )
    )
    for ident in sorted(crates, key=lambda k: (k[0].lower(), k[1])):
        crate = crates[ident]
        package = crate["package"]
        out.append("")
        out.append(f"{ident[0]} {ident[1]}")
        out.append(wrap(f"Licence: {crate['declared']}", "    "))
        used = sorted({k[0] for k in crate["texts"]})
        if used and " OR " in crate["declared"]:
            out.append(wrap(f"Used under: {' AND '.join(used)}", "    "))
        # Copyright lines from all its licence files, not just the one it's used under: a
        # crate's Apache-2.0 text rarely names anyone, while its MIT one usually does.
        notices = []
        crate_dir = os.path.dirname(package["manifest_path"])
        files = sorted(f for f in os.listdir(crate_dir) if NOTICE_FILE.match(f))
        bodies = [key[1] for key in crate["texts"]]
        for name in files:
            path = os.path.join(crate_dir, name)
            if os.path.isfile(path):
                with open(path, encoding="utf-8", errors="replace") as f:
                    bodies.append(f.read())
        for body in bodies:
            for line in copyright_lines(body):
                if line not in notices:
                    notices.append(line)
        if notices:
            for line in notices:
                out.append(wrap(line, "    "))
        elif package.get("authors"):
            out.append(wrap(f"Copyright the authors: {', '.join(package['authors'])}", "    "))
        if package.get("repository"):
            out.append(f"    {package['repository']}")
        if ident[0] in UPSTREAM:
            project, url, licence_id, _ = UPSTREAM[ident[0]]
            out.append(wrap(f"Ported from {project} ({licence_id}), whose notice is kept:", "    "))
            for line in upstream_texts[ident[0]][1]:
                out.append(wrap(line, "      "))
            out.append(f"      {url}")
        # A crate's COPYRIGHT or NOTICE file, in full: some explain which licence covers what.
        for extra in files:
            if EXTRA_FILE.match(extra):
                with open(os.path.join(crate_dir, extra), encoding="utf-8", errors="replace") as f:
                    body = clean(f.read())
                out.append(f"    {extra}:")
                out.append(textwrap.indent(body, "      "))
    out.append("")
    out.append("")

    # Part 3: the licence texts, once each, with the crates they cover.
    out.append(heading("3. Licence texts"))
    order = sorted(texts.items(), key=lambda kv: (kv[1]["id"], sorted(kv[1]["users"])))
    for number, ((_, body), entry) in enumerate(order, 1):
        users = ", ".join(f"{n} {v}" for n, v in sorted(entry["users"]))
        out.append("")
        title = f"3.{number} {entry['name']} ({entry['id']})"
        out.append(title)
        out.append("-" * len(title))
        out.append(wrap(f"Used by: {users}"))
        out.append("")
        out.append(body)
    out.append("")
    return "\n".join(out)


def main():
    check = "--check" in sys.argv[1:]
    text = generate()
    if check:
        try:
            with open(OUTPUT, encoding="utf-8", newline="") as f:
                current = f.read().replace("\r\n", "\n")
        except FileNotFoundError:
            current = None
        if current != text:
            sys.exit(
                "THIRD-PARTY-NOTICES.txt is out of date with Cargo.lock: run "
                "`python scripts/notices.py` and commit the result."
            )
        print("THIRD-PARTY-NOTICES.txt is up to date")
        return
    with open(OUTPUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {OUTPUT}")


if __name__ == "__main__":
    main()
