//! Minimal custom widget whose `snapshot` builds GSK render nodes (GPU), with no Cairo surface.

use std::cell::RefCell;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

pub type DrawFn = Box<dyn Fn(&gtk::Snapshot, f32, f32)>;
pub type ResizeFn = Box<dyn Fn(i32, i32)>;

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct TermView {
        pub draw: RefCell<Option<DrawFn>>,
        pub resize: RefCell<Option<ResizeFn>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TermView {
        const NAME: &'static str = "ProtoTermView";
        type Type = super::TermView;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for TermView {}

    impl WidgetImpl for TermView {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let w = self.obj();
            if let Some(f) = self.draw.borrow().as_ref() {
                f(snapshot, w.width() as f32, w.height() as f32);
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.parent_size_allocate(width, height, baseline);
            if let Some(f) = self.resize.borrow().as_ref() {
                f(width, height);
            }
        }
    }
}

glib::wrapper! {
    pub struct TermView(ObjectSubclass<imp::TermView>) @extends gtk::Widget, @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TermView {
    /// A widget that only draws (an icon): not focusable, not stretched.
    pub fn plain(draw: impl Fn(&TermView, &gtk::Snapshot, f32, f32) + 'static) -> TermView {
        let v: TermView = glib::Object::new();
        let weak = v.downgrade();
        *v.imp().draw.borrow_mut() = Some(Box::new(move |snap, w, h| {
            if let Some(me) = weak.upgrade() {
                draw(&me, snap, w, h);
            }
        }));
        v.set_focusable(false);
        v.set_can_target(false);
        v
    }

    pub fn new(draw: DrawFn, resize: ResizeFn) -> TermView {
        let v: TermView = glib::Object::new();
        *v.imp().draw.borrow_mut() = Some(draw);
        *v.imp().resize.borrow_mut() = Some(resize);
        v.set_hexpand(true);
        v.set_vexpand(true);
        v.set_focusable(true);
        v
    }
}
