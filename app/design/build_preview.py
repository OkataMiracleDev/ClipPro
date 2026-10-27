"""Builds a single-file, self-contained HTML preview of the UI (mock backend) for review.

Run after `npm --prefix app/ui run build`:
    python app/design/build_preview.py  ->  target/preview/clippro-preview.html

JS and CSS are inlined; font files referenced by the CSS are embedded as data URIs
(latin subsets only, to keep the file small; other subsets fall back to system fonts).
"""
import base64
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DIST = ROOT / "app" / "ui" / "dist"
OUT = ROOT / "target" / "preview" / "clippro-preview.html"

html = (DIST / "index.html").read_text(encoding="utf8")
js_files = re.findall(r'<script type="module" crossorigin src="/([^"]+)"></script>', html)
css_files = re.findall(r'<link rel="stylesheet" crossorigin href="/([^"]+)">', html)
if not js_files or not css_files:
    raise SystemExit("unexpected dist/index.html layout")

css = "\n".join((DIST / f).read_text(encoding="utf8") for f in css_files)


def embed(m):
    url = m.group(1)
    name = url.rsplit("/", 1)[-1]
    if "latin-wght" not in name and "latin-ext" not in name:
        return "url(data:font/woff2;base64,)"  # non-latin subsets: system fallback
    data = (DIST / url.lstrip("/")).read_bytes()
    return "url(data:font/woff2;base64," + base64.b64encode(data).decode() + ")"


css = re.sub(r"url\((/assets/[^)]+\.woff2)\)", embed, css)
js = "\n".join((DIST / f).read_text(encoding="utf8") for f in js_files)
# Inline module code must not contain a literal closing script tag.
js = js.replace("</script", "<\\/script")

page = f"""<title>ClipPro Preview</title>
<style>
{css}
/* Preview frame: the desktop window scrolls inside its own container on small screens. */
.preview-note {{ display: none; padding: 8px 16px; font: 12px/1.4 system-ui, sans-serif; background: var(--c-panel); color: var(--c-text-2); border-bottom: 1px solid var(--c-separator); }}
.preview-frame {{ height: 100%; overflow: auto; }}
.preview-frame > #root {{ min-width: 1180px; min-height: 680px; height: 100%; }}
html, body {{ height: 100%; }}
@media (max-width: 1180px) {{ .preview-note {{ display: block; }} .preview-frame {{ height: calc(100% - 34px); }} }}
</style>
<div class="preview-note">ClipPro is a desktop app. On a small screen, scroll sideways inside the window.</div>
<div class="preview-frame"><div id="root"></div></div>
<script type="module">
{js}
</script>
"""
OUT.parent.mkdir(parents=True, exist_ok=True)
OUT.write_text(page, encoding="utf8")
print(f"wrote {OUT} ({OUT.stat().st_size // 1024} KB)")
