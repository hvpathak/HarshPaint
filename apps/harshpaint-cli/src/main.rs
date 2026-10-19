//! Headless front end on the same engine as the desktop app.

use std::path::Path;
use std::process::ExitCode;

use harshpaint_core::Result;
use harshpaint_core::color::Rgba;
use harshpaint_core::commands::{CATALOG, Command};
use harshpaint_core::document::Document;
use harshpaint_core::image_ops::{self, Rotation};
use harshpaint_core::io;
use harshpaint_core::session::{Button, Mods, Session, Tool};
use harshpaint_core::symmetry::Symmetry;
use harshpaint_core::timelapse::{self, TimelapseOptions};

const USAGE: &str = "usage:
  harshpaint-cli commands
  harshpaint-cli info <picture>
  harshpaint-cli convert <in> <out>                 (png, jpg, bmp, gif, hpaint)
  harshpaint-cli resize <in> <out> <width>x<height>
  harshpaint-cli rotate <in> <out> 90|180|270
  harshpaint-cli flip <in> <out> h|v
  harshpaint-cli invert <in> <out>
  harshpaint-cli demo <out.png> [timelapse.gif]     (paints a kaleidoscope with the engine)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            println!("{out}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> std::result::Result<String, String> {
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let text = |r: Result<String>| r.map_err(|e| e.to_string());
    match args.as_slice() {
        ["commands"] => Ok(list_commands()),
        ["info", path] => text(info(Path::new(path))),
        ["convert", input, output] => text(transform(input, output, |d| Ok(d.clone()))),
        ["resize", input, output, size] => {
            let (w, h) = parse_size(size)
                .ok_or_else(|| format!("size must look like 800x600, got {size}"))?;
            text(transform(input, output, |d| {
                per_layer(d, |r| image_ops::resize(r, w, h, true))
            }))
        }
        ["rotate", input, output, degrees] => {
            let rotation = match *degrees {
                "90" => Rotation::Right90,
                "180" => Rotation::Half,
                "270" => Rotation::Left90,
                other => return Err(format!("rotate by 90, 180 or 270, not {other}")),
            };
            text(transform(input, output, |d| {
                per_layer(d, |r| image_ops::rotate(r, rotation))
            }))
        }
        ["flip", input, output, axis] => {
            let horizontal = match *axis {
                "h" => true,
                "v" => false,
                other => return Err(format!("flip h or v, not {other}")),
            };
            text(transform(input, output, |d| {
                per_layer(d, |r| image_ops::flip(r, horizontal))
            }))
        }
        ["invert", input, output] => text(transform(input, output, |d| {
            per_layer(d, |r| {
                let mut out = r.clone();
                image_ops::invert(&mut out);
                Ok(out)
            })
        })),
        ["bench"] => text(bench()),
        ["demo", out] => text(demo(Path::new(out), None)),
        ["demo", out, gif] => text(demo(Path::new(out), Some(Path::new(gif)))),
        _ => Err(USAGE.to_owned()),
    }
}

