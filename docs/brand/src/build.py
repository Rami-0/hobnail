#!/usr/bin/env python3
"""Build every Hobnail brand asset from the traced mark (mark.json).

  python3 docs/brand/src/build.py          # needs: pillow, fonttools, a Chromium headless shell

Writes
  static/brand/hobnail.svg            favicon / sidebar tile (glyph, close-up)
  static/brand/icon-1024.png          app icon master (faceted, close-up)
  static/pwa/icon-512.png, icon-192.png, apple-touch-icon.png
  docs/brand/mark-glyph.svg, mark-facets.svg, wordmark.svg, wordmark-light.svg
  docs/brand/social-preview.png       the wordmark banner (1280x640)
  docs/brand/banner-pulse.png, banner-plate.png   (2560x1280)

The lettering is Big Shoulders Display Black (OFL), fetched into fonts/ on first run and
converted to outlines, so the SVGs need no font. Banner text uses Google Fonts at render time.
"""
import glob, json, math, os, random, subprocess, sys, tempfile, urllib.request
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.ttLib import TTFont
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
BRAND = os.path.dirname(HERE)
ROOT = os.path.dirname(os.path.dirname(BRAND))
P = dict(ground="#1D1210", raised="#35231D", border="#795245", cream="#FFF0E5", muted="#CDB4A8",
         rust="#F77848", rust_hi="#FF956B", rust_lo="#AC3712", rust_sh="#7A2A12", ok="#83CCA7",
         paper="#FFF8F3", ink="#2B1912", ink_muted="#725448")
M = json.load(open(os.path.join(HERE, "mark.json")))
T = M["tones"]

# ---- the mark -------------------------------------------------------------------------------
# Both cuts live on the 1024 canvas of mark.json and share one outline.
def glyph(fill=P["rust"]):
    return f'<path fill="{fill}" fill-rule="evenodd" d="{M["glyph"]}"/>'

def facets():
    return f'<path fill="{T["mid"]}" d="{M["silhouette"]}"/>' + "".join(
        f'<path fill="{T[k]}" d="{M["facets"][k]}"/>' for k in ("hi", "lo", "sh", "glint"))

FULL = "180 224 664 576"      # the whole nail, trimmed
CLOSE = "371 110 560 560"     # head and upper shank: the point runs off the corner (small sizes)
TILT = 49.5                   # degrees from upright; rotate(-TILT) stands the nail on its point
UP = (362, 182, 274, 735)     # bounding box of the upright nail (x, y, w, h)

def svg(inner, view, extra=""):
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{view}"{extra}>{inner}</svg>'

def nail(width, rot=0.0, cut=facets, style=""):
    """The nail as an inline SVG `width` px wide when upright; rot=TILT gives the brand angle."""
    x, y, w, h = UP
    return (f'<svg class="nail" width="{width}" height="{width * h / w:.1f}" viewBox="{x} {y} {w} {h}" '
            f'style="overflow:visible;transform:rotate({rot}deg);{style}">'
            f'<g transform="rotate({-TILT} 512 512)">{cut()}</g></svg>')

# ---- the wordmark: HOBNA(nail)L ----------------------------------------------------------------
def font():
    path = os.path.join(HERE, "fonts", "BigShouldersDisplay-Black.ttf")
    if not os.path.exists(path):
        os.makedirs(os.path.dirname(path), exist_ok=True)
        css = urllib.request.urlopen("https://fonts.googleapis.com/css2?family=Big+Shoulders+Display:wght@900").read().decode()
        urllib.request.urlretrieve(css.split("url(")[1].split(")")[0], path)
    return TTFont(path)

def wordmark(letters=P["cream"]):
    """Returns (inner svg, width, height). Units are font units; y grows down, cap top at 0."""
    f = font(); gs = f.getGlyphSet(); cmap = f.getBestCmap(); cap = f["OS/2"].sCapHeight
    x = 0; out = []; gap = 36
    for ch in "HOBNAIL":
        if ch == "I":
            ux, uy, uw, uh = UP
            s = cap * 1.04 / uh                      # a hair taller than the caps, as a pointed letter needs
            x += gap + 44                            # clear of the A's shoulder
            out.append(f'<g transform="translate({x:.0f} {-cap * 0.02:.0f}) scale({s:.4f}) translate({-ux} {-uy}) rotate({-TILT} 512 512)">{facets()}</g>')
            x += uw * s + gap
            continue
        g = gs[cmap[ord(ch)]]; pen = SVGPathPen(gs)
        g.draw(TransformPen(pen, (1, 0, 0, -1, x, cap)))
        out.append(f'<path fill="{letters}" d="{pen.getCommands()}"/>')
        x += g.width
    return "".join(out), x, cap

def wordmark_svg(letters, attrs=""):
    inner, w, h = wordmark(letters)
    return f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="-40 -60 {w + 80:.0f} {h + 120:.0f}"{attrs}>{inner}</svg>'

# ---- rendering -------------------------------------------------------------------------------
def chrome():
    c = os.environ.get("CH") or sorted(glob.glob(os.path.expanduser(
        "~/.cache/ms-playwright/chromium_headless_shell-*/chrome-headless-shell-linux64/chrome-headless-shell")))[-1]
    return c

