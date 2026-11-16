//! Draw-only micro-interactions. No application messages, timers or per-frame relayout.
use super::Msg;
use iced::advanced::{Clipboard, Layout, Shell, Widget, layout, mouse, overlay, renderer, widget};
use iced::time::{Duration, Instant};
use iced::{
    Animation, Element, Event, Length, Rectangle, Renderer, Size, Theme, Transformation, Vector,
};
use std::sync::LazyLock;

static REDUCED: LazyLock<bool> = LazyLock::new(|| {
    std::env::var("EORZEA_REDUCED_MOTION").is_ok_and(|value| matches!(value.as_str(), "1" | "true"))
});
const FRAME: Duration = Duration::from_millis(16);

struct Transition {
    animation: Animation<bool>,
    now: Instant,
    reduced: bool,
}
impl Transition {
    fn new(open: bool, duration: u64, reduced: bool) -> Self {
        Self {
            animation: Animation::new(open)
                .duration(Duration::from_millis(duration))
                .easing(iced::animation::Easing::EaseOut),
            now: Instant::now(),
            reduced,
        }
    }
    fn target(&self) -> bool {
        self.animation.value()
    }
    fn update(&mut self, target: Option<bool>, now: Instant) {
        self.now = now.max(self.now);
        if let Some(target) = target {
            if self.animation.value() != target {
                if self.reduced {
                    self.animation = Animation::new(target);
                } else {
                    self.animation.go_mut(target, self.now);
                }
            }
        }
    }
    fn animating(&self) -> bool {
        !self.reduced && self.animation.is_animating(self.now)
    }
    fn amount(&self) -> f32 {
        if self.reduced {
            if self.target() { 1. } else { 0. }
        } else {
            self.animation.interpolate(0., 1., self.now).clamp(0., 1.)
        }
    }
}

