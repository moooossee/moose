use std::{cell::Cell, rc::Rc};

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
    last_value: Cell<f64>,
}

impl ChatScroll {
    pub(super) fn new(scrolled: &gtk::ScrolledWindow) -> Self {
        Self::attach(scrolled, None)
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
