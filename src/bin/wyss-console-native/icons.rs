//! Line icons drawn with GSK paths (no icon theme, no files, no fonts): a 24x24 grid scaled to the
//! widget, stroked in the widget's CSS `color` so hover and active states recolor them.

use gtk::prelude::*;
use gtk::{graphene, gsk};

use crate::view::TermView;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Icon {
    Terminal,
    Grid,
    Palette,
    Settings,
    Folder,
    SplitRow,
    SplitCol,
    Keyboard,
}

fn line(b: &gsk::PathBuilder, pts: &[(f32, f32)], k: f32) {
    b.move_to(pts[0].0 * k, pts[0].1 * k);
    for p in &pts[1..] {
        b.line_to(p.0 * k, p.1 * k);
    }
}

fn rrect(b: &gsk::PathBuilder, x: f32, y: f32, w: f32, h: f32, r: f32, k: f32) {
    b.add_rounded_rect(&gsk::RoundedRect::from_rect(graphene::Rect::new(x * k, y * k, w * k, h * k), r * k));
}

/// A slider line with a knob: the line stops at the knob.
fn slider(b: &gsk::PathBuilder, y: f32, knob: f32, k: f32) {
    line(b, &[(4.0, y), (knob - 2.6, y)], k);
    line(b, &[(knob + 2.6, y), (20.0, y)], k);
    b.add_circle(&graphene::Point::new(knob * k, y * k), 2.4 * k);
}

/// (stroked path, dots to fill) for an icon on the 24-unit grid scaled by `k`.
fn paths(icon: Icon, k: f32) -> (gsk::Path, Option<gsk::Path>) {
    let b = gsk::PathBuilder::new();
    let mut dots = None;
    match icon {
        Icon::Terminal => {
            line(&b, &[(5.0, 7.0), (10.5, 12.0), (5.0, 17.0)], k);
            line(&b, &[(13.0, 18.0), (19.0, 18.0)], k);
        }
        Icon::Grid => {
            for (x, y) in [(4.0, 4.0), (13.5, 4.0), (4.0, 13.5), (13.5, 13.5)] {
                rrect(&b, x, y, 6.5, 6.5, 1.8, k);
            }
        }
        Icon::Settings => {
            slider(&b, 8.0, 15.0, k);
            slider(&b, 16.0, 9.0, k);
        }
        Icon::Palette => {
            b.add_circle(&graphene::Point::new(12.0 * k, 12.0 * k), 8.5 * k);
            let d = gsk::PathBuilder::new();
            for (x, y) in [(8.0, 10.5), (12.0, 7.5), (16.0, 10.5), (8.5, 15.0)] {
                d.add_circle(&graphene::Point::new(x * k, y * k), 1.1 * k);
            }
            dots = Some(d);
        }
        Icon::Folder => line(&b, &[(3.0, 7.0), (3.0, 18.0), (21.0, 18.0), (21.0, 9.0), (11.5, 9.0), (9.5, 6.0), (3.0, 6.0), (3.0, 7.0)], k),
        Icon::SplitRow => {
            rrect(&b, 3.0, 4.0, 18.0, 16.0, 2.5, k);
            line(&b, &[(12.0, 4.0), (12.0, 20.0)], k);
        }
        Icon::SplitCol => {
            rrect(&b, 3.0, 4.0, 18.0, 16.0, 2.5, k);
            line(&b, &[(3.0, 12.0), (21.0, 12.0)], k);
        }
        Icon::Keyboard => {
            rrect(&b, 2.0, 6.0, 20.0, 12.0, 2.5, k);
            for x in [6.0, 10.0, 14.0, 18.0] {
                line(&b, &[(x, 10.0), (x + 0.01, 10.0)], k);
            }
            line(&b, &[(6.0, 14.0), (6.01, 14.0)], k);
            line(&b, &[(9.5, 14.0), (14.5, 14.0)], k);
            line(&b, &[(18.0, 14.0), (18.01, 14.0)], k);
        }
    }
    (b.to_path(), dots.map(|d| d.to_path()))
}

/// An icon widget `size` pixels square.
pub fn widget(icon: Icon, size: i32) -> TermView {
    let v = TermView::plain(move |me, snap, w, h| {
        let k = w.min(h) / 24.0;
        let (path, dots) = paths(icon, k);
        let color = me.color();
        let stroke = gsk::Stroke::new(1.6 * k);
        stroke.set_line_cap(gsk::LineCap::Round);
        stroke.set_line_join(gsk::LineJoin::Round);
        snap.append_stroke(&path, &stroke, &color);
        if let Some(d) = dots {
            snap.append_fill(&d, gsk::FillRule::Winding, &color);
        }
    });
    v.set_size_request(size, size);
    v.set_halign(gtk::Align::Center);
    v.set_valign(gtk::Align::Center);
    v
}
