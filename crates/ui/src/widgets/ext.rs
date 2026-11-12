//! Extension traits: widget handlers built from a view handle, and text size with colour.

use super::*;

/// Builds widget event handlers that run on a view, so a screen does not have to clone
/// its handle and call `update` in every button.
pub trait Listen<V: 'static> {
    /// A handler for `on_click` and the like that runs `f` on the view.
    fn listen(
        &self,
        f: impl Fn(&mut V, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static;

    /// A handler for `on_change` and the like that passes its value on to `f`.
    fn listen_with<A: 'static>(
        &self,
        f: impl Fn(&mut V, A, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(A, &mut Window, &mut App) + 'static;

    /// A handler for GPUI's own `on_click` and mouse events that passes the event on to `f`.
    fn listen_event<E: ?Sized + 'static>(
        &self,
        f: impl Fn(&mut V, &E, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static;
}

impl<V: 'static> Listen<V> for gpui::Entity<V> {
    fn listen(
        &self,
        f: impl Fn(&mut V, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(&mut Window, &mut App) + 'static {
        let view = self.clone();
        move |window, cx| view.update(cx, |view, cx| f(view, window, cx))
    }

    fn listen_with<A: 'static>(
        &self,
        f: impl Fn(&mut V, A, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(A, &mut Window, &mut App) + 'static {
        let view = self.clone();
        move |value, window, cx| view.update(cx, |view, cx| f(view, value, window, cx))
    }

    fn listen_event<E: ?Sized + 'static>(
        &self,
        f: impl Fn(&mut V, &E, &mut Window, &mut gpui::Context<V>) + 'static,
    ) -> impl Fn(&E, &mut Window, &mut App) + 'static {
        let view = self.clone();
        move |event, window, cx| view.update(cx, |view, cx| f(view, event, window, cx))
    }
}

/// Style pairs that nearly every element needs, set in one call with a palette colour
/// such as `T.muted`.
pub trait StyleExt: Styled + Sized {
    /// Text at `size` pixels in `colour`.
    fn text_as(self, size: f32, colour: u32) -> Self {
        self.text_size(px(size)).text_color(rgb(colour))
    }

    /// A 1px border on every side in `colour`.
    fn border_as(self, colour: u32) -> Self {
        self.border_1().border_color(rgb(colour))
    }

    /// A 1px bottom rule in `colour`, such as `T.divider` between rows.
    fn border_b_as(self, colour: u32) -> Self {
        self.border_b_1().border_color(rgb(colour))
    }

    /// A 1px top rule in `colour`.
    fn border_t_as(self, colour: u32) -> Self {
        self.border_t_1().border_color(rgb(colour))
    }

    /// A 1px left rule in `colour`.
    fn border_l_as(self, colour: u32) -> Self {
        self.border_l_1().border_color(rgb(colour))
    }

    /// A 1px right rule in `colour`.
    fn border_r_as(self, colour: u32) -> Self {
        self.border_r_1().border_color(rgb(colour))
    }

    /// A fixed-width table column of `width` pixels that never grows or shrinks, so
    /// values stay in place as they update.
    fn col_w(self, width: f32) -> Self {
        self.w(px(width)).flex_none()
    }
}

impl<E: Styled> StyleExt for E {}
