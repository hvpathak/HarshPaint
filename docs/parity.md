# Parity with the classic paint app

Target: the Paint app installed on Windows 10 build 19045 (`mspaint.exe` and `en-US\mspaint.exe.mui`,
copied to a test folder and studied with REA; no code or assets were copied).

## Evidence

| Id | What | Source |
|---|---|---|
| E1 | Keyboard accelerators (Ctrl+N/O/S/P/A/C/V/X/Z/Y, Ctrl+E, Ctrl+G, Ctrl+R, Ctrl+W, Ctrl+Shift+X, Ctrl+Shift+I, Ctrl+PgUp/PgDn, Ctrl+Num+/−, F11, F12, Del, Esc) | REA `inspect_pe_resources` on `mspaint.exe.mui`, Evidence `ev_248992ea77ebf4d9ce9c16ad1f5789f2c0ef224556ea2e3333999ac84e2b9bb5`; accelerator table decoded from the reported payload offsets |
| E2 | Command labels ("Resize and skew (Ctrl+W)", "Crop (Ctrl+Shift+X)", "Invert color (Ctrl+Shift+I)", "Size (Ctrl++, Ctrl+-)", "Gridlines (Ctrl+G)", "Full screen (F11)"…) | String tables in the same resource inventory (E1) |
| E3 | Default palette colours (e.g. 136,0,21 · 163,73,164 · 63,72,204 · 239,228,176 · 112,146,190 · 200,191,231) | Found as COLORREF values in `mspaint.exe` between byte offsets 57,757 and 58,018 |
| E4 | Left button paints colour 1, right button colour 2; eraser paints colour 2; right-drag eraser replaces colour 1 with colour 2; Shift constrains shapes | Observed behaviour |

## Coverage

| Feature | Status | Evidence |
|---|---|---|
| Pencil, brush, eraser, airbrush (spray), fill, colour picker, text | live | E2, E4 |
| Shapes with outline / fill, Shift for squares and 45° lines | live (12 shapes) | E4 |
| Colour 1 / colour 2, default palette, edit colours | live | E3, E4 |
| Rectangle select, transparent selection, move, delete, crop | live | E1, E2 |
| Free-form select, invert selection | missing | E2 |
| Resize (keep aspect), canvas size (Properties) | live | E1, E2 |
| Skew | missing | E2 |
| Rotate right / left / 180°, flip vertical / horizontal, invert colours | live | E2 |
| Zoom in/out (Ctrl+PgUp/PgDn), gridlines (Ctrl+G), status bar, full screen (F11) | live | E1, E2 |
| Brush size Ctrl++ / Ctrl+− | live | E1, E2 |
| Rulers (Ctrl+R) | missing | E1 |
| Print (Ctrl+P) | missing | E1 |
| Text bold/italic/underline (Ctrl+B/I/U) | missing (font family + size only) | E1 |
| Clipboard image copy/paste | live | E1 |
| Open/save PNG, JPEG, BMP, GIF | live | — |

## HarshPaint additions (not in the target)

Layers + `.hpaint`, symmetry and kaleidoscope (9 modes), smooth-stroke stabilizer, pixel-art mode,
time-lapse GIF export, fill-everywhere (Shift+fill), command palette (Ctrl+K), one-key tool shortcuts, CLI.
