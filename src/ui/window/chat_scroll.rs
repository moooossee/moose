use std::{cell::Cell, rc::Rc, time::Duration};

use gtk::{glib, prelude::*};

pub(super) struct ChatScroll {
    state: Rc<State>,
}

struct State {
    scrolled: glib::WeakRef<gtk::ScrolledWindow>,
    adjustment: gtk::Adjustment,
    buffer: Option<glib::WeakRef<gtk::TextBuffer>>,
    following: Cell<bool>,
    pending: Cell<bool>,
    adjusting: Cell<bool>,
    dragging: Cell<bool>,
    selecting: Cell<Option<u32>>,
    pointer: Cell<gtk::graphene::Point>,
    last_value: Cell<f64>,
}

impl ChatScroll {
    pub(super) fn new(scrolled: &gtk::ScrolledWindow) -> Self {
        let scroll = Self::attach(scrolled, None);
        let pointer = gtk::EventControllerLegacy::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(&scroll.state);
        pointer.connect_event(move |pointer, event| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            if let Some((x, y)) = event.position() {
                state
                    .pointer
                    .set(gtk::graphene::Point::new(x as f32, y as f32));
            }
            if event.event_type() == gtk::gdk::EventType::ButtonRelease {
                state.selecting.set(None);
            }
            if event.event_type() != gtk::gdk::EventType::ButtonPress
                || !event
                    .downcast_ref::<gtk::gdk::ButtonEvent>()
                    .is_some_and(|event| event.button() == 1)
            {
                return glib::Propagation::Proceed;
            }
            let Some(scrolled) = pointer.widget().and_downcast::<gtk::ScrolledWindow>() else {
                return glib::Propagation::Proceed;
            };
            let view = state
                .pointer_in(&scrolled)
                .and_then(|point| {
                    scrolled.pick(
                        f64::from(point.x()),
                        f64::from(point.y()),
                        gtk::PickFlags::DEFAULT,
                    )
                })
                .and_then(|widget| widget.ancestor(gtk::TextView::static_type()))
                .and_downcast::<gtk::TextView>()
                .filter(|view| view.has_css_class("moose-markdown-text"));
            if let Some(view) = view {
                let selection = event.time();
                state.selecting.set(Some(selection));
                let weak = Rc::downgrade(&state);
                let view = view.downgrade();
                glib::timeout_add_local(Duration::from_millis(30), move || {
                    let Some(state) = weak.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    if state.selecting.get() != Some(selection) {
                        return glib::ControlFlow::Break;
                    }
                    let position = state.scrolled.upgrade().and_then(|scrolled| {
                        let view = view.upgrade().filter(|view| view.is_mapped())?;
                        Some((
                            state.pointer_in(&scrolled)?,
                            state.pointer_in(&view)?,
                            scrolled.height(),
                            view,
                        ))
                    });
                    let Some((point, cursor, height, view)) = position else {
                        state.selecting.set(None);
                        return glib::ControlFlow::Break;
                    };
                    let buffer = view.buffer();
                    if buffer.has_selection() {
                        state.following.set(false);
                        let delta = ((point.y() - 32.0).min(0.0)
                            + (point.y() - height as f32 + 32.0).max(0.0))
                        .clamp(-20.0, 20.0)
                        .clamp(
                            -cursor.y().max(0.0),
                            (view.height() as f32 - cursor.y()).max(0.0),
                        );
                        if delta != 0.0 {
                            let (x, y) = view.window_to_buffer_coords(
                                gtk::TextWindowType::Widget,
                                cursor.x() as i32,
                                cursor.y() as i32,
                            );
                            if let Some(iter) = view.iter_at_location(x, y) {
                                buffer.move_mark(&buffer.get_insert(), &iter);
                            }
                            state
                                .adjustment
                                .set_value(state.adjustment.value() + f64::from(delta));
                        }
                    }
                    glib::ControlFlow::Continue
                });
            }
            glib::Propagation::Proceed
        });
        scrolled.add_controller(pointer);
        scroll
    }

    pub(super) fn bind_code(scrolled: &gtk::ScrolledWindow, buffer: &impl IsA<gtk::TextBuffer>) {
        let scroll = Self::attach(scrolled, Some(buffer.as_ref()));
        scroll.state.queue_bottom();
        scrolled.connect_destroy(move |_| scroll.pause());
    }

    fn attach(scrolled: &gtk::ScrolledWindow, buffer: Option<&gtk::TextBuffer>) -> Self {
        let adjustment = scrolled.vadjustment();
        let state = Rc::new(State {
            scrolled: scrolled.downgrade(),
            last_value: Cell::new(adjustment.value()),
            adjustment,
            buffer: buffer.map(|buffer| buffer.downgrade()),
            following: Cell::new(true),
            pending: Cell::new(false),
            adjusting: Cell::new(false),
            dragging: Cell::new(false),
            selecting: Cell::new(None),
            pointer: Cell::new(gtk::graphene::Point::new(0.0, 0.0)),
        });
        let weak = Rc::downgrade(&state);
        state.adjustment.connect_changed(move |_| {
            if let Some(state) = weak.upgrade()
                && state.following.get()
            {
                state.queue_bottom();
            }
        });
        let weak = Rc::downgrade(&state);
        state.adjustment.connect_value_changed(move |adjustment| {
            if let Some(state) = weak.upgrade() {
                let previous = state.last_value.replace(adjustment.value());
                if !state.adjusting.get()
                    && !state.dragging.get()
                    && state.selecting.get().is_none()
                    && !state.following.get()
                    && adjustment.value() > previous
                    && state.remaining() <= 1.0
                {
                    state.following.set(true);
                    state.queue_bottom();
                }
            }
        });

        let weak = Rc::downgrade(&state);
        scrolled.connect_map(move |_| {
            if let Some(state) = weak.upgrade() {
                state.queue_bottom();
            }
        });
        if let Some(buffer) = buffer {
            let weak = Rc::downgrade(&state);
            buffer.connect_changed(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.queue_bottom();
                }
            });
            let weak = Rc::downgrade(&state);
            buffer.connect_has_selection_notify(move |_| {
                if let Some(state) = weak.upgrade() {
                    state.queue_bottom();
                }
            });
        }

        let wheel = gtk::EventControllerScroll::builder()
            .flags(gtk::EventControllerScrollFlags::VERTICAL)
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(&state);
        wheel.connect_scroll(move |_, _, dy| {
            if let Some(state) = weak.upgrade() {
                if dy < 0.0 {
                    state.following.set(false);
                } else if dy > 0.0 && state.remaining() <= 1.0 {
                    state.following.set(true);
                    state.queue_bottom();
                }
            }
            glib::Propagation::Proceed
        });
        scrolled.add_controller(wheel);

        let pointer = gtk::EventControllerLegacy::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(&state);
        pointer.connect_event(move |_, event| {
            if let Some(state) = weak.upgrade() {
                match event.event_type() {
                    gtk::gdk::EventType::ButtonPress | gtk::gdk::EventType::TouchBegin => {
                        state.dragging.set(true);
                        state.following.set(false);
                    }
                    gtk::gdk::EventType::ButtonRelease
                    | gtk::gdk::EventType::TouchEnd
                    | gtk::gdk::EventType::TouchCancel => {
                        state.dragging.set(false);
                        state.following.set(state.remaining() <= 1.0);
                        state.queue_bottom();
                    }
                    _ => {}
                }
            }
            glib::Propagation::Proceed
        });
        scrolled.vscrollbar().add_controller(pointer);

        let keys = gtk::EventControllerKey::builder()
            .propagation_phase(gtk::PropagationPhase::Capture)
            .build();
        let weak = Rc::downgrade(&state);
        keys.connect_key_pressed(move |_, key, _, modifiers| {
            if (matches!(
                key,
                gtk::gdk::Key::Page_Up | gtk::gdk::Key::Home | gtk::gdk::Key::Up
            ) || (key == gtk::gdk::Key::space
                && modifiers.contains(gtk::gdk::ModifierType::SHIFT_MASK)))
                && let Some(state) = weak.upgrade()
            {
                state.following.set(false);
            }
            glib::Propagation::Proceed
        });
        scrolled.add_controller(keys);
        Self { state }
    }

    pub(super) fn follow(&self) {
        self.state.following.set(true);
        self.state.queue_bottom();
    }

    pub(super) fn pause(&self) {
        self.state.following.set(false);
    }

    pub(super) fn is_following(&self) -> bool {
        self.state.following.get()
    }
}

