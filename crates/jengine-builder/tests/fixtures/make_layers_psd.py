"""Writes layers.psd, the PSD importer's fixture (psd_import tests), with
psd-tools - a writer independent of the importer's reader. Layers, bottom
first: a full-canvas back (raw), a group "Arm" holding two layers, a
half-opaque body, a hidden layer and a screen-blended glow; every channel
but the back's RLE. psd-tools keeps a layer's transparency in its layer
mask, which is how the reader came to apply masks. It also writes
layers_expected.png, psd-tools' own composite of the document, which the
import test rebuilds from the imported prefab's parts.
Run: python make_layers_psd.py (needs psd-tools)."""
from pathlib import Path

from PIL import Image
from psd_tools import PSDImage
from psd_tools.api.layers import Group, PixelLayer
from psd_tools.constants import BlendMode, Compression

W, H = 64, 48
psd = PSDImage.new("RGB", (W, H))


def rect(w, h, rgba):
    return Image.new("RGBA", (w, h), rgba)


def layer(name, img, left, top, parent=psd, compression=Compression.RLE):
    return PixelLayer.frompil(img, parent, name, top=top, left=left, compression=compression)


layer("Back", rect(W, H, (20, 40, 60, 255)), 0, 0, compression=Compression.RAW)
arm = Group.new(psd, "Arm", open_folder=True)
# A gradient, so a wrong row or channel order shows.
upper = Image.new("RGBA", (6, 10))
upper.putdata([(x * 40, y * 25, 200, 255) for y in range(10) for x in range(6)])
layer("Upper", upper, 40, 10, parent=arm)
layer("Lower", rect(6, 12, (0, 255, 0, 255)), 40, 20, parent=arm)
body = layer("Body", rect(20, 24, (255, 0, 0, 255)), 10, 8)
body.opacity = 128
hidden = layer("Hidden", rect(4, 4, (255, 255, 255, 255)), 0, 0)
hidden.visible = False
glow = layer("Glow", rect(8, 8, (255, 255, 0, 180)), 50, 36)
glow.blend_mode = BlendMode.SCREEN
out = Path(__file__).with_name("layers.psd")
psd.save(out)

back = PSDImage.open(out)
back.composite(force=True).convert("RGBA").save(out.with_name("layers_expected.png"))
for l in back.descendants():
    print(l.kind, repr(l.name), l.bbox, l.opacity, l.visible, l.blend_mode, l.parent.name if l.parent is not back else "-")
print(back.descendants and len(list(back.descendants())), out.stat().st_size, "bytes")
