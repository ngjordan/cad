#!/usr/bin/env python3
"""Rebrand Cad as Cad.

Idempotent: every rule is a literal (old, new) pair applied to an explicit
file list, and a rule that has already been applied is skipped rather than
failing.

    python3 xtask/rebrand_iridium.py [--check]

--check reports what would change without writing anything.
"""

import argparse
import pathlib
import shutil
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

PRODUCT = "Cad"
OLD = "Cad"
APP_ID = "ph.iridium.cad"
NEW_APP_ID = "ph.iridium.cad"
ICON_OLD = "iridium-cad"
ICON_NEW = "iridium-cad"

CHANGES = []


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    # ------------------------------------------------------------------
    # 1. the product name, in user-visible strings and identifiers
    # ------------------------------------------------------------------
    skip = {
        "docs/brand/LICENSE-brand.txt",   # read, then deleted in step 4
        "NOTICE",
        "LICENSE-MIT",
        "ATTRIBUTION.md",
    }
    text_ext = {".rs", ".toml", ".sh", ".yml", ".yaml", ".md", ".json",
                ".txt", ".in", ".html", ".js", ".ps1", ".xml", ".wxs", ".wxi"}
    for p in sorted(ROOT.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(ROOT).as_posix()
        if rel in skip or rel.startswith("docs/brand/"):
            continue
        if "target/" in rel or rel.startswith(".git/"):
            continue
        if p.suffix.lower() not in text_ext:
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        if OLD not in text:
            continue
        if args.check:
            CHANGES.append((rel, OLD, PRODUCT, text.count(OLD), "name"))
            continue
        p.write_text(text.replace(OLD, PRODUCT), encoding="utf-8")
        CHANGES.append((rel, OLD, PRODUCT, text.count(OLD), "name"))

    # ------------------------------------------------------------------
    # 2. the app id
    # ------------------------------------------------------------------
    for p in sorted(ROOT.rglob("*")):
        if not p.is_file():
            continue
        rel = p.relative_to(ROOT).as_posix()
        if "target/" in rel or rel.startswith(".git/"):
            continue
        try:
            text = p.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        if APP_ID not in text:
            continue
        if args.check:
            CHANGES.append((rel, APP_ID, NEW_APP_ID, text.count(APP_ID), "app id"))
            continue
        p.write_text(text.replace(APP_ID, NEW_APP_ID), encoding="utf-8")
        CHANGES.append((rel, APP_ID, NEW_APP_ID, text.count(APP_ID), "app id"))

    # Paths that carry the app id.
    for p in sorted(ROOT.rglob(f"*{APP_ID}*")):
        if not p.is_file():
            continue
        rel = p.relative_to(ROOT).as_posix()
        if "target/" in rel or rel.startswith(".git/"):
            continue
        new_rel = rel.replace(APP_ID, NEW_APP_ID)
        if args.check:
            CHANGES.append((rel, rel, new_rel, 1, "path"))
        else:
            dst = ROOT / new_rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            p.rename(dst)
            CHANGES.append((rel, rel, new_rel, 1, "path"))

    # ------------------------------------------------------------------
    # 3. the icon files
    # ------------------------------------------------------------------
    if not args.check:
        icon_dir = ROOT / "assets" / "app-icon"
        for old, new in (("iridium-cad-1024.png", "iridium-cad-1024.png"),
                         ("iridium-cad-macos-512.png", "iridium-cad-macos-512.png"),
                         ("iridium-cad-256.png", "iridium-cad-256.png"),
                         ("iridium-cad-64.png", "iridium-cad-64.png"),
                         ("iridium-cad.ico", "iridium-cad.ico"),
                         ("iridium-cad.icns", "iridium-cad.icns")):
            src = icon_dir / old
            if src.exists():
                src.rename(icon_dir / new)
                CHANGES.append((f"assets/app-icon/{old}", old, new, 1, "icon"))

    # ------------------------------------------------------------------
    # 4. the the upstream project marks (trademark clause: forks must remove them)
    # ------------------------------------------------------------------
    brand = ROOT / "docs" / "brand"
    if brand.exists():
        if args.check:
            CHANGES.append(("docs/brand/", "the upstream project marks", "deleted", 1, "trademark"))
        else:
            shutil.rmtree(brand)
            CHANGES.append(("docs/brand/", "the upstream project marks", "deleted", 1, "trademark"))

    # ATTRIBUTION.md rows for the deleted brand files would break
    # `cargo xtask assets`, which requires every asset to have a row and
    # every row to point at a file that exists.
    attribution = ROOT / "ATTRIBUTION.md"
    if attribution.exists() and not args.check:
        lines = attribution.read_text(encoding="utf-8").split("\n")
        kept = [l for l in lines if "docs/brand/" not in l]
        removed = len(lines) - len(kept)
        if removed:
            kept = [l for l in kept
                    if "The one exception is the the upstream project brand files in `docs/brand/`"
                    not in l]
            text = "\n".join(kept)
            text = text.replace("**Policy (mandatory):** Cad contains",
                                "**Policy (mandatory):** Cad contains")
            attribution.write_text(text, encoding="utf-8")
            CHANGES.append(("ATTRIBUTION.md", "docs/brand rows", "removed", removed,
                            "attribution"))

    # The icon files were renamed, so the ATTRIBUTION.md rows that point at
    # them must be repointed or the assets gate fails.
    attribution = ROOT / "ATTRIBUTION.md"
    if attribution.exists() and not args.check:
        text = attribution.read_text(encoding="utf-8")
        n = text.count("assets/app-icon/" + ICON_OLD)
        if n:
            text = text.replace("assets/app-icon/" + ICON_OLD,
                                "assets/app-icon/" + ICON_NEW)
            attribution.write_text(text, encoding="utf-8")
            CHANGES.append(("ATTRIBUTION.md", "app-icon/" + ICON_OLD,
                            "app-icon/" + ICON_NEW, n, "attribution"))

    # ------------------------------------------------------------------
    # 5. licence and attribution hygiene
    # ------------------------------------------------------------------
    if not args.check:
        notice = ROOT / "NOTICE"
        if notice.exists():
            t = notice.read_text(encoding="utf-8")
            t = t.replace(
                "Cad\nCopyright (c) 2026 the upstream project Team and the Cad contributors.",
                "Cad\n"
                "Based on Cad (c) 2026 the upstream project Team and the Cad contributors.\n"
                "Copyright (c) 2026 Iridium Technologies, Inc.")
            t = t.replace(
                "The the upstream project name, wordmark and logos in docs/brand/ are trademarks of the the upstream project Team. They are\n"
                "not open source and are not covered by the license above; their terms are in\n"
                "docs/brand/LICENSE-brand.txt.",
                "This is a modified distribution. The the upstream project name, wordmark and logos have been removed\n"
                "as the upstream brand licence requires; see ATTRIBUTION.md. The code remains under\n"
                "the MIT License or the Apache License, Version 2.0, at your option.")
            notice.write_text(t, encoding="utf-8")
            CHANGES.append(("NOTICE", OLD, PRODUCT, 1, "notice"))

    # ------------------------------------------------------------------
    # report
    # ------------------------------------------------------------------
    total = sum(c[3] for c in CHANGES)
    by_note = {}
    for rel, old, new, n, note in CHANGES:
        by_note.setdefault(note, [0, 0])
        by_note[note][0] += 1
        by_note[note][1] += n
    print(f"{'CHECK' if args.check else 'REBRAND'}: {len(CHANGES)} files, {total} substitutions")
    for note, (files, n) in sorted(by_note.items()):
        print(f"  {note:12s} {files:4d} files  {n:6d} substitutions")


if __name__ == "__main__":
    main()
