"""The showcase's videos, rendered frame by frame here and encoded by ffmpeg
(H.264, baseline profile, yuv420p: what Media Foundation and every browser
play). Run from the project root: `python tools/gen_videos.py`. Needs ffmpeg
on the PATH.

`videos/voyage.mp4` - "The Voyage", eight seconds at sea at sunset for scene
57's cinema: a sinking sun, waves, a sailing boat bobbing across, gulls, a
title at the start, film grain and a flickering vignette like an old print.
"""
import subprocess
from pathlib import Path

import numpy as np
from PIL import Image, ImageDraw, ImageFont

OUT = Path(__file__).resolve().parent.parent / "assets" / "videos"


def encode(path, frames, w, h, fps):
    path.parent.mkdir(parents=True, exist_ok=True)
    ff = subprocess.Popen([
        "ffmpeg", "-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgb24", "-s", f"{w}x{h}",
        "-r", str(fps), "-i", "-", "-c:v", "libx264", "-profile:v", "baseline", "-pix_fmt", "yuv420p",
        "-movflags", "+faststart", "-crf", "24", str(path)], stdin=subprocess.PIPE)
    for f in frames:
        ff.stdin.write(f.tobytes())
    ff.stdin.close()
    if ff.wait() != 0:
        raise SystemExit(f"ffmpeg failed on {path}")
    print(f"wrote {path.relative_to(OUT.parent.parent)}")


def voyage():
    W, H, FPS, SECONDS = 640, 360, 24, 8
    rng = np.random.default_rng(57)
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    horizon = H * 0.58
    try:
        title_font = ImageFont.truetype("georgia.ttf", 54)
    except OSError:
        title_font = ImageFont.load_default()

    def frame(f):
        t = f / FPS
        k = t / SECONDS
        # The sky from gold to rose to dusk as the sun sinks.
        top = np.array([0.25, 0.30, 0.55]) * (1 - k) + np.array([0.10, 0.10, 0.28]) * k
        low = np.array([1.0, 0.62, 0.30]) * (1 - k) + np.array([0.85, 0.35, 0.35]) * k
        s = np.clip(y / horizon, 0, 1)[..., None]
        rgb = top * (1 - s) + low * s
        sun_y = horizon - 70 + k * 85
        sun = np.exp(-(((x - W * 0.62) ** 2 + (y - sun_y) ** 2) / 900.0))
        rgb += np.array([1.0, 0.85, 0.5]) * sun[..., None] * 1.2
        rgb += np.array([1.0, 0.6, 0.3]) * np.exp(-(((x - W * 0.62) ** 2) / 8000 + ((y - sun_y) ** 2) / 3000))[..., None] * 0.35
        # The sea: dark, with the sun's road glinting on the waves.
        sea = y > horizon
        wave = np.sin(x * 0.05 + t * 2.2 + (y - horizon) * 0.4) * np.sin(x * 0.013 - t * 1.1)
        depth = np.clip((y - horizon) / (H - horizon), 0, 1)
        sea_rgb = np.array([0.08, 0.10, 0.22])[None, None, :] * (1.2 - depth)[..., None]
        road = np.exp(-((x - W * 0.62) ** 2) / (900 + 9000 * depth)) * (wave > 0.3) * (sun_y < horizon + 6)
        sea_rgb = sea_rgb + np.array([1.0, 0.7, 0.35]) * road[..., None] * 0.9
        rgb = np.where(sea[..., None], sea_rgb, rgb)
        img = Image.fromarray((np.clip(rgb, 0, 1) * 255).astype(np.uint8), "RGB")
        d = ImageDraw.Draw(img)
        # The boat, sailing left to right and bobbing on the swell.
        bx = -80 + (W + 160) * k
        by = horizon + 26 + 4 * np.sin(t * 2.4)
        tilt = 0.06 * np.sin(t * 2.4 + 0.8)
        hull = [(bx - 46, by), (bx + 46, by), (bx + 30, by + 14), (bx - 34, by + 14)]
        d.polygon(hull, fill=(20, 14, 18))
        mast_top = (bx + 60 * tilt, by - 70)
        d.line([(bx, by), mast_top], fill=(20, 14, 18), width=3)
        d.polygon([(bx + 3, by - 6), (mast_top[0] + 2, mast_top[1] + 4), (bx + 40, by - 8)], fill=(235, 220, 200))
        d.polygon([(bx - 3, by - 10), (mast_top[0] - 2, mast_top[1] + 14), (bx - 30, by - 8)], fill=(215, 200, 185))
        # Gulls, a slow V each.
        for g in range(3):
            gx = (W * (0.15 + 0.25 * g) + t * (18 + 6 * g)) % (W + 40) - 20
            gy = 70 + 25 * g + 6 * np.sin(t * 3 + g)
            flap = 6 * np.sin(t * 9 + g * 2)
            d.line([(gx - 9, gy - flap), (gx, gy), (gx + 9, gy - flap)], fill=(30, 25, 35), width=2)
        # The title, fading in and out over the first two seconds.
        if t < 2.2:
            a = int(255 * np.clip(min(t / 0.5, (2.2 - t) / 0.6), 0, 1))
            layer = Image.new("RGBA", img.size, (0, 0, 0, 0))
            ImageDraw.Draw(layer).text((W / 2, H * 0.3), "The Voyage", font=title_font, anchor="mm",
                                       fill=(255, 240, 215, a))
            img = Image.alpha_composite(img.convert("RGBA"), layer).convert("RGB")
        # An old print: grain, a flickering vignette, now and then a scratch.
        arr = np.asarray(img).astype(np.float64) / 255.0
        grain = rng.normal(0, 0.035, (H, W))[..., None]
        vig = 1 - 0.55 * (((x - W / 2) / (W / 2)) ** 2 + ((y - H / 2) / (H / 2)) ** 2) ** 1.4
        flicker = 0.94 + 0.06 * rng.random()
        arr = np.clip((arr + grain) * vig[..., None] * flicker, 0, 1)
        if rng.random() < 0.25:
            sx = int(rng.uniform(0, W))
            arr[:, sx:sx + 1] = np.clip(arr[:, sx:sx + 1] + 0.25, 0, 1)
        return Image.fromarray((arr * 255).astype(np.uint8), "RGB")

    encode(OUT / "voyage.mp4", (frame(f) for f in range(FPS * SECONDS)), W, H, FPS)


if __name__ == "__main__":
    voyage()
