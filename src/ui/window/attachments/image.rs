use std::cell::Cell;

use gtk::{gdk, glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct ImageLayout {
        pub size: Cell<(i32, i32)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ImageLayout {
        const NAME: &'static str = "MooseImageLayout";
        type Type = super::ImageLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for ImageLayout {}

    impl LayoutManagerImpl for ImageLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            _widget: &gtk::Widget,
            orientation: gtk::Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let (width, height) = self.size.get();
            if orientation == gtk::Orientation::Horizontal {
                (width.min(96), width, -1, -1)
            } else {
                let height = if for_size < 0 {
                    height
                } else {
                    (f64::from(height) * f64::from(for_size.min(width)) / f64::from(width)).round()
                        as i32
                };
                (height, height, -1, -1)
            }
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = widget.first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct ImageLayout(ObjectSubclass<imp::ImageLayout>)
        @extends gtk::LayoutManager;
}

pub(super) fn preview(texture: &gdk::Texture, name: &str, frame: Option<(i32, i32)>) -> adw::Bin {
    let size = frame.unwrap_or_else(|| {
        let width = f64::from(texture.width().max(1));
        let height = f64::from(texture.height().max(1));
        let scale = (96.0 / width)
            .max(72.0 / height)
            .max(1.0)
            .min(180.0 / width)
            .min(144.0 / height);
        (
            (width * scale).round().max(1.0) as i32,
            (height * scale).round().max(1.0) as i32,
        )
    });
    let layout = glib::Object::new::<ImageLayout>();
    layout.imp().size.set(size);
    let picture = gtk::Picture::builder()
        .paintable(texture)
        .content_fit(gtk::ContentFit::Contain)
        .can_shrink(true)
        .can_target(false)
        .alternative_text(name)
        .build();
    adw::Bin::builder()
        .child(&picture)
        .layout_manager(&layout)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["moose-sent-image-preview"])
        .build()
}
