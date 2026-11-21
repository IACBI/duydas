use std::fmt;
use std::time::Instant;

use duydas_i18n::{Key, Localizer, UiLocale, args};
use duydas_ui::tokens::{radius, space, text as type_scale};
use iced::advanced::input_method;
use iced::keyboard::{Key as KbKey, key::Named};
use iced::widget::{column, container, pick_list, row, rule, scrollable, text, text_editor};
use iced::{Element, Length, Subscription, Task, Theme, event, system, theme, window};

use crate::perf::Probe;

/// Placeholder channel until Phase 1 brings real communities. Channel names are user data, not UI
/// copy, so this is not a localization concern.
const DEMO_CHANNEL: &str = "general";

pub(crate) struct App {
    l10n: Localizer,
    dark: bool,
    composer: text_editor::Content,
    /// True while an IME composition is in progress. Enter must commit the composition, never send.
    composing: bool,
    sent: Vec<String>,
    perf: Option<Probe>,
}

#[derive(Debug, Clone)]
pub(crate) enum Message {
    LocaleSelected(LocaleOption),
    ThemeChanged(theme::Mode),
    Edit(text_editor::Action),
    Send,
    Ime(input_method::Event),
    Frame(Instant),
}

/// Wrapper so the picker shows each language by its own name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LocaleOption(UiLocale);

impl fmt::Display for LocaleOption {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0.autonym())
    }
}

impl App {
    pub(crate) fn new(perf: Option<Probe>) -> (Self, Task<Message>) {
        let app = Self {
            l10n: Localizer::system(),
            dark: true,
            composer: text_editor::Content::new(),
            composing: false,
            sent: Vec::new(),
            perf,
        };
        (app, system::theme().map(Message::ThemeChanged))
    }

    pub(crate) fn title(&self) -> String {
        self.l10n.tr(Key::AppName)
    }

    pub(crate) fn theme(&self) -> Theme {
        duydas_ui::theme(self.dark)
    }

    pub(crate) fn subscription(&self) -> Subscription<Message> {
        let ime = event::listen_with(|event, _status, _window| match event {
            iced::Event::InputMethod(ime) => Some(Message::Ime(ime)),
            _ => None,
        });
        let theme = system::theme_changes().map(Message::ThemeChanged);
        let frames = match self.perf {
            Some(probe) if probe.pending() => window::frames().map(Message::Frame),
            _ => Subscription::none(),
        };
        Subscription::batch([ime, theme, frames])
    }

    pub(crate) fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LocaleSelected(LocaleOption(locale)) => {
                // Rebuilding the localizer is the whole language switch: every view reads from it.
                self.l10n = Localizer::new(locale);
            }
            Message::ThemeChanged(mode) => {
                self.dark = !matches!(mode, theme::Mode::Light);
            }
            Message::Edit(action) => self.composer.perform(action),
            Message::Send => {
                let body = self.composer.text().trim_end().to_owned();
                if !body.trim().is_empty() {
                    self.sent.push(body);
                    self.composer = text_editor::Content::new();
                }
            }
            Message::Ime(event) => {
                self.composing = match event {
                    input_method::Event::Preedit(content, _) => !content.is_empty(),
                    input_method::Event::Opened
                    | input_method::Event::Commit(_)
                    | input_method::Event::Closed => false,
                };
            }
            Message::Frame(at) => {
                if let Some(probe) = &mut self.perf {
                    probe.first_frame(at);
                }
            }
        }
        Task::none()
    }

    pub(crate) fn view(&self) -> Element<'_, Message> {
        let l = &self.l10n;
        let sidebar = self.settings_panel();

        let header = column![
            text(l.tr(Key::WelcomeHeading)).size(type_scale::DISPLAY),
            text(l.tr(Key::AppTagline)).size(type_scale::TITLE),
            text(l.tr(Key::WelcomeBody)).size(type_scale::BODY),
        ]
        .spacing(space::SM);

        let count = i64::try_from(self.sent.len()).unwrap_or(i64::MAX);
        let history = scrollable(
            column(self.sent.iter().map(|m| {
                container(text(m.as_str()).size(type_scale::BODY))
                    .padding(space::SM)
                    .into()
            }))
            .spacing(space::XS),
        )
        .height(Length::Fill);

        let composing = self.composing;
        let composer = text_editor(&self.composer)
            .placeholder(l.tr_args(Key::ComposerPlaceholder, &args!(channel = DEMO_CHANNEL)))
            .on_action(Message::Edit)
            .height(Length::Shrink)
            .padding(space::MD)
            .key_binding(move |press| {
                let is_enter = matches!(press.key, KbKey::Named(Named::Enter));
                if is_enter && !press.modifiers.shift() && !composing {
                    Some(text_editor::Binding::Custom(Message::Send))
                } else {
                    text_editor::Binding::from_key_press(press)
                }
            })
            .style(|theme: &Theme, status| {
                let mut style = text_editor::default(theme, status);
                style.border.radius = radius::SM.into();
                style
            });

        let conversation = column![
            header,
            rule::horizontal(1),
            text(l.tr_args(Key::MessageCount, &args!(count = count))).size(type_scale::CAPTION),
            history,
            composer,
            text(l.tr(Key::ComposerHint)).size(type_scale::CAPTION),
        ]
        .spacing(space::MD)
        .padding(space::XL)
        .width(Length::Fill);

        row![sidebar, rule::vertical(1), conversation].into()
    }

    fn settings_panel(&self) -> Element<'_, Message> {
        let l = &self.l10n;
        let options: Vec<LocaleOption> = UiLocale::selectable()
            .into_iter()
            .map(LocaleOption)
            .collect();
        let picker = pick_list(
            options,
            Some(LocaleOption(l.ui_locale())),
            Message::LocaleSelected,
        );
        let sample_word = l.uppercase("istanbul");
        let date = l.format_date(2026, 11, 21).unwrap_or_default();

        column![
            text(l.tr(Key::SettingsTitle)).size(type_scale::TITLE),
            text(l.tr(Key::SettingsLanguage)).size(type_scale::CAPTION),
            picker,
            rule::horizontal(1),
            text(l.tr(Key::SettingsPreviewHeading)).size(type_scale::CAPTION),
            text(l.tr_args(
                Key::SettingsPreviewNumber,
                &args!(value = l.format_integer(1_234_567))
            )),
            text(l.tr_args(Key::SettingsPreviewDate, &args!(date = date))),
            text(l.tr_args(Key::SettingsPreviewUppercase, &args!(text = sample_word))),
        ]
        .spacing(space::SM)
        .padding(space::XL)
        .width(280)
        .into()
    }
}