impl State {
    fn pointer_in(&self, widget: &impl IsA<gtk::Widget>) -> Option<gtk::graphene::Point> {
        let native = widget.as_ref().native()?;
        let (x, y) = native.surface_transform();
        let point = self.pointer.get();
        native.compute_point(
            widget,
            &gtk::graphene::Point::new(point.x() - x as f32, point.y() - y as f32),
        )
    }

    fn remaining(&self) -> f64 {
        self.adjustment.upper() - self.adjustment.page_size() - self.adjustment.value()
    }

    fn queue_bottom(self: &Rc<Self>) {
        if !self.can_follow() || self.pending.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::idle_add_local_once(move || {
            if let Some(state) = weak.upgrade() {
                state.pending.set(false);
                if state.can_follow() {
                    let bottom = (state.adjustment.upper() - state.adjustment.page_size())
                        .max(state.adjustment.lower());
                    state.adjusting.set(true);
                    state.adjustment.set_value(bottom);
                    state.adjusting.set(false);
                }
            }
        });
    }

    fn can_follow(&self) -> bool {
        self.following.get()
            && !self.dragging.get()
            && self.selecting.get().is_none()
            && self
                .scrolled
                .upgrade()
                .is_some_and(|scrolled| scrolled.is_mapped())
            && !self
                .buffer
                .as_ref()
                .and_then(|buffer| buffer.upgrade())
                .is_some_and(|buffer| buffer.has_selection())
    }
}
