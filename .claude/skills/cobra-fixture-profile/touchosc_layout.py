#!/usr/bin/env python3
"""Reusable TouchOSC (.touchosc / v17) layout helpers for Cobra Commander fixture templates.

A .touchosc file is a ZIP holding one base64-flavoured index.xml. This module provides control
emitters and label helpers, then a worked example (`build_lilchonker`) you can copy/adapt.

COORDINATE SYSTEM (see touchosc-template.md and the repo's src/touchosc/CLAUDE.md):
  orientation="vertical" renders LANDSCAPE (90deg CCW). In the XML:
    XML x = editor vertical axis (increases UPWARD)   XML w = editor height
    XML y = editor horizontal axis (increases right)  XML h = editor width
    labelv reads left-to-right in the editor.
  "Columns" below are constant-x bands stacked along the vertical axis; higher x = higher on
  screen. To order bands top->bottom, give them DECREASING x. A full-width fader is w~84 h~1015.

Usage:  python3 touchosc_layout.py [output.touchosc]
"""
import base64, zipfile, os, sys


def make_layout(fixture_name, build):
    """Run a `build(api)` callback, returning the index.xml string for `fixture_name`.

    `api` exposes the emitters/helpers below (all coordinates are raw XML/portrait values).
    """
    controls, counter = [], [0]

    def b(s):
        return base64.b64encode(s.encode()).decode()

    def cid():
        counter[0] += 1
        return b(f"ctrl{counter[0]}")

    def ctrl(**a):
        controls.append("<control " + " ".join(f'{k}="{v}"' for k, v in a.items()) + " ></control>")

    # --- control emitters (osc names are relative to the fixture) ---
    def fader(osc, x, y, w, h, color, centered):
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color,
             scalef=("-1.0" if centered else "0.0"), scalet="1.0",
             osc_cs=b(f"/{fixture_name}/{osc}"), type="faderv", response="absolute",
             inverted="false", centered=("true" if centered else "false"))

    def toggle(osc, x, y, w, h, color):
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color, scalef="0.0", scalet="1.0",
             osc_cs=b(f"/{fixture_name}/{osc}"), type="toggle", local_off="true")

    def push(osc, x, y, w, h, color):
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color, scalef="0.0", scalet="1.0",
             osc_cs=b(f"/{fixture_name}/{osc}"), type="push", local_off="true", sp="true", sr="false")

    def multipush(osc, x, y, w, h, color, ny):
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color, scalef="0.0", scalet="1.0",
             osc_cs=b(f"/{fixture_name}/{osc}"), type="multipush", number_x="1", number_y=str(ny),
             local_off="true")

    # --- label helpers ---
    def rawlabel(text, x, y, w, h, color, bg, size=20):
        """A label at exact coords (no centering); for cribbed fixed clusters."""
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color, type="labelv",
             text=b(text), size=str(size), background=("true" if bg else "false"), outline="false")

    def writeable_label(osc, x, y, w, h, color):
        """An OSC-addressable label (e.g. positioner preset names)."""
        ctrl(name=cid(), x=x, y=y, w=w, h=h, color=color,
             osc_cs=b(f"/{fixture_name}/{osc}"), type="labelv", text="", size="20",
             background="true", outline="false")

    def fader_label(text, cx, cy, cw, ch, color):
        """Fader/whole-control label: no background, centered along x, tucked to low-y end."""
        lw = 33
        lh = min(ch - 10, len(text) * 14 + 18)
        rawlabel(text, cx + (cw - lw) // 2, cy + 6, lw, lh, color, bg=False)

    def wrap(name, budget):
        lines, cur = [], ""
        for w in name.split():
            if cur and len(cur) + 1 + len(w) > budget:
                lines.append(cur); cur = w
            else:
                cur = (cur + " " + w) if cur else w
        if cur:
            lines.append(cur)
        return lines or [name]

    def stacked_label(lines, bx, by, bw, bh, size=20):
        """(text,color) lines stacked along x, centered on a button, tight boxes (with bg).

        Line 0 sits on top (highest x). lw = size+12 leaves room for descenders.
        """
        n = len(lines)
        lw = min(size + 12, (bw - 4) // n)
        x0 = bx + (bw - n * lw) // 2
        for j, (text, color) in enumerate(lines):
            lh = min(bh - 4, len(text) * 12 + 14)
            ly = by + (bh - lh) // 2
            rawlabel(text, x0 + (n - 1 - j) * lw, ly, lw, lh, color, bg=True, size=size)

    def button_label(name, bx, by, bw, bh, color="gray", size=20):
        """Single-button name label: word-wrapped to fit, centered both axes, tight box + bg."""
        budget = max(4, (bh - 14) // 12)
        stacked_label([(ln, color) for ln in wrap(name, budget)], bx, by, bw, bh, size)

    api = dict(b=b, fader=fader, toggle=toggle, push=push, multipush=multipush,
               rawlabel=rawlabel, writeable_label=writeable_label, fader_label=fader_label,
               stacked_label=stacked_label, button_label=button_label)

    build(api)

    tab = (f'<tabpage name="{b(fixture_name)}" scalef="0.0" scalet="1.0" '
           f'li_t="{b(fixture_name)}" li_c="gray" li_s="14" li_o="false" li_b="false" '
           f'la_t="{b(fixture_name)}" la_c="gray" la_s="14" la_o="false" la_b="false" >')
    return ('<?xml version="1.0" encoding="UTF-8"?>'
            '<layout version="17" mode="1" orientation="vertical">'
            + tab + "".join(controls) + "</tabpage></layout>")


def write_touchosc(path, xml):
    with zipfile.ZipFile(path, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("index.xml", xml)


# Cribbed-by-eye constants from existing templates (don't re-derive):
IWASH_PRESET_Y = [15, 143, 270, 396, 523, 649, 776, 903]  # PositionPresetLabel/0..7


# ---------------------------------------------------------------------------
# WORKED EXAMPLE: the LilChonker (Chode 90W spot) layout. Copy and adapt.
# Bands top->bottom: dimmer/focus, color, fixed gobo, rotating gobo (decreasing x).
# ---------------------------------------------------------------------------
def build_lilchonker(api):
    fader, toggle, push, multipush = api["fader"], api["toggle"], api["push"], api["multipush"]
    rawlabel, writeable_label = api["rawlabel"], api["writeable_label"]
    fader_label, stacked_label, button_label = api["fader_label"], api["stacked_label"], api["button_label"]
    # column x positions (w=84, step 90); canvas ~727 x 1024 (portrait XML)
    c0, c1, c2, c3, c4, c5, c6, c7 = 3, 93, 183, 273, 363, 453, 543, 633
    W = 84

    # c0 positioner presets (cribbed from IWashLed): select multipush + 8 writeable labels
    multipush("PositionPresetSelect", c0, 3, W, 1018, "purple", 8)
    for i in range(8):
        writeable_label(f"PositionPresetLabel/{i}", c0 + (W - 25) // 2, IWASH_PRESET_Y[i], 25, 105, "purple")

    # c1/c2 pan & tilt — full-width faders for max 16-bit resolution. (Mirror toggles omitted;
    # they're set-once geometry. The MirrorPan/MirrorTilt OSC controls still exist on the fixture.)
    fader("Pan", c1, 3, W, 1015, "blue", centered=True);   fader_label("pan", c1, 3, W, 1015, "blue")
    fader("Tilt", c2, 3, W, 1015, "green", centered=True); fader_label("tilt", c2, 3, W, 1015, "green")

    # c3 gobo rotation (top) + prism toggle + prism rotation (bottom)
    fader("GoboRotation", c3, 3, W, 510, "yellow", centered=True); fader_label("gobo rotation", c3, 3, W, 510, "yellow")
    toggle("Prism", c3, 523, W, 84, "orange"); button_label("prism", c3, 523, W, 84, "orange")
    fader("PrismRotation", c3, 615, W, 400, "orange", centered=False); fader_label("prism rotation", c3, 615, W, 400, "orange")

    # c6 (top of the wheel group): color — colors first, SplitColor toggle to the RIGHT (high y).
    # Display labels lowercase; push osc_cs keeps the profile's capitalized labels.
    color_slots = [("Open", "gray"), ("Red", "red"), ("Green", "green"), ("Blue", "blue"),
                   ("Yellow", "yellow"), ("Orange", "orange"), ("Magenta", "pink"), ("Cyan", "blue")]
    stx, sty = c6, 1018 - 84
    cstep = (sty - 5 - 3) // len(color_slots); cbh = cstep - 2
    for i, (name, col) in enumerate(color_slots):
        y = 3 + i * cstep
        push(f"Color/{name}", c6, y, W, cbh, col); button_label(name.lower(), c6, y, W, cbh, col)
    toggle("SplitColor", stx, sty, W, 84, "gray")            # cribbed split indicator (WizardExtreme):
    rawlabel("color?", stx + 12, sty + 13, 33, 59, "gray", bg=True)
    rawlabel("split",  stx + 39, sty + 20, 33, 43, "gray", bg=True)
    rawlabel("→", stx + 26, sty + 19, 33, 43, "gray", bg=False)  # arrow last -> drawn in front

    # c5 fixed gobo wheel — one name label per button (index 0 = open)
    FIXED = ["open", "gears", "breakup", "diamonds", "bubbles", "asym tri spiral", "basket", "teeth", "pyramid"]
    multipush("Gobo", c5, 3, W, 1015, "gray", len(FIXED))
    gcell = 1015 / len(FIXED)
    for i, nm in enumerate(FIXED):
        button_label(nm, c5, int(3 + i * gcell), W, int(gcell))

    # c4 rotating gobo wheel — "pents on blue" is a white-on-blue litho (two lines, 2nd blue)
    ROT = ["open", "breakup", "triangle", "spiral", "pents on blue", "three dots", "offset dot"]
    multipush("RotatingGobo", c4, 3, W, 1015, "blue", len(ROT))
    rcell = 1015 / len(ROT)
    for i, nm in enumerate(ROT):
        by, bh = int(3 + i * rcell), int(rcell)
        if nm == "pents on blue":
            stacked_label([("pents", "gray"), ("on blue", "blue")], c4, by, W, bh)
        else:
            button_label(nm, c4, by, W, bh)

    # c7 dimmer + focus
    fader("Dimmer", c7, 3, W, 500, "yellow", centered=False); fader_label("dimmer", c7, 3, W, 500, "yellow")
    fader("Focus", c7, 515, W, 500, "gray", centered=True);   fader_label("focus", c7, 515, W, 500, "gray")


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "LilChonker.touchosc"
    write_touchosc(out, make_layout("LilChonker", build_lilchonker))
    print("wrote", os.path.abspath(out))