FONTS = ("https://fonts.googleapis.com/css2?family=Fraunces:ital,opsz,wght,SOFT,WONK@0,9..144,400..900,0..100,0..1;"
         "1,9..144,400..900,0..100,0..1&family=Instrument+Serif:ital@0;1&family=JetBrains+Mono:wght@400;500;700&display=block")

def shoot(html, out, w, h, scale=1, colors=None):
    with tempfile.TemporaryDirectory() as d:
        page = os.path.join(d, "p.html"); raw = os.path.join(d, "p.png")
        open(page, "w").write(html)
        subprocess.run([chrome(), "--no-sandbox", "--disable-gpu", "--hide-scrollbars", f"--window-size={w},{h}",
                        f"--force-device-scale-factor={scale}", "--virtual-time-budget=8000",
                        f"--screenshot={raw}", "file://" + page], check=True, capture_output=True, timeout=90)
        im = Image.open(raw).convert("RGB")
    if colors:
        im = im.quantize(colors=colors, method=Image.Quantize.MEDIANCUT, dither=Image.Dither.NONE)
    os.makedirs(os.path.dirname(out), exist_ok=True)
    im.save(out, optimize=True)
    print(f"{os.path.relpath(out, ROOT):40} {im.size[0]}x{im.size[1]} {os.path.getsize(out):>7} B")

def page(body, css, bg, w=1280, h=640, fonts=True):
    link = f'<link rel="stylesheet" href="{FONTS}">' if fonts else ""
    return (f'<!doctype html><meta charset="utf-8">{link}<style>*{{box-sizing:border-box;margin:0}}'
            f'html,body{{width:{w}px;height:{h}px;overflow:hidden;background:{bg}}}body{{position:relative;'
            f'-webkit-font-smoothing:antialiased}}.mono{{font-family:"JetBrains Mono",monospace}}{css}</style>{body}')

def write(path, text):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    open(path, "w").write(text + "\n")
    print(f"{os.path.relpath(path, ROOT):40} {len(text) + 1:>7} B")

# ---- vectors ---------------------------------------------------------------------------------
def vectors():
    x, y, s, _ = map(int, CLOSE.split())
    box = f'x="{x}" y="{y}" width="{s}" height="{s}" rx="{s * 0.22:.0f}"'
    # The clip keeps the point inside the rounded corner it runs off.
    tile = (f'<clipPath id="hn"><rect {box}/></clipPath><rect {box} fill="{P["ground"]}"/>'
            f'<g clip-path="url(#hn)">{glyph()}</g>')
    write(os.path.join(ROOT, "static/brand/hobnail.svg"), svg(tile, CLOSE, ' class="logo" aria-hidden="true"'))
    write(os.path.join(BRAND, "mark-glyph.svg"), svg(glyph(), FULL))
    write(os.path.join(BRAND, "mark-facets.svg"), svg(facets(), FULL))
    write(os.path.join(BRAND, "wordmark.svg"), wordmark_svg(P["cream"]))
    write(os.path.join(BRAND, "wordmark-light.svg"), wordmark_svg(P["ink"]))

# ---- app icons -------------------------------------------------------------------------------
def icons():
    art = svg(facets(), CLOSE, ' width="100%" height="100%"')
    for out, px in (("static/brand/icon-1024.png", 1024), ("static/pwa/icon-512.png", 512),
                    ("static/pwa/icon-192.png", 192), ("static/pwa/apple-touch-icon.png", 180)):
        shoot(page(art, "svg{display:block}", P["ground"], px, px, fonts=False), os.path.join(ROOT, out), px, px, colors=48)

# ---- banners ---------------------------------------------------------------------------------
def struck():
    wm = wordmark_svg(P["cream"], ' class="wm"')
    body = (f'{wm}<div class="f mono"><span>A TINY HOME FOR YOUR HOMELAB</span>'
            f'<span>ONE BINARY, 10 MB, 1 MiB IDLE</span></div>')
    css = (f'.wm{{position:absolute;left:84px;top:96px;width:1112px}}'
           f'.f{{position:absolute;left:112px;right:112px;bottom:70px;display:flex;justify-content:space-between;'
           f'font-size:17px;letter-spacing:.24em;color:{P["muted"]};border-top:1px solid {P["border"]};padding-top:22px}}'
           f'.f span:first-child{{color:{P["rust"]}}}')
    shoot(page(body, css, P["ground"]), os.path.join(BRAND, "social-preview.png"), 1280, 640, colors=128)

