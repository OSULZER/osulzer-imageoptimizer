# Hero image source

`hero.html` is rendered with headless Chrome:

```
chrome --headless=new --hide-scrollbars --window-size=1280,640 --force-device-scale-factor=2 --screenshot=hero.png hero.html
```

- `--force-device-scale-factor=2` gives `docs/images/hero.png` (README), factor 1 the 1280×640 social preview.
- `hero.html` is self-contained (fonts from Google Fonts); `thumb.jpg` is the downscaled `DSC_0144.JPG`.
- `app-ai.png` is a real screenshot of the app (English UI, AI description with LM Studio and the note "evening view from a mountain lodge in tyrol, austria", after Optimize). The AI card shows the texts the app embedded in `evening-sun-over-tyrol-mountains.jpg`.
- The sample photos are synthetic (`make_photos.py`, needs Pillow and NumPy) and free to use.