fn list_commands() -> String {
    CATALOG
        .iter()
        .map(|s| {
            let menu = s.menu.map_or("-", |m| m.label());
            let key = s.shortcut.map(|k| k.label()).unwrap_or_default();
            format!("{:<24} {:<9} {:<26} {key}", s.id, menu, s.label)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn info(path: &Path) -> Result<String> {
    let doc = io::open(path)?;
    Ok(format!(
        "file: {}\nsize: {} x {} px\nlayers: {}",
        path.display(),
        doc.width(),
        doc.height(),
        doc.layers().len()
    ))
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

fn per_layer(
    doc: &Document,
    f: impl Fn(&harshpaint_core::canvas::Raster) -> Result<harshpaint_core::canvas::Raster>,
) -> Result<Document> {
    let mut out = doc.clone();
    let layers = doc
        .layers()
        .iter()
        .map(|l| {
            Ok(harshpaint_core::document::Layer {
                raster: f(&l.raster)?,
                ..l.clone()
            })
        })
        .collect::<Result<Vec<_>>>()?;
    out.replace_layers(layers)?;
    Ok(out)
}

fn transform(
    input: &str,
    output: &str,
    f: impl Fn(&Document) -> Result<Document>,
) -> Result<String> {
    let doc = io::open(Path::new(input))?;
    let out = f(&doc)?;
    io::save(&out, Path::new(output))?;
    Ok(format!(
        "wrote {output} ({} x {} px)",
        out.width(),
        out.height()
    ))
}

/// Times the heavy operations on a 12-megapixel picture (4000 × 3000).
fn bench() -> Result<String> {
    use std::time::Instant;
    let ms = |t: Instant| t.elapsed().as_secs_f64() * 1000.0;
    let mut s = Session::new(Document::new(4000, 3000, Rgba::WHITE)?);

    s.run(Command::ToolFill)?;
    let t = Instant::now();
    s.pointer_down((2000.0, 1500.0), Button::Primary, Mods::default());
    let fill = ms(t);

    s.run(Command::ToolBrush)?;
    s.settings.size = 24;
    let points: Vec<(f32, f32)> = (0..1000)
        .map(|i| {
            let u = i as f32 / 999.0;
            (200.0 + 3600.0 * u, 1500.0 + 900.0 * (u * 12.0).sin())
        })
        .collect();
    let t = Instant::now();
    stroke(&mut s, &points);
    let brush = ms(t);

    s.settings.symmetry = Symmetry::Radial {
        arms: 12,
        mirror: true,
    };
    let t = Instant::now();
    stroke(&mut s, &points);
    let kaleido = ms(t);

    let t = Instant::now();
    let png = io::encode(&s.doc, io::FileKind::Png)?;
    let save = ms(t);

    let t = Instant::now();
    s.run(Command::Undo)?;
    let undo = ms(t);

    Ok(format!(
        "12 MP picture (4000 x 3000)\n\
         fill whole picture        {fill:>8.1} ms\n\
         1000-point brush stroke   {brush:>8.1} ms\n\
         same stroke, x24 symmetry {kaleido:>8.1} ms\n\
         undo that stroke          {undo:>8.1} ms\n\
         encode PNG ({} KB)     {save:>8.1} ms",
        png.len() / 1024
    ))
}

/// Paints a 12-way kaleidoscope mandala using the same tools a person would.
fn demo(out: &Path, gif: Option<&Path>) -> Result<String> {
    let size = 1024u32;
    let mut s = Session::new(Document::new(size, size, Rgba::rgb(10, 14, 32))?);
    s.run(Command::LayerNew)?;
    s.run(Command::ToolBrush)?;
    s.settings.symmetry = Symmetry::Radial {
        arms: 12,
        mirror: true,
    };
    s.settings.stabilizer = 20;
    let c = size as f32 / 2.0;
    for k in 0..34u32 {
        let t = k as f32 / 34.0;
        s.settings.color1 = Rgba::from_hsv(190.0 + 170.0 * t, 0.75 - 0.3 * t, 1.0);
        s.settings.size = 3 + (k * 7 % 13);
        s.settings.hardness = if k % 3 == 0 { 0.2 } else { 0.8 };
        // One petal-shaped curve inside a single sector; symmetry makes the rest.
        let r0 = 40.0 + 430.0 * ((k * 11) % 34) as f32 / 34.0;
        let reach = 60.0 + 120.0 * ((k * 5) % 7) as f32 / 7.0;
        let points: Vec<(f32, f32)> = (0..=48)
            .map(|i| {
                let u = i as f32 / 48.0;
                let r = r0 + reach * u;
                let a = -std::f32::consts::FRAC_PI_2
                    + 0.24 * (u * std::f32::consts::PI).sin()
                    + 0.02 * k as f32;
                (c + r * a.cos(), c + r * a.sin())
            })
            .collect();
        stroke(&mut s, &points);
    }
    // A ring of dots with the spray tool.
    s.run(Command::ToolSpray)?;
    s.settings.color1 = Rgba::rgb(255, 236, 160);
    s.settings.size = 6;
    let ring: Vec<(f32, f32)> = (0..20)
        .map(|i| (c + 18.0 * i as f32 / 20.0, c - 300.0))
        .collect();
    stroke(&mut s, &ring);
    debug_assert_eq!(s.tool(), Tool::Spray);

    io::save(&s.doc, out)?;
    let mut msg = format!(
        "wrote {} ({} strokes, {} layers)",
        out.display(),
        s.history().len(),
        s.doc.layers().len()
    );
    if let Some(gif) = gif {
        let bytes = timelapse::render(&s.doc, s.history(), &TimelapseOptions::default())?;
        io::write_atomically(gif, &bytes)?;
        msg.push_str(&format!(
            ", time-lapse {} ({} KB)",
            gif.display(),
            bytes.len() / 1024
        ));
    }
    Ok(msg)
}

fn stroke(s: &mut Session, points: &[(f32, f32)]) {
    let m = Mods::default();
    let Some((&first, rest)) = points.split_first() else {
        return;
    };
    s.pointer_down(first, Button::Primary, m);
    for &p in rest {
        s.pointer_move(p, m);
    }
    s.pointer_up(points.last().copied().unwrap_or(first), m);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    fn temp(name: &str) -> String {
        std::env::temp_dir()
            .join(format!("hpaint-cli-{}-{name}", std::process::id()))
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn demo_then_transform_end_to_end() {
        let (png, gif, small, rot) = (
            temp("demo.png"),
            temp("demo.gif"),
            temp("small.jpg"),
            temp("rot.hpaint"),
        );
        let out = run(&args(&["demo", &png, &gif])).unwrap();
        assert!(out.contains("strokes"), "{out}");
        assert!(run(&args(&["info", &png])).unwrap().contains("1024 x 1024"));
        run(&args(&["resize", &png, &small, "256x128"])).unwrap();
        assert!(run(&args(&["info", &small])).unwrap().contains("256 x 128"));
        run(&args(&["rotate", &small, &rot, "90"])).unwrap();
        assert!(run(&args(&["info", &rot])).unwrap().contains("128 x 256"));
        for p in [png, gif, small, rot] {
            std::fs::remove_file(p).ok();
        }
    }

    #[test]
    fn bad_arguments_are_errors() {
        assert!(run(&args(&[])).is_err());
        assert!(run(&args(&["resize", "a.png", "b.png", "big"])).is_err());
        assert!(run(&args(&["rotate", "a.png", "b.png", "45"])).is_err());
        assert!(run(&args(&["info", "no-such-file.png"])).is_err());
        assert!(
            run(&args(&["commands"]))
                .unwrap()
                .contains("symmetry.kaleido12")
        );
        assert_eq!(parse_size("800x600"), Some((800, 600)));
    }
}
