use gtk::prelude::*;
use std::time::Instant;

pub fn vertical(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Vertical, spacing)
}

pub fn horizontal(spacing: i32) -> gtk::Box {
    gtk::Box::new(gtk::Orientation::Horizontal, spacing)
}

pub fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.);
    label.set_wrap(true);
    label.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    if !class.is_empty() {
        label.add_css_class(class);
    }
    label
}

pub fn button(key: &str, class: &str) -> gtk::Button {
    let button = gtk::Button::with_label(&crate::i18n::tr(key));
    button.add_css_class(class);
    button
}

pub fn scroll(child: &impl IsA<gtk::Widget>) -> gtk::ScrolledWindow {
    gtk::ScrolledWindow::builder()
        .child(child)
        .vexpand(true)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .build()
}

pub fn clear(parent: &gtk::Box) {
    while let Some(child) = parent.first_child() {
        parent.remove(&child);
    }
}

pub fn timing(action: &str, started: Instant) {
    if std::env::var_os("WYSS_WS_TIMING").is_some() {
        eprintln!("{action} {:.2} ms", started.elapsed().as_secs_f64() * 1000.);
    }
}

pub fn rail_item(button: &impl IsA<gtk::Button>, icon: &str, key: &str) -> gtk::Label {
    let button = button.upcast_ref::<gtk::Button>();
    let title = crate::i18n::tr(key);
    let row = horizontal(8);
    row.append(&gtk::Image::from_icon_name(icon));
    let caption = single_line(&title, "");
    row.append(&caption);
    button.set_child(Some(&row));
    button.set_tooltip_text(Some(&title));
    button.update_property(&[gtk::accessible::Property::Label(&title)]);
    button.add_css_class("flat");
    caption
}

pub fn single_line(text: &str, class: &str) -> gtk::Label {
    let label = label(text, class);
    label.set_wrap(false);
    label.set_ellipsize(gtk::pango::EllipsizeMode::End);
    label.set_single_line_mode(true);
    label
}
