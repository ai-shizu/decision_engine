#!/usr/bin/env python3
"""Post-process generated apple tree: FORCE_COLOR sentinel + Info.plist key merge."""
from __future__ import annotations

import plistlib
import re
import sys
from pathlib import Path

SENTINEL = "__PKB_FORCE_COLOR_ARG__"
LITERAL = "${FORCE_COLOR}"
REQUIRED_KEYS = (
    "NSFaceIDUsageDescription",
    "NSCalendarsFullAccessUsageDescription",
    "ITSAppUsesNonExemptEncryption",
)


def fail(msg: str) -> None:
    print(f"ios_postprocess_gen_apple: RED: {msg}", file=sys.stderr)
    raise SystemExit(1)


def restore_force_color(text: str) -> str:
    if SENTINEL in text:
        text = text.replace(SENTINEL, LITERAL)
    # Also repair any ambient-expanded bare positional 0/1 left of ${ARCHS
    text = re.sub(
        r"(--configuration \$\{CONFIGURATION:\?\}) (0|1) (\$\{ARCHS)",
        r"\1 ${FORCE_COLOR} \3",
        text,
    )
    return text


def assert_force_color(path: Path, text: str) -> None:
    if SENTINEL in text:
        fail(f"sentinel remains in {path}")
    if re.search(r"--configuration \$\{CONFIGURATION:\?\} (0|1) \$\{ARCHS", text):
        fail(f"bare FORCE_COLOR positional 0/1 in {path}")
    if "Build Rust Code" in text or "xcode-script" in text:
        if LITERAL not in text and "${FORCE_COLOR}" not in text:
            # project.yml / pbxproj that contain the rust script must keep literal
            if "xcode-script" in text:
                fail(f"missing literal ${{FORCE_COLOR}} in {path}")


def merge_info_plist(gen_info: Path, canonical_info: Path) -> None:
    raw_gen = gen_info.read_bytes()
    raw_can = canonical_info.read_bytes()
    raw_can = re.sub(br"<!--.*?-->", b"", raw_can, flags=re.S)
    raw_gen_nc = re.sub(br"<!--.*?-->", b"", raw_gen, flags=re.S)
    gen = plistlib.loads(raw_gen_nc)
    can = plistlib.loads(raw_can)
    for key in REQUIRED_KEYS:
        if key not in can:
            fail(f"canonical Info.ios.plist missing {key}")
        gen[key] = can[key]
    # Write XML plist with trailing LF (byte-stable)
    out = plistlib.dumps(gen, fmt=plistlib.FMT_XML)
    if not out.endswith(b"\n"):
        out += b"\n"
    gen_info.write_bytes(out)


def main() -> None:
    if len(sys.argv) != 3:
        fail("usage: ios_postprocess_gen_apple.py <gen/apple> <Info.ios.plist>")
    apple = Path(sys.argv[1])
    canonical_info = Path(sys.argv[2])
    if not apple.is_dir():
        fail(f"missing apple dir: {apple}")
    if not canonical_info.is_file():
        fail(f"missing canonical info: {canonical_info}")

    for rel in (
        "project.yml",
        "pkb-desktop.xcodeproj/project.pbxproj",
    ):
        path = apple / rel
        if not path.is_file():
            fail(f"missing {path}")
        text = path.read_text(encoding="utf-8")
        new = restore_force_color(text)
        with path.open("w", encoding="utf-8", newline="\n") as fh:
            fh.write(new)
        assert_force_color(path, new)

    gen_info = apple / "pkb-desktop_iOS" / "Info.plist"
    if not gen_info.is_file():
        fail(f"missing {gen_info}")
    merge_info_plist(gen_info, canonical_info)

    # Structural PrivacyInfo membership (exact path, not nested find)
    yml = (apple / "project.yml").read_text(encoding="utf-8")
    if "path: PrivacyInfo.xcprivacy" not in yml:
        fail("project.yml missing exact 'path: PrivacyInfo.xcprivacy'")
    privacy = apple / "PrivacyInfo.xcprivacy"
    if not privacy.is_file():
        fail("gen/apple/PrivacyInfo.xcprivacy ABSENT at root")
    pbx = (apple / "pkb-desktop.xcodeproj" / "project.pbxproj").read_text(encoding="utf-8")
    if "PrivacyInfo.xcprivacy in Resources" not in pbx:
        fail("pbxproj missing PrivacyInfo.xcprivacy in Resources membership")
    if 'path = PrivacyInfo.xcprivacy' not in pbx and 'path = "PrivacyInfo.xcprivacy"' not in pbx:
        fail("pbxproj missing exact PrivacyInfo.xcprivacy file reference path")

    # Coraxis.app product identity
    if "Coraxis.app" not in pbx:
        fail("pbxproj missing Coraxis.app productReference")
    if "pkb-desktop_iOS.app" in pbx:
        fail("pbxproj still references pkb-desktop_iOS.app")
    # Normalize PBXNativeTarget.productName attribute when XcodeGen leaves target name
    if 'productName = "pkb-desktop_iOS";' in pbx:
        pbx2 = pbx.replace('productName = "pkb-desktop_iOS";', 'productName = "Coraxis";')
        (apple / "pkb-desktop.xcodeproj" / "project.pbxproj").write_text(pbx2, encoding="utf-8")
        pbx = pbx2
    if 'productName = "Coraxis";' not in pbx and "productName = Coraxis;" not in pbx:
        fail("pbxproj productName attribute is not Coraxis")
    scheme = apple / "pkb-desktop.xcodeproj/xcshareddata/xcschemes/pkb-desktop_iOS.xcscheme"
    if scheme.is_file():
        st = scheme.read_text(encoding="utf-8")
        if 'BuildableName = "Coraxis.app"' not in st:
            fail("scheme BuildableName is not Coraxis.app")
        if "pkb-desktop_iOS.app" in st:
            fail("scheme still references pkb-desktop_iOS.app")

    # Confirm required Info keys after merge
    pl = plistlib.loads(re.sub(br"<!--.*?-->", b"", gen_info.read_bytes(), flags=re.S))
    for key in REQUIRED_KEYS:
        if key not in pl:
            fail(f"generated Info.plist missing {key}")

    print("ios_postprocess_gen_apple: GREEN")


if __name__ == "__main__":
    main()
