//! Draws the primitives of [`super::layout`] with egui's painter.

use egui::text::{LayoutJob, TextFormat};
use egui::{Painter, Pos2, Shape, Stroke, pos2, vec2};

use super::{LineStyle, Primitive};

/// Points a bold glyph is drawn a second time to the right, as fonts without a bold face have
/// no other way to look bold.
const FAUX_BOLD: f32 = 0.6;

fn line(painter: &Painter, from: Pos2, to: Pos2, color: egui::Color32, style: LineStyle) {
    let stroke = Stroke::new(1.0, color);
    match style {
        LineStyle::Solid => {
            painter.line_segment([from, to], stroke);
        }
        LineStyle::Double => {
            painter.line_segment([from, to], stroke);
            painter.line_segment([from + vec2(0.0, 2.0), to + vec2(0.0, 2.0)], stroke);
        }
        LineStyle::Curly => {
            let mut points = Vec::new();
            let mut x = from.x;
            let mut up = true;
            while x <= to.x {
                points.push(pos2(x, from.y + if up { -1.0 } else { 1.0 }));
                up = !up;
                x += 2.0;
            }
            painter.add(Shape::line(points, stroke));
        }
        LineStyle::Dotted => {
            painter.extend(Shape::dotted_line(&[from, to], color, 2.0, 0.5));
        }
        LineStyle::Dashed => {
            painter.extend(Shape::dashed_line(&[from, to], stroke, 3.0, 2.0));
        }
    }
}

pub fn paint(painter: &Painter, primitives: &[Primitive]) {
    for primitive in primitives {
        match primitive {
            Primitive::Rect { rect, color } => {
                painter.rect_filled(*rect, 0.0, *color);
            }
            Primitive::Text { pos, width, text, color, font, italic, bold } => {
                let mut job = LayoutJob::default();
                job.append(text, 0.0, TextFormat { font_id: font.clone(), color: *color, italics: *italic, ..TextFormat::default() });
                let galley = painter.layout_job(job);
                let x = pos.x + ((width - galley.size().x) / 2.0).max(0.0);
                let at = pos2(x, pos.y);
                if *bold {
                    painter.galley(at + vec2(FAUX_BOLD, 0.0), galley.clone(), *color);
                }
                painter.galley(at, galley, *color);
            }
            Primitive::Line { from, to, color, style } => line(painter, *from, *to, *color, *style),
        }
    }
}
