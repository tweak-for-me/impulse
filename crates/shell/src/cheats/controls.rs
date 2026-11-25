//! The control at the right of a cheat row, one per cheat kind, and the saved bar over the rows.

use client::{
    api::settle,
    library::GameId,
    trainer::{Sheet, Span, TOP_TICK},
};
use gpui_kit::{
    AnyElement, App, Context, Div, Entity, IntoElement, MouseButton, MouseMoveEvent, ScrollEvent, SharedString, Stateful, Window,
    component::{
        h_flex,
        input::{Input, InputEvent, InputState},
    },
    div,
    prelude::*,
    px, rgb,
};
use i18n::{Arg, Key};
use model::{Cheat, Kind};

use super::{CheatList, Menu, Printed, number, open_menu};
use crate::ui::{
    dropdown::{dropdown, trigger},
    key,
    text::{t, tf},
    theme::*,
    widgets::{button, flash_button, mono, switch, tip, toggle},
};

impl CheatList {
    fn set_value(&mut self, index: usize, value: f64, cx: &mut Context<Self>) {
        self.runs.update(cx, |runs, cx| runs.set(index, value, cx));
    }

    /// The cheat and its value in the live sheet, for the game a handler was drawn for: a render may be behind the
    /// pointer, so none once another game is open or its sheet has no such cheat.
    fn current<'a>(&self, game: &GameId, index: usize, cx: &'a App) -> Option<(&'a Cheat, f64)> {
        if self.library.read(cx).open_id() != Some(game) {
            return None;
        }
        let sheet = self.trainers.read(cx).sheet(game)?;
        Some((sheet.cheats.get(index)?, *sheet.values.get(index)?))
    }

    /// `set_value` to what `to` makes of the live value, but not for what the cheat already has: a drag inside one tick
    /// or a click at a limit sends nothing and makes no sound.
    fn change_value(&mut self, game: &GameId, index: usize, to: impl FnOnce(&Cheat, f64) -> f64, cx: &mut Context<Self>) {
        let Some((cheat, now)) = self.current(game, index, cx) else { return };
        let value = to(cheat, now);
        if value != now {
            self.set_value(index, value, cx);
        }
    }

    /// A number without a button, or a slider, takes its typed value on Enter or blur.
    pub(super) fn follow_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(printed) = self.printed.as_mut() else { return };
        let game = &printed.game;
        printed.typed = printed
            .inputs
            .iter()
            .map(|(&index, input)| {
                let game = game.clone();
                cx.subscribe_in(input, window, move |list, input, event: &InputEvent, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        list.typed(&game, index, input, window, cx);
                    }
                })
            })
            .collect();
    }

    /// The typed value within the cheat's limits, a slider's on a step; `Set` then shows it, else the field shows the
    /// value again.
    fn typed(&mut self, game: &GameId, index: usize, input: &Entity<InputState>, window: &mut Window, cx: &mut Context<Self>) {
        let running = self.is_running(cx);
        let Some((cheat, now)) = self.current(game, index, cx) else { return };
        if cheat.args.button {
            return;
        }
        // Against the value as the field would set it: a value a hotkey left between steps is no change.
        match field_value(input, cheat, cx) {
            Some(value) if running && Some(value) != settle(cheat, now) => self.set_value(index, value, cx),
            _ => input.update(cx, |input, cx| input.set_value(number(now), window, cx)),
        }
    }

    pub(super) fn control(
        &self,
        index: usize,
        sheet: &Sheet,
        printed: &Printed,
        running: bool,
        live: bool,
        cx: &Context<Self>,
    ) -> AnyElement {
        let cheat = &sheet.cheats[index];
        let value = sheet.values[index];
        let value_color = if live { rgb(accent(cx)) } else { rgb(DIM) };
        match cheat.kind {
            Kind::Toggle => switch(("toggle", index), live, cx)
                .on_click({
                    let game = printed.game.clone();
                    cx.listener(move |list, _, _, cx| {
                        // Inverts the live value: two clicks before a repaint must not send the same one twice.
                        if running {
                            list.change_value(&game, index, |_, now| if now == 0. { 1. } else { 0. }, cx);
                        }
                    })
                })
                .into_any_element(),
            Kind::Slider => h_flex()
                .gap(px(5.))
                .child(self.ticks(&printed.game, sheet, index, running, live, cx))
                .when_some(printed.inputs.get(&index), |control, input| control.child(field(input, running, 52.).text_color(value_color)))
                .into_any_element(),
            Kind::Number if cheat.args.button => h_flex()
                .gap(px(8.))
                .when_some(printed.inputs.get(&index), |control, input| control.child(field(input, running, 72.)))
                .child(self.press(&printed.game, index, key::CHEATS_SET, key::CHEATS_SET_DONE, running, cx))
                .into_any_element(),
            Kind::Number => {
                let step = cheat.args.stride();
                let nudge = |id: &'static str, label: &'static str, by: f64| {
                    let game = printed.game.clone();
                    button(label).id((id, index)).w(px(26.)).px_0().font_family(MONO).text_size(px(TYPE_LEAD)).on_click(cx.listener(
                        move |list, _, _, cx| {
                            if running {
                                list.change_value(&game, index, |cheat, now| cheat.args.clamp(now + by), cx);
                            }
                        },
                    ))
                };
                h_flex()
                    .gap(px(8.))
                    .child(nudge("decrease", "−", -step))
                    .when_some(printed.inputs.get(&index), |control, input| {
                        control.child(field(input, running, 72.).text_color(value_color))
                    })
                    .child(nudge("increase", "+", step))
                    .into_any_element()
            }
            Kind::Button => {
                self.press(&printed.game, index, key::CHEATS_EXECUTE, key::CHEATS_EXECUTE_DONE, running, cx).px(px(14.)).into_any_element()
            }
            Kind::Selection => {
                let options = printed.rows[index].options.clone();
                let current = value as usize;
                let width = px(112.);
                let trigger = trigger(("select", index), options.get(current).cloned().unwrap_or_default()).w(width);
                if !running {
                    return trigger.into_any_element();
                }
                let menu = Menu::Cheat(index);
                dropdown(trigger, menu, open_menu, self.menu == Some(menu), options, current, width, cx, move |list, option, _, cx| {
                    // The menu is dropped when another game opens, so the open game is the one it was drawn for.
                    if let Some(game) = list.selected(cx) {
                        list.change_value(&game, index, |_, _| option as f64, cx);
                    }
                })
                .into_any_element()
            }
            Kind::Unknown => div().into_any_element(),
        }
    }

    /// A button with no state of its own: it confirms the press for a moment.
    fn press(&self, game: &GameId, index: usize, label: Key, done: Key, running: bool, cx: &Context<Self>) -> Stateful<Div> {
        let game = game.clone();
        flash_button(&t(cx, label), &t(cx, done), self.is_flashed(index), cx).id(("press", index)).on_click(cx.listener(
            move |list, _, _, cx| {
                if !running {
                    return;
                }
                // A cheat with a text field takes its value from it; a button cheat sends its own and
                // keeps no state. A control drawn for another game does nothing.
                let Some((cheat, _)) = list.current(&game, index, cx) else { return };
                let input = list.printed.as_ref().filter(|printed| printed.game == game).and_then(|printed| printed.inputs.get(&index));
                let typed = input.and_then(|input| field_value(input, cheat, cx));
                let button = cheat.kind == Kind::Button;
                if let Some(value) = typed {
                    list.set_value(index, value, cx);
                }
                // A press flashes on `RunEvent::Pressed`, as a hotkey's does.
                if button {
                    list.runs.update(cx, |runs, cx| runs.press(index, cx));
                } else {
                    list.flash(index, cx);
                }
            },
        ))
    }

    /// Ten ticks: click or drag across them, or use the wheel.
    fn ticks(&self, game: &GameId, sheet: &Sheet, index: usize, running: bool, live: bool, cx: &Context<Self>) -> impl IntoElement {
        let (span, value) = (Span::of(&sheet.cheats[index].args), sheet.values[index]);
        let lit = if live { accent(cx) } else { TICK_IDLE };
        let level = span.tick(value);
        let wheel = game.clone();
        h_flex()
            .when(running, |ticks| {
                ticks.cursor_pointer().on_scroll(cx.listener(move |list, event: &ScrollEvent, window, cx| {
                    let up = event.delta.pixel_delta(window.line_height()).y > px(0.);
                    // From the live value: two notches before a render each move the slider.
                    list.change_value(&wheel, index, |_, now| span.nudge(now, up), cx);
                    // Keeps the pane from scrolling under the slider.
                    cx.stop_propagation();
                }))
            })
            .children((0..=TOP_TICK).map(|tick| {
                let (pressed, dragged) = (game.clone(), game.clone());
                // The 3 px gap belongs to the cell, so a drag never falls between ticks.
                div()
                    .pr(px(3.))
                    .when(running, |cell| {
                        cell.on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |list, _, _, cx| {
                                list.change_value(&pressed, index, |_, _| span.value(tick), cx);
                            }),
                        )
                        .on_mouse_move(cx.listener(move |list, event: &MouseMoveEvent, _, cx| {
                            if event.pressed_button == Some(MouseButton::Left) {
                                list.change_value(&dragged, index, |_, _| span.value(tick), cx);
                            }
                        }))
                    })
                    .child(div().w(px(6.)).h(px(15.)).bg(rgb(if tick <= level { lit } else { TICK })))
            }))
    }

    fn reset_mods(&mut self, cx: &mut Context<Self>) {
        let Some(game) = self.selected(cx) else { return };
        self.runs.update(cx, |runs, cx| runs.reset(&game, cx));
    }

    /// The right of the filter row: the game's save switch and the reset.
    pub(super) fn saved_bar(&self, game: &GameId, sheet: &Sheet, running: bool, cx: &Context<Self>) -> impl IntoElement {
        let profile = self.trainers.read(cx).trainer_profile(game);
        let (save, kept) = (profile.save_mods, profile.values.len());
        let resettable = if running { sheet.active() > 0 } else { kept > 0 };
        let label = match kept {
            0 => t(cx, key::CHEATS_SAVE),
            _ if running => t(cx, key::CHEATS_SAVE),
            kept => {
                let padded = format!("{kept:02}");
                tf(cx, key::CHEATS_SAVED, &[("saved", Arg::Str(&padded)), ("count", Arg::Num(kept as f64))])
            }
        };
        let save_tip = t(cx, key::CHEATS_SAVE_TIP);
        h_flex()
            .flex_none()
            .gap(px(10.))
            .child(div().w(px(1.)).h(px(16.)).bg(line(0.12)))
            .child(
                h_flex()
                    .id("save-mods")
                    .gap(px(8.))
                    .cursor_pointer()
                    .tooltip(move |window, cx| tip(save_tip.clone(), window, cx))
                    .on_click(cx.listener(move |list, _, _, cx| {
                        let Some(game) = list.selected(cx) else { return };
                        list.trainers.update(cx, |trainers, cx| trainers.set_save_mods(&game, !save, cx));
                    }))
                    .child(mono(label, TYPE_CAPTION).text_color(rgb(if save { MUTED } else { GHOST })))
                    .child(toggle(save, cx)),
            )
            .child(
                button(&t(cx, key::CHEATS_RESET))
                    .id("reset-mods")
                    .h(px(24.))
                    .when(!resettable, |button| button.opacity(0.4).cursor_default())
                    .when(resettable, |button| button.on_click(cx.listener(|list, _, _, cx| list.reset_mods(cx)))),
            )
    }
}

/// What the typed text sets: a number within its limits, a slider's value on one of its steps.
fn field_value(input: &Entity<InputState>, cheat: &Cheat, cx: &App) -> Option<f64> {
    settle(cheat, input.read(cx).value().trim().parse().ok()?)
}

fn field(input: &Entity<InputState>, running: bool, width: f32) -> Div {
    value_box("")
        .w(px(width))
        .child(Input::new(input).appearance(false).disabled(!running).px(px(8.)).text_center().font_family(MONO).text_size(px(TYPE_FIELD)))
}

fn value_box(value: impl Into<SharedString>) -> Div {
    mono(value, TYPE_FIELD).h(px(26.)).flex().items_center().border_1().border_color(line(0.14)).bg(rgb(SUNKEN))
}