#[derive(Clone, Copy)]
enum Effect {
    Press(bool),
    Reveal { open: bool, enabled: bool },
    ReadOnly,
}
struct Motion<'a> {
    content: Element<'a, Msg>,
    effect: Effect,
}
pub fn press<'a>(content: impl Into<Element<'a, Msg>>, enabled: bool) -> Element<'a, Msg> {
    Element::new(Motion {
        content: content.into(),
        effect: Effect::Press(enabled),
    })
}
pub fn reveal<'a>(
    content: impl Into<Element<'a, Msg>>,
    open: bool,
    enabled: bool,
) -> Element<'a, Msg> {
    Element::new(Motion {
        content: content.into(),
        effect: Effect::Reveal { open, enabled },
    })
}
/// Parent must reject edit actions. Suppress IME and read-only caret blink redraws.
pub fn read_only<'a>(content: impl Into<Element<'a, Msg>>) -> Element<'a, Msg> {
    Element::new(Motion {
        content: content.into(),
        effect: Effect::ReadOnly,
    })
}
impl Motion<'_> {
    fn interactive(&self, state: &Transition) -> bool {
        match self.effect {
            Effect::Press(_) | Effect::ReadOnly => true,
            Effect::Reveal { open, enabled } => open && state.amount() >= 1. && enabled,
        }
    }
    fn visible_bounds(&self, state: &Transition, bounds: Rectangle) -> Rectangle {
        match self.effect {
            Effect::Press(_) | Effect::ReadOnly => bounds,
            Effect::Reveal { .. } => Rectangle {
                y: bounds.y + bounds.height * (1. - state.amount()),
                height: bounds.height * state.amount(),
                ..bounds
            },
        }
    }
}
impl Widget<Msg, Theme, Renderer> for Motion<'_> {
    fn tag(&self) -> widget::tree::Tag {
        widget::tree::Tag::of::<Transition>()
    }
    fn state(&self) -> widget::tree::State {
        let (open, duration) = match self.effect {
            Effect::Press(_) => (false, 100),
            Effect::Reveal { open, .. } => (open, 200),
            Effect::ReadOnly => (false, 100),
        };
        widget::tree::State::new(Transition::new(open, duration, *REDUCED))
    }
    fn children(&self) -> Vec<widget::Tree> {
        vec![widget::Tree::new(self.content.as_widget())]
    }
    fn diff(&self, tree: &mut widget::Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }
    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }
    fn layout(
        &mut self,
        tree: &mut widget::Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }
    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Msg>,
        viewport: &Rectangle,
    ) {
        if matches!(self.effect, Effect::ReadOnly) {
            let mut messages = Vec::new();
            let mut local = Shell::new(&mut messages);
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                &mut local,
                viewport,
            );
            *local.input_method_mut() = iced::advanced::InputMethod::Disabled;
            if matches!(
                event,
                Event::Window(iced::window::Event::RedrawRequested(_))
            ) {
                Shell::replace_redraw_request(&mut local, iced::window::RedrawRequest::Wait);
            }
            shell.merge(local, |message| message);
            return;
        }
        let state = tree.state.downcast_mut::<Transition>();
        let now = match event {
            Event::Window(iced::window::Event::RedrawRequested(at)) => *at,
            _ => Instant::now(),
        };
        let target = match self.effect {
            Effect::Reveal { open, .. } => Some(open),
            Effect::ReadOnly => None,
            Effect::Press(enabled) => match event {
                Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                    if enabled && cursor.is_over(layout.bounds()) =>
                {
                    Some(true)
                }
                Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                | Event::Window(iced::window::Event::Unfocused) => Some(false),
                Event::Mouse(mouse::Event::CursorMoved { .. })
                    if !cursor.is_over(layout.bounds()) =>
                {
                    Some(false)
                }
                _ if !enabled => Some(false),
                _ => None,
            },
        };
        state.update(target, now);
        if state.animating() {
            shell.request_redraw_at(state.now + FRAME);
        }
        if self.interactive(state) {
            self.content.as_widget_mut().update(
                &mut tree.children[0],
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        } else if cursor.is_over(self.visible_bounds(state, layout.bounds()))
            && matches!(event, Event::Mouse(_))
        {
            shell.capture_event();
        }
    }
    fn draw(
        &self,
        tree: &widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let state = tree.state.downcast_ref::<Transition>();
        if matches!(self.effect, Effect::Reveal { .. }) && state.amount() <= 0. {
            return;
        }
        let bounds = layout.bounds();
        let transformation = match self.effect {
            Effect::Press(_) => {
                let scale = 1. - state.amount() * 0.015;
                let center = bounds.center();
                Transformation::translate(center.x, center.y)
                    * Transformation::scale(scale)
                    * Transformation::translate(-center.x, -center.y)
            }
            Effect::Reveal { .. } => {
                Transformation::translate(0., bounds.height * (1. - state.amount()))
            }
            Effect::ReadOnly => Transformation::IDENTITY,
        };
        if let Some(clip) = bounds.intersection(viewport) {
            // Nested layers are transformed but do not inherit their parent's clip.
            // Keep this stationary boundary in the child's coordinate space as well.
            let child_viewport = clip * transformation.inverse();
            renderer.with_layer(clip, |renderer| {
                renderer.with_transformation(transformation, |renderer| {
                    self.content.as_widget().draw(
                        &tree.children[0],
                        renderer,
                        theme,
                        style,
                        layout,
                        if self.interactive(state) {
                            cursor
                        } else {
                            mouse::Cursor::Unavailable
                        },
                        &child_viewport,
                    );
                })
            });
        }
    }
    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let state = tree.state.downcast_ref::<Transition>();
        if self.interactive(state) {
            self.content.as_widget().mouse_interaction(
                &tree.children[0],
                layout,
                cursor,
                viewport,
                renderer,
            )
        } else if cursor.is_over(self.visible_bounds(state, layout.bounds())) {
            mouse::Interaction::Idle
        } else {
            mouse::Interaction::None
        }
    }
    fn operate(
        &mut self,
        tree: &mut widget::Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        if self.interactive(tree.state.downcast_ref::<Transition>()) {
            self.content.as_widget_mut().operate(
                &mut tree.children[0],
                layout,
                renderer,
                operation,
            );
        }
    }
    fn overlay<'a>(
        &'a mut self,
        tree: &'a mut widget::Tree,
        layout: Layout<'a>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'a, Msg, Theme, Renderer>> {
        if self.interactive(tree.state.downcast_ref::<Transition>()) {
            self.content.as_widget_mut().overlay(
                &mut tree.children[0],
                layout,
                renderer,
                viewport,
                translation,
            )
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transition_stops_and_can_reverse_without_jumping() {
        let mut transition = Transition::new(false, 200, false);
        let start = Instant::now();
        assert!(!transition.animating());
        transition.update(Some(true), start);
        transition.update(None, start + Duration::from_millis(80));
        let middle = transition.amount();
        assert!(middle > 0. && middle < 1.);
        transition.update(Some(false), start + Duration::from_millis(80));
        assert!((transition.amount() - middle).abs() < 0.001);
        transition.update(None, start + Duration::from_millis(500));
        assert!(!transition.animating());
        assert_eq!(transition.amount(), 0.);
    }
    #[test]
    fn reduced_motion_never_starts_an_animation() {
        let mut transition = Transition::new(false, 200, true);
        transition.update(Some(true), Instant::now());
        assert_eq!(transition.amount(), 1.);
        assert!(!transition.animating());
        transition.update(Some(false), Instant::now());
        assert_eq!(transition.amount(), 0.);
    }
    #[test]
    fn widget_motion_requests_only_temporary_draws_not_layout_or_messages() {
        let renderer = Renderer::new(iced::Font::DEFAULT, 14.into());
        let mut element = press(iced::widget::button("test").on_press(Msg::Copy), true);
        let mut tree = widget::Tree::new(element.as_widget());
        let node = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(100., 40.)),
        );
        let cursor = mouse::Cursor::Available(node.bounds().center());
        let viewport = Rectangle::with_size(Size::new(100., 40.));
        let mut messages = Vec::new();
        {
            let mut shell = Shell::new(&mut messages);
            element.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                Layout::new(&node),
                cursor,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &viewport,
            );
            assert!(!shell.is_layout_invalid());
            assert!(!shell.are_widgets_invalid());
            if !*REDUCED {
                assert_ne!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            }
        }
        let start = tree.state.downcast_ref::<Transition>().now;
        let mut shell = Shell::new(&mut messages);
        element.as_widget_mut().update(
            &mut tree,
            &Event::Window(iced::window::Event::RedrawRequested(
                start + Duration::from_millis(500),
            )),
            Layout::new(&node),
            cursor,
            &renderer,
            &mut iced::advanced::clipboard::Null,
            &mut shell,
            &viewport,
        );
        assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
        assert!(!shell.is_layout_invalid());
        assert!(!shell.are_widgets_invalid());
        assert!(messages.is_empty());
    }
    #[test]
    fn drawer_uses_cached_layout_blocks_partial_hits_and_stops_redrawing() {
        let renderer = Renderer::new(iced::Font::DEFAULT, 14.into());
        let content = || {
            iced::widget::button("test")
                .width(200)
                .height(200)
                .on_press(Msg::Copy)
        };
        let mut closed = reveal(content(), false, true);
        let mut tree = widget::Tree::new(closed.as_widget());
        let node = closed.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, Size::new(200., 200.)),
        );
        let viewport = Rectangle::with_size(Size::new(200., 200.));
        let mut messages = Vec::new();
        let start = Instant::now();
        let mut opened = reveal(content(), true, true);
        tree.diff(opened.as_widget());
        for offset in [0, 80, 250, 500] {
            let at = start + Duration::from_millis(offset);
            let mut shell = Shell::new(&mut messages);
            opened.as_widget_mut().update(
                &mut tree,
                &Event::Window(iced::window::Event::RedrawRequested(at)),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut shell,
                &viewport,
            );
            assert!(!shell.is_layout_invalid());
            assert!(!shell.are_widgets_invalid());
            if offset >= 250 {
                assert_eq!(shell.redraw_request(), iced::window::RedrawRequest::Wait);
            }
            if offset == 80 && !*REDUCED {
                let state = tree.state.downcast_ref::<Transition>();
                assert!(state.amount() > 0. && state.amount() < 1.);
                let mut shell = Shell::new(&mut messages);
                opened.as_widget_mut().update(
                    &mut tree,
                    &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
                    Layout::new(&node),
                    mouse::Cursor::Available(iced::Point::new(100., 195.)),
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
                assert!(shell.is_event_captured());
                assert!(messages.is_empty());
            }
        }
        assert!(messages.is_empty());
    }
}
