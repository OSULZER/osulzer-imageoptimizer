"""Synthetische Landschaftsfotos in Kameragröße für den Screenshot im Aufmacherbild (frei verwendbar)."""
import sys, os
import numpy as np
from PIL import Image

out = sys.argv[1]
os.makedirs(out, exist_ok=True)
W, H = 6000, 4000
rng = np.random.default_rng(7)

scenes = [
    # Himmel oben, Himmel unten, Sonne, Hügelfarben (hinten -> vorne)
    ((255, 190, 140), (250, 110, 120), (255, 236, 200), [(196, 96, 120), (140, 60, 100), (80, 34, 72)]),
    ((120, 180, 235), (220, 235, 245), (255, 250, 230), [(120, 150, 170), (70, 120, 100), (40, 80, 60)]),
    ((40, 36, 90), (230, 120, 110), (255, 210, 160), [(90, 60, 110), (60, 40, 80), (30, 22, 50)]),
    ((170, 210, 230), (245, 230, 210), (255, 245, 220), [(180, 170, 160), (130, 140, 120), (90, 100, 80)]),
    ((250, 160, 90), (255, 220, 170), (255, 250, 235), [(200, 120, 80), (150, 80, 60), (90, 50, 40)]),
]

ys = np.linspace(0, 1, H)[:, None]
xs = np.linspace(0, 1, W)[None, :]

for i, (top, bottom, sun, hills) in enumerate(scenes):
    img = np.zeros((H, W, 3), dtype=np.float32)
    for c in range(3):
        img[..., c] = top[c] + (bottom[c] - top[c]) * ys
    cx, cy, r = 0.3 + 0.1 * i, 0.38 + 0.02 * (i % 2), 0.07
    d = np.sqrt(((xs - cx) * 1.5) ** 2 + (ys - cy) ** 2)
    glow = np.clip(1 - d / (r * 3), 0, 1) ** 2
    disc = (d < r).astype(np.float32)
    for c in range(3):
        img[..., c] = img[..., c] * (1 - glow * 0.5) + sun[c] * glow * 0.5
        img[..., c] = img[..., c] * (1 - disc) + sun[c] * disc
    for k, col in enumerate(hills):
        base = 0.55 + 0.12 * k
        phase = rng.uniform(0, 6.28)
        ridge = base + 0.05 * np.sin(xs * (3 + k) * 3.14 + phase) + 0.025 * np.sin(xs * (11 + 2 * k) * 3.14 + phase * 2)
        mask = (ys > ridge).astype(np.float32)
        for c in range(3):
            img[..., c] = img[..., c] * (1 - mask) + col[c] * mask
    grain = rng.normal(0, 7, (H, W, 1)).astype(np.float32)
    img = np.clip(img + grain, 0, 255).astype(np.uint8)
    Image.fromarray(img).save(os.path.join(out, f"DSC_{142 + i:04d}.JPG"), quality=95)
    print("ok", i)