def pulse():
    random.seed(7)
    xs = list(range(0, 1281, 16)); peak = (928, 352)
    ys = [500 + random.uniform(-9, 9) - 150 * math.exp(-((x - peak[0]) / 26) ** 2) for x in xs]
    line = "M" + " L".join(f"{x} {y:.1f}" for x, y in zip(xs, ys))
    nw = 76; nh = nw * UP[3] / UP[2]; r = math.radians(TILT)           # put the point on the peak
    left = peak[0] - nw / 2 + nh / 2 * math.sin(r) - 2; top = peak[1] - nh / 2 - nh / 2 * math.cos(r) + 4
    body = (f'<svg class="g" width="1280" height="640"><path d="{line} L1280 640 L0 640 Z" fill="{P["rust"]}" opacity=".07"/>'
            f'<path d="M0 500 H1280" stroke="{P["border"]}" stroke-dasharray="2 8"/>'
            f'<path d="{line}" fill="none" stroke="{P["rust"]}" stroke-width="2.5" stroke-linejoin="round"/></svg>'
            f'<div class="n" style="left:{left:.0f}px;top:{top:.0f}px">{nail(nw, TILT)}</div>'
            f'<div class="tag mono">03:12 postgres restarted, back in 4 s</div>'
            f'<h1>Hobnail</h1><p>a tiny home for your homelab</p>'
            f'<div class="m mono"><span><b>23</b> up</span><span><b>1.0</b> MiB</span><span><b>10</b> MB image</span><span><b>0</b> yaml</span></div>')
    css = (f'h1{{position:absolute;left:96px;top:96px;font:400 190px/1 "Instrument Serif",serif;color:{P["cream"]};letter-spacing:-.02em}}'
           f'p{{position:absolute;left:104px;top:296px;font:italic 400 46px/1 "Instrument Serif",serif;color:{P["rust_hi"]}}}'
           f'.g{{position:absolute;inset:0}}.n{{position:absolute}}'
           f'.tag{{position:absolute;right:96px;top:84px;white-space:nowrap;font-size:14px;color:{P["muted"]};'
           f'border:1px solid {P["border"]};border-radius:6px;padding:7px 12px;background:{P["raised"]}}}'
           f'.m{{position:absolute;left:104px;bottom:54px;display:flex;gap:44px;font-size:16px;color:{P["muted"]}}}'
           f'.m b{{color:{P["cream"]};font-weight:500}}')
    shoot(page(body, css, P["ground"]), os.path.join(BRAND, "banner-pulse.png"), 1280, 640, scale=2, colors=128)

def plate():
    body = (f'<div class="frame"></div><div class="t mono">PLATE I &nbsp;·&nbsp; BOOT &amp; SERVER IRONMONGERY</div>'
            f'<div class="b mono">FORGED IN RUST &nbsp;·&nbsp; MMXXVI</div>'
            f'<h1>Hobnail</h1><p>A tiny home<br>for your homelab.</p>'
            f'<div class="fig">{nail(132, TILT)}</div>'
            f'<div class="c c1 mono"><b>a.</b> HEAD<span>finds every container<br>by itself</span></div>'
            f'<div class="c c2 mono"><b>b.</b> SHANK<span>one static binary,<br>~10 MB image</span></div>'
            f'<div class="c c3 mono"><b>c.</b> POINT<span>~1 MiB of RAM<br>at idle</span></div>'
            f'<svg class="ln" width="1280" height="640"><g stroke="{P["ink"]}" stroke-width="1.2" fill="none">'
            f'<path d="M978 190 H1022"/><path d="M868 322 H1022"/><path d="M714 452 H648"/></g>'
            f'<g fill="{P["ink"]}"><circle cx="978" cy="190" r="3"/><circle cx="868" cy="322" r="3"/><circle cx="714" cy="452" r="3"/></g></svg>')
    css = (f'body{{color:{P["ink"]}}}.frame{{position:absolute;inset:28px;border:1.5px solid {P["ink"]};outline:1px solid {P["ink"]};outline-offset:5px}}'
           f'.t,.b{{position:absolute;left:0;right:0;text-align:center;font-size:13px;letter-spacing:.3em}}.t{{top:58px}}.b{{bottom:58px}}'
           f'h1{{position:absolute;left:96px;top:150px;font:900 168px/1 Fraunces,serif;font-variation-settings:"SOFT" 100,"WONK" 1,"opsz" 144;letter-spacing:-.035em}}'
           f'p{{position:absolute;left:104px;top:352px;font:italic 400 44px/1.15 Fraunces,serif;font-variation-settings:"SOFT" 100,"WONK" 1,"opsz" 60;color:{P["rust_lo"]}}}'
           f'.fig{{position:absolute;left:776px;top:142px}}.c{{position:absolute;font-size:15px;letter-spacing:.14em;line-height:1.5}}.c b{{color:{P["rust_lo"]}}}'
           f'.c span{{display:block;letter-spacing:0;font-size:16px;color:{P["ink_muted"]}}}.c1{{left:1034px;top:179px}}.c2{{left:1034px;top:311px}}'
           f'.c3{{left:448px;top:441px;text-align:right;width:188px}}.ln{{position:absolute;inset:0}}')
    shoot(page(body, css, P["paper"]), os.path.join(BRAND, "banner-plate.png"), 1280, 640, scale=2, colors=128)

if __name__ == "__main__":
    only = sys.argv[1:]
    for fn in (vectors, icons, struck, pulse, plate):
        if not only or fn.__name__ in only:
            fn()
