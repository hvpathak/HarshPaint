# Dev log

## 2026-10-19: v0.1.0
- Engine: canvas with hard size caps, layers, `.hpaint` (zip of PNGs + manifest), coverage-mask strokes,
  pencil/brush/eraser/spray, fill (+ fill everywhere), 12 shapes, text (system fonts), selection with
  floating move, undo with memory budget, symmetry (9 modes), stabilizer, pixel art, time-lapse GIF.
- REA evidence from the installed paint app: accelerators, labels, palette (docs/parity.md).
- Perf: symmetric strokes recomputed the union of all copies (x24 stroke on 12 MP: 18.7 s). Now each copy's
  area is applied and uploaded separately and undo stores per-copy patches: 0.52 s (36x).
- Numbers (release, 12 MP): fill 111 ms, 1000-point stroke 47 ms, undo 4 ms, PNG encode 55 ms; window 31 ms warm.
- 70 tests. Still open: see docs/gaps.md.
