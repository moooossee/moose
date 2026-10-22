use gtk::{Align, Orientation, glib, prelude::*, subclass::prelude::*};

mod imp {
    use super::*;

    #[derive(Default)]
    pub struct BubbleLayout;

    #[glib::object_subclass]
    impl ObjectSubclass for BubbleLayout {
        const NAME: &'static str = "MooseMessageBubbleLayout";
        type Type = super::BubbleLayout;
        type ParentType = gtk::LayoutManager;
    }

    impl ObjectImpl for BubbleLayout {}

    impl LayoutManagerImpl for BubbleLayout {
        fn request_mode(&self, _widget: &gtk::Widget) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(
            &self,
            widget: &gtk::Widget,
            orientation: Orientation,
            for_size: i32,
        ) -> (i32, i32, i32, i32) {
            let Some(child) = widget.first_child() else {
                return (0, 0, -1, -1);
            };
            let for_size = if orientation == Orientation::Vertical && for_size >= 0 {
                for_size.min(child.measure(Orientation::Horizontal, -1).1)
            } else {
                for_size
            };
            child.measure(orientation, for_size)
        }

        fn allocate(&self, widget: &gtk::Widget, width: i32, height: i32, baseline: i32) {
            if let Some(child) = widget.first_child() {
                child.allocate(width, height, baseline, None);
            }
        }
    }
}

glib::wrapper! {
    pub struct BubbleLayout(ObjectSubclass<imp::BubbleLayout>)
        @extends gtk::LayoutManager;
}

pub(super) fn new(child: &impl IsA<gtk::Widget>) -> adw::Bin {
    adw::Bin::builder()
        .child(child)
        .layout_manager(&glib::Object::new::<BubbleLayout>())
        .halign(Align::End)
        .hexpand(false)
        .build()
}
