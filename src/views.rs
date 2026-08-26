//! Rendering of the app's screens: the lock/PIN screen, the wallet header,
//! the entry form, the ledger table, and the settings window.
//!
//! These methods all belong to the same `impl CofferlyApp` block declared in
//! `main.rs`. Field privacy is unaffected because the block is in the same
//! crate as the struct definition.

use eframe::egui;

use crate::data::{valid_child_name, LedgerRowDate, LedgerSort};
use crate::money::format_money;
use crate::money::format_money_input;
use crate::theme;
use crate::theme::amount_color;
use crate::theme::balance_color;
use crate::{CofferlyApp, LockMode};
use crate::{StatusSeverity, APP_NAME, APP_VERSION, PIN_LENGTH};

impl CofferlyApp {
    pub fn lock_screen(&mut self, ui: &mut egui::Ui) {
        if self.lock_mode != LockMode::LegacyPin {
            self.story_lock_screen(ui);
            return;
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::default().fill(self.lock_screen_bg))
            .show(ui, |ui| {
                let viewport_height = ui.available_height();
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.vertical_centered(|ui| {
                            // Preserve enough room for the privacy and first-run guidance at
                            // common laptop window heights while retaining breathing room on
                            // larger displays.
                            let compact = viewport_height < 800.0;
                            ui.add_space(if compact {
                                12.0
                            } else {
                                (viewport_height * 0.03).clamp(18.0, 30.0)
                            });

                            if let Some(texture) = &self.lock_screen_image {
                                let art_max_width = if compact { 250.0 } else { 340.0 };
                                let max_width =
                                    (ui.available_width() * 0.5).clamp(200.0, art_max_width);
                                let aspect = 260.0 / 146.0;
                                let size = egui::vec2(max_width, max_width / aspect);
                                ui.add(egui::Image::new(texture).fit_to_exact_size(size));
                            }
                            ui.add_space(8.0);

                            ui.label(
                                egui::RichText::new(APP_NAME)
                                    .size(40.0)
                                    .strong()
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.label(
                                egui::RichText::new("A simple, private allowance wallet")
                                    .size(18.0)
                                    .color(theme::LOCK_TEXT_SECONDARY),
                            );
                            ui.add_space(22.0);

                            egui::Frame::new()
                                .fill(theme::CARD_BG)
                                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                                .corner_radius(egui::CornerRadius::same(14))
                                .inner_margin(egui::Margin::symmetric(28, 22))
                                .show(ui, |ui| {
                                    ui.set_max_width(460.0);
                                    ui.set_min_width((ui.available_width() * 0.7).min(460.0));
                                    ui.label(
                                        egui::RichText::new("Welcome back")
                                            .size(19.0)
                                            .strong()
                                            .color(theme::TEXT_PRIMARY),
                                    );
                                    ui.label(
                                        egui::RichText::new(
                                            "Enter the 4-digit parent PIN to continue",
                                        )
                                        .size(13.0)
                                        .color(theme::LOCK_TEXT_SECONDARY),
                                    );
                                    ui.add_space(16.0);

                                    let cooldown_remaining = self.unlock_cooldown_remaining();
                                    if let Some(remaining) = cooldown_remaining {
                                        ui.ctx().request_repaint_after(
                                            remaining.min(std::time::Duration::from_secs(1)),
                                        );
                                    }
                                    let pin_entry_enabled =
                                        !self.unlocking && cooldown_remaining.is_none();

                                    if pin_entry_enabled {
                                        if let Some(index) = self.pending_pin_focus.take() {
                                            ui.memory_mut(|memory| {
                                                memory.request_focus(crate::pin_digit_id(index))
                                            });
                                        }
                                    } else {
                                        self.pending_pin_focus = Some(0);
                                    }

                                    let enter_pressed =
                                        ui.input(|input| input.key_pressed(egui::Key::Enter));
                                    let mut pin_changed = false;

                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 14.0;
                                        let pin_entry_width = 4.0 * 58.0 + 3.0 * 14.0;
                                        ui.add_space(
                                            ((ui.available_width() - pin_entry_width) / 2.0)
                                                .max(0.0),
                                        );

                                        for index in 0..PIN_LENGTH {
                                            let (coin_rect, _) = ui.allocate_exact_size(
                                                egui::vec2(58.0, 58.0),
                                                egui::Sense::hover(),
                                            );
                                            let response = ui.put(
                                                coin_rect,
                                                egui::TextEdit::singleline(
                                                    &mut self.pin_digits[index],
                                                )
                                                .id(crate::pin_digit_id(index))
                                                .password(true)
                                                .frame(egui::Frame::NONE)
                                                .background_color(egui::Color32::TRANSPARENT)
                                                .text_color(egui::Color32::TRANSPARENT)
                                                .horizontal_align(egui::Align::Center)
                                                .vertical_align(egui::Align::Center)
                                                .char_limit(PIN_LENGTH)
                                                .desired_width(54.0)
                                                .interactive(pin_entry_enabled),
                                            );

                                            if response.changed() {
                                                self.normalize_pin_digit_input(index);
                                                pin_changed = true;
                                                ui.ctx().request_repaint();
                                            }

                                            if response.has_focus()
                                                && self.pin_digits[index].is_empty()
                                                && ui.input(|input| {
                                                    input.key_pressed(egui::Key::Backspace)
                                                })
                                                && index > 0
                                            {
                                                self.pending_pin_focus = Some(index - 1);
                                                ui.ctx().request_repaint();
                                            }

                                            draw_pin_coin(
                                                ui,
                                                index,
                                                coin_rect,
                                                !self.pin_digits[index].is_empty(),
                                                response.has_focus(),
                                            );
                                        }
                                    });

                                    ui.add_space(4.0);
                                    let pin_guidance = cooldown_remaining
                                        .map(|remaining| {
                                            format!(
                                                "Try again in {}",
                                                crate::format_cooldown(remaining)
                                            )
                                        })
                                        .unwrap_or_else(|| {
                                            "Gold coins fill as you type".to_owned()
                                        });
                                    ui.label(
                                        egui::RichText::new(pin_guidance)
                                            .size(12.0)
                                            .strong()
                                            .color(theme::LOCK_TEXT_SECONDARY),
                                    );

                                    // Auto-submit as soon as the 4th digit lands (ATM / phone
                                    // lock convention). Enter and the Unlock button still work
                                    // for paste / partial flows.
                                    let should_unlock = pin_entry_enabled
                                        && self.parent_pin_complete()
                                        && (pin_changed || enter_pressed);

                                    if should_unlock {
                                        self.start_unlock();
                                    }

                                    ui.add_space(16.0);

                                    let unlock_enabled = pin_entry_enabled;
                                    let unlock_label = if self.unlocking {
                                        "Unlocking…"
                                    } else if cooldown_remaining.is_some() {
                                        "Please wait…"
                                    } else {
                                        "Unlock"
                                    };
                                    if ui
                                        .add_enabled(
                                            unlock_enabled,
                                            egui::Button::new(
                                                egui::RichText::new(unlock_label)
                                                    .size(15.0)
                                                    .color(egui::Color32::WHITE)
                                                    .strong(),
                                            )
                                            .fill(theme::ACCENT_DARK)
                                            .min_size(egui::vec2(240.0, 42.0)),
                                        )
                                        .clicked()
                                    {
                                        self.start_unlock();
                                    }
                                });

                            ui.add_space(14.0);
                            let status_color = match self.status.severity {
                                StatusSeverity::Error => theme::NEGATIVE,
                                StatusSeverity::Success => theme::POSITIVE,
                                StatusSeverity::Info => theme::LOCK_TEXT_SECONDARY,
                            };
                            let status_text = if self.status.severity == StatusSeverity::Error {
                                format!("⚠ {}", self.status.text)
                            } else {
                                self.status.text.clone()
                            };
                            ui.label(
                                egui::RichText::new(status_text)
                                    .size(13.0)
                                    .color(status_color),
                            );
                            ui.add_space(8.0);
                            ui.label(
                                egui::RichText::new("Local-only  •  No account  •  No cloud sync")
                                    .size(13.0)
                                    .color(theme::LOCK_TEXT_SECONDARY),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Legacy file? Enter its existing PIN, then enroll a Coffer Story.",
                                )
                                .size(12.0)
                                .color(theme::LOCK_TEXT_SECONDARY),
                            );
                        });
                    });
            });
    }

    fn story_lock_screen(&mut self, ui: &mut egui::Ui) {
        egui::CentralPanel::default().frame(egui::Frame::default().fill(self.lock_screen_bg)).show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                if let Some(texture) = &self.lock_screen_image {
                    let art_width = 260.0_f32.min(ui.available_width());
                    // The source artwork has a blank strip below the chest. Crop it
                    // from the display so the title and chooser sit closer together.
                    let size = egui::vec2(art_width, art_width * 306.0 / 640.0);
                    ui.add(
                        egui::Image::new(texture)
                            .fit_to_exact_size(size)
                            .uv(egui::Rect::from_min_max(
                                egui::Pos2::ZERO,
                                egui::pos2(1.0, 306.0 / 360.0),
                            ))
                            .alt_text("A closed treasure coffer"),
                    );
                    ui.add_space(6.0);
                }
                ui.label(egui::RichText::new(APP_NAME).size(38.0).strong().color(theme::TEXT_PRIMARY));
                ui.label(egui::RichText::new("A simple, private allowance wallet").size(16.0).color(theme::LOCK_TEXT_SECONDARY));
                ui.add_space(14.0);
                // This is a desktop-first screen: keep the chooser on a deliberate,
                // stable column instead of letting the frame span the whole window.
                egui::Frame::new().fill(theme::CARD_BG).stroke(egui::Stroke::new(1.0, theme::BORDER)).corner_radius(egui::CornerRadius::same(14)).inner_margin(egui::Margin::same(20)).show(ui, |ui| {
                    ui.set_width(540.0);
                    let is_reveal = matches!(self.lock_mode, LockMode::SetupReveal | LockMode::MigrateReveal | LockMode::ChangeReveal);
                    let is_migration = matches!(self.lock_mode, LockMode::MigrateReveal | LockMode::MigrateConfirm);
                    let is_change = matches!(self.lock_mode, LockMode::ChangeReveal | LockMode::ChangeConfirm);
                    let heading = if is_reveal { if is_migration { "Move to Coffer Story" } else if is_change { "Your replacement Coffer Story" } else { "Your new Coffer Story" } } else if matches!(self.lock_mode, LockMode::SetupConfirm | LockMode::MigrateConfirm | LockMode::ChangeConfirm) { "Confirm your Coffer Story" } else { "Welcome back" };
                    ui.label(egui::RichText::new(heading).size(20.0).strong().color(theme::TEXT_PRIMARY));
                    if is_reveal {
                        ui.label(egui::RichText::new("Cofferly generated this six-object key. Keep it private — it unlocks your encrypted ledger.").size(13.0).color(theme::LOCK_TEXT_SECONDARY));
                        ui.add_space(12.0);
                        if let Some(story) = self.pending_story {
                            ui.horizontal_wrapped(|ui| for (index, id) in story.iter().enumerate() { ui.group(|ui| { ui.label(egui::RichText::new(format!("{}", index + 1)).strong().color(theme::GOLD_DARK)); ui.label(egui::RichText::new(crate::story::label(id).unwrap_or(id)).size(16.0).strong().color(theme::TEXT_PRIMARY)); }); });
                        }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new("Write or print this recovery key and store it away from the computer. Without it, the encrypted ledger cannot be recovered.").size(12.0).color(theme::NEGATIVE));
                        ui.horizontal(|ui| {
                            if ui.button("Generate another story").clicked() { self.regenerate_story(); }
                            if ui.button("Print recovery card").clicked() { self.print_recovery_card(); }
                            if ui.add(egui::Button::new(egui::RichText::new("I wrote it down — continue").strong().color(egui::Color32::WHITE)).fill(theme::ACCENT_DARK)).clicked() { self.lock_mode = if is_migration { LockMode::MigrateConfirm } else if is_change { LockMode::ChangeConfirm } else { LockMode::SetupConfirm }; self.reset_story_entry(); }
                            if is_migration && ui.button("Cancel migration").clicked() { self.cancel_story_migration(); }
                            if is_change && ui.button("Cancel").clicked() { self.cancel_story_change(); }
                        });
                    } else {
                        let cooldown = self.unlock_cooldown_remaining();
                        if let Some(remaining) = cooldown { ui.ctx().request_repaint_after(remaining.min(std::time::Duration::from_secs(1))); ui.label(egui::RichText::new(format!("Try again in {}", crate::format_cooldown(remaining))).color(theme::NEGATIVE)); }
                        else { ui.label(egui::RichText::new("Choose the six objects in order. The grid reshuffles each time; Cofferly never reveals partial correctness.").size(13.0).color(theme::LOCK_TEXT_SECONDARY)); }
                        ui.add_space(8.0);
                        // `horizontal_centered` expands to the remaining height in egui.
                        // The desktop card is fixed-width, so center this known-width row
                        // explicitly without creating another expanding region.
                        let progress_width = 190.0;
                        let progress_inset = ((ui.available_width() - progress_width) / 2.0).max(0.0);
                        ui.horizontal(|ui| {
                            ui.add_space(progress_inset);
                            // Painted directly (not `ui.label`) so these decorative
                            // dots don't reach the accessibility tree as separate
                            // widgets — "N of 6 selected" below is the readable count.
                            let (dots_rect, _) = ui.allocate_exact_size(
                                egui::vec2(progress_width, 30.0),
                                egui::Sense::hover(),
                            );
                            let dot_width = progress_width / crate::story::STORY_LENGTH as f32;
                            for index in 0..crate::story::STORY_LENGTH {
                                let (mark, color) = if index < self.story_selections.len() {
                                    ("●", theme::GOLD_DARK)
                                } else {
                                    ("○", theme::TEXT_SECONDARY)
                                };
                                let center = egui::pos2(
                                    dots_rect.left() + dot_width * (index as f32 + 0.5),
                                    dots_rect.center().y,
                                );
                                ui.painter().text(
                                    center,
                                    egui::Align2::CENTER_CENTER,
                                    mark,
                                    egui::FontId::proportional(25.0),
                                    color,
                                );
                            }
                        });
                        ui.label(
                            egui::RichText::new(format!(
                                "{} of {} selected",
                                self.story_selections.len(),
                                crate::story::STORY_LENGTH
                            ))
                            .size(12.0)
                            .color(theme::TEXT_SECONDARY),
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Clear").clicked() {
                                self.reset_story_entry();
                            }
                            if ui
                                .add_enabled(
                                    !self.story_selections.is_empty(),
                                    egui::Button::new("Remove last"),
                                )
                                .clicked()
                            {
                                self.remove_last_story_selection();
                            }
                            match self.lock_mode {
                                LockMode::SetupConfirm => {
                                    if ui.button("Back").clicked() {
                                        self.back_to_story_reveal();
                                    }
                                }
                                LockMode::MigrateConfirm => {
                                    if ui.button("Back").clicked() {
                                        self.back_to_story_reveal();
                                    }
                                    if ui.button("Cancel migration").clicked() {
                                        self.cancel_story_migration();
                                    }
                                }
                                LockMode::ChangeConfirm => {
                                    if ui.button("Back").clicked() {
                                        self.back_to_story_reveal();
                                    }
                                    if ui.button("Cancel").clicked() {
                                        self.cancel_story_change();
                                    }
                                }
                                _ => {}
                            }
                        });
                        if ui.input(|input| input.key_pressed(egui::Key::Backspace)) {
                            self.remove_last_story_selection();
                        }
                        ui.add_space(8.0);
                        egui::Grid::new("story_objects").num_columns(6).spacing([8.0, 8.0]).show(ui, |ui| {
                            // `&'static str` is Copy, so indexing here is a cheap
                            // copy per tile rather than cloning the whole Vec.
                            for index in 0..self.display_order.len() {
                                let id = self.display_order[index];
                                let enabled = cooldown.is_none() && !self.unlocking;
                                let (rect, response) = ui.allocate_exact_size(
                                    egui::vec2(82.0, 64.0),
                                    egui::Sense::click(),
                                );
                                let label = crate::story::label(id).unwrap_or(id);
                                let (fill, stroke, text_color) = if !enabled {
                                    (theme::FAINT_BG, theme::BORDER, theme::TEXT_SECONDARY)
                                } else if response.hovered() {
                                    (theme::ACCENT_LIGHT, theme::ACCENT, theme::ACCENT_DARK)
                                } else {
                                    (theme::CARD_BG, theme::BORDER, theme::TEXT_PRIMARY)
                                };
                                ui.painter().rect(
                                    rect,
                                    egui::CornerRadius::same(8),
                                    fill,
                                    egui::Stroke::new(1.0, stroke),
                                    egui::StrokeKind::Inside,
                                );
                                if let Some(texture) = self.story_icon_textures.get(id) {
                                    draw_story_icon(ui, texture, response.rect);
                                }
                                ui.painter().text(
                                    rect.center_bottom() - egui::vec2(0.0, 6.0),
                                    egui::Align2::CENTER_BOTTOM,
                                    label,
                                    egui::FontId::proportional(11.0),
                                    text_color,
                                );
                                let accessible_label = format!("Coffer Story object: {label}");
                                response.widget_info(|| {
                                    egui::WidgetInfo::labeled(
                                        egui::WidgetType::Button,
                                        enabled,
                                        accessible_label.clone(),
                                    )
                                });
                                response.clone().on_hover_text(accessible_label);
                                if enabled && response.clicked() {
                                    self.select_story_object(id);
                                }
                                if index % 6 == 5 { ui.end_row(); }
                            }
                        });
                    }
                });
                ui.add_space(10.0);
                ui.label(egui::RichText::new(&self.status.text).size(13.0).color(match self.status.severity { StatusSeverity::Error => theme::NEGATIVE, StatusSeverity::Success => theme::POSITIVE, StatusSeverity::Info => theme::LOCK_TEXT_SECONDARY }));
                ui.label(egui::RichText::new("Local-only  •  No account  •  No cloud sync").size(12.0).color(theme::LOCK_TEXT_SECONDARY));
            }));
        });
    }

    pub fn wallet_header(&mut self, ui: &mut egui::Ui) {
        let wallet = self.selected_wallet();
        let name = wallet.child_name.clone();
        let balance = wallet.current_balance_cents();

        ui.columns(3, |columns| {
            columns[0].add_space(31.0);
            columns[0].vertical(|ui| {
                ui.label(
                    egui::RichText::new(&name)
                        .size(26.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Available balance")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });

            columns[1].with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                if let Some(texture) = &self.open_coffer_image {
                    egui::Frame::new()
                        .fill(theme::APP_BG)
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(egui::Margin::symmetric(10, 4))
                        .show(ui, |ui| {
                            ui.add(
                                egui::Image::new(texture)
                                    .fit_to_exact_size(egui::vec2(142.0, 120.0))
                                    .texture_options(egui::TextureOptions::NEAREST)
                                    .alt_text("An open treasure chest overflowing with gold coins"),
                            );
                        });
                }
            });

            columns[2].add_space(27.0);
            columns[2].with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                ui.label(
                    egui::RichText::new(format_money(balance))
                        .size(32.0)
                        .strong()
                        .color(balance_color(balance)),
                );
                ui.label(
                    egui::RichText::new("Ready to save or spend")
                        .size(11.0)
                        .color(theme::TEXT_SECONDARY),
                );
            });
        });
    }

    pub fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }

        let selected_name = self.selected_wallet().child_name.clone();
        let starting_balance = self.selected_wallet().starting_balance_cents;
        let has_entries = !self.selected_wallet().entries.is_empty();
        let can_delete_wallet = self.data.wallets.len() > 1;
        let modal_width = settings_modal_width(ctx.content_rect().width());
        let scroll_height = settings_scroll_height(ctx.content_rect().height(), self.capturing);
        let mut close_requested = false;

        let response = egui::Modal::new(egui::Id::new("settings_modal"))
            .backdrop_color(egui::Color32::from_black_alpha(96))
            .frame(
                egui::Frame::new()
                    .fill(theme::CARD_BG)
                    .stroke(egui::Stroke::new(1.0, theme::BORDER))
                    .corner_radius(egui::CornerRadius::same(16))
                    .inner_margin(egui::Margin::same(22)),
            )
            .show(ctx, |ui| {
                ui.set_width(modal_width);

                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Settings")
                                .size(23.0)
                                .strong()
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            egui::RichText::new("Manage wallets, history, and parent access")
                                .size(12.0)
                                .color(theme::TEXT_SECONDARY),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized([76.0, 36.0], egui::Button::new("Close"))
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                let capturing = self.capturing;
                egui::ScrollArea::vertical()
                    .id_salt("settings_content")
                    .auto_shrink([false, false])
                    .max_height(scroll_height)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        // README capture: prioritize Coffer Story + danger zone so the new
                        // parent-access section is visible without relying on scroll offset.
                        if capturing {
                            settings_section(
                                ui,
                                "Coffer Story",
                                "Your generated six-object story encrypts the local wallet file. Keep its recovery copy away from this computer.",
                                theme::FAINT_BG,
                                egui::Stroke::new(1.0, theme::BORDER),
                                theme::TEXT_PRIMARY,
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Cofferly does not keep a local bypass. If you lose the story and its recovery card, the encrypted ledger cannot be recovered.",
                                        )
                                        .size(12.0)
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                    ui.add_space(8.0);
                                    let _ = ui.add_sized(
                                        [178.0, 38.0],
                                        egui::Button::new(
                                            egui::RichText::new("Change Coffer Story")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(theme::ACCENT_DARK),
                                    );
                                },
                            );
                            ui.add_space(12.0);
                            settings_section(
                                ui,
                                "Danger zone",
                                "Deleting a wallet permanently removes its balance and transaction history.",
                                theme::ERROR_LIGHT,
                                egui::Stroke::new(1.0, theme::NEGATIVE),
                                theme::NEGATIVE,
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(
                                            "Cofferly always keeps at least one wallet.",
                                        )
                                        .size(12.0)
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                },
                            );
                            return;
                        }

                        settings_section(
                            ui,
                            &format!("{selected_name}'s wallet"),
                            "Update this wallet without changing its transaction history.",
                            theme::CARD_BG,
                            egui::Stroke::new(1.0, theme::BORDER),
                            theme::TEXT_PRIMARY,
                            |ui| {
                                settings_field_label(ui, "Wallet name");
                                let rename_ready = valid_child_name(self.child_name_input.trim())
                                    && self.child_name_input.trim() != selected_name;
                                settings_input_action_row(ui, 112.0, |ui, input_width| {
                                    ui.add_sized(
                                        [input_width, 38.0],
                                        egui::TextEdit::singleline(&mut self.child_name_input)
                                            .hint_text(&selected_name)
                                            .char_limit(40),
                                    );
                                    if ui
                                        .add_enabled(
                                            rename_ready,
                                            egui::Button::new("Save name")
                                                .min_size(egui::vec2(112.0, 38.0)),
                                        )
                                        .clicked()
                                    {
                                        self.rename_selected_child();
                                    }
                                });

                                ui.add_space(12.0);
                                settings_field_label(ui, "Starting balance");
                                ui.label(
                                    egui::RichText::new(
                                        "Changes the opening balance; existing entries stay intact.",
                                    )
                                    .size(11.0)
                                    .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_space(4.0);
                                let balance_ready = self.starting_balance_save_ready();
                                settings_input_action_row(ui, 112.0, |ui, input_width| {
                                    ui.add_sized(
                                        [input_width, 38.0],
                                        egui::TextEdit::singleline(
                                            &mut self.starting_balance_input,
                                        )
                                        .hint_text(format_money_input(starting_balance)),
                                    );
                                    if ui
                                        .add_enabled(
                                            balance_ready,
                                            egui::Button::new("Save balance")
                                                .min_size(egui::vec2(112.0, 38.0)),
                                        )
                                        .clicked()
                                    {
                                        self.update_starting_balance();
                                    }
                                });

                                ui.add_space(14.0);
                                ui.separator();
                                ui.add_space(10.0);
                                settings_field_label(ui, "Latest transaction");
                                ui.label(
                                    egui::RichText::new(
                                        "Remove the newest entry. Undo remains available until the next wallet change.",
                                    )
                                    .size(11.0)
                                    .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_space(6.0);
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_enabled(
                                            has_entries,
                                            egui::Button::new("Remove latest entry")
                                                .min_size(egui::vec2(152.0, 36.0)),
                                        )
                                        .clicked()
                                    {
                                        self.remove_latest_entry();
                                    }
                                    if let Some(removable) = &self.undo {
                                        let enabled = self
                                            .data
                                            .wallets
                                            .get(removable.wallet_index)
                                            .is_some();
                                        if ui
                                            .add_enabled(
                                                enabled,
                                                egui::Button::new(format!(
                                                    "Undo {}",
                                                    format_money(removable.entry.amount_cents)
                                                ))
                                                .min_size(egui::vec2(116.0, 36.0)),
                                            )
                                            .clicked()
                                        {
                                            self.undo_remove_entry();
                                        }
                                    }
                                });
                            },
                        );

                        ui.add_space(12.0);
                        settings_section(
                            ui,
                            "Add another child",
                            "Each child gets a separate wallet stored only on this device.",
                            theme::FAINT_BG,
                            egui::Stroke::new(1.0, theme::BORDER),
                            theme::TEXT_PRIMARY,
                            |ui| {
                                settings_field_label(ui, "Child name");
                                let add_ready = valid_child_name(self.new_child_name_input.trim());
                                settings_input_action_row(ui, 112.0, |ui, input_width| {
                                    ui.add_sized(
                                        [input_width, 38.0],
                                        egui::TextEdit::singleline(&mut self.new_child_name_input)
                                            .hint_text("New child")
                                            .char_limit(40),
                                    );
                                    if ui
                                        .add_enabled(
                                            add_ready,
                                            egui::Button::new(
                                                egui::RichText::new("Add wallet")
                                                    .strong()
                                                    .color(egui::Color32::WHITE),
                                            )
                                            .fill(theme::ACCENT_DARK)
                                            .min_size(egui::vec2(112.0, 38.0)),
                                        )
                                        .clicked()
                                    {
                                        self.add_child_wallet();
                                    }
                                });
                            },
                        );

                        ui.add_space(12.0);
                        settings_section(
                            ui,
                            "Coffer Story",
                            "Your generated six-object story encrypts the local wallet file. Keep its recovery copy away from this computer.",
                            theme::FAINT_BG,
                            egui::Stroke::new(1.0, theme::BORDER),
                            theme::TEXT_PRIMARY,
                            |ui| {
                                ui.label(
                                    egui::RichText::new(
                                        "Cofferly does not keep a local bypass. If you lose the story and its recovery card, the encrypted ledger cannot be recovered.",
                                    )
                                    .size(12.0)
                                    .color(theme::TEXT_SECONDARY),
                                );
                                ui.add_space(8.0);
                                if ui
                                    .add_sized(
                                        [178.0, 38.0],
                                        egui::Button::new(
                                            egui::RichText::new("Change Coffer Story")
                                                .strong()
                                                .color(egui::Color32::WHITE),
                                        )
                                        .fill(theme::ACCENT_DARK),
                                    )
                                    .clicked()
                                {
                                    self.begin_story_change();
                                }
                            },
                        );

                        ui.add_space(12.0);
                        settings_section(
                            ui,
                            "Danger zone",
                            "Deleting a wallet permanently removes its balance and transaction history.",
                            theme::ERROR_LIGHT,
                            egui::Stroke::new(1.0, theme::NEGATIVE),
                            theme::NEGATIVE,
                            |ui| {
                                if !can_delete_wallet {
                                    ui.label(
                                        egui::RichText::new(
                                            "Cofferly always keeps at least one wallet.",
                                        )
                                        .size(12.0)
                                        .color(theme::TEXT_SECONDARY),
                                    );
                                } else if self.confirm_delete_wallet {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Delete {selected_name} and every entry? This cannot be undone."
                                        ))
                                        .size(12.0)
                                        .strong()
                                        .color(theme::NEGATIVE),
                                    );
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        if ui
                                            .add_sized(
                                                [154.0, 38.0],
                                                egui::Button::new(
                                                    egui::RichText::new("Delete permanently")
                                                        .strong()
                                                        .color(egui::Color32::WHITE),
                                                )
                                                .fill(theme::NEGATIVE),
                                            )
                                            .clicked()
                                        {
                                            self.delete_selected_wallet();
                                        }
                                        if ui
                                            .add_sized(
                                                [82.0, 38.0],
                                                egui::Button::new("Cancel"),
                                            )
                                            .clicked()
                                        {
                                            self.confirm_delete_wallet = false;
                                            self.set_status_info("Wallet deletion cancelled.");
                                        }
                                    });
                                } else if ui
                                    .add_sized(
                                        [142.0, 38.0],
                                        egui::Button::new(
                                            egui::RichText::new("Delete this wallet")
                                                .strong()
                                                .color(theme::NEGATIVE),
                                        )
                                        .fill(theme::ERROR_LIGHT)
                                        .stroke(egui::Stroke::new(1.0, theme::NEGATIVE)),
                                    )
                                    .clicked()
                                {
                                    self.confirm_delete_wallet = true;
                                    self.set_status_info(format!(
                                        "Confirm deletion of {selected_name} and all its entries."
                                    ));
                                }
                            },
                        );
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                ui.horizontal(|ui| {
                    let (prefix, status_color) = match self.status.severity {
                        StatusSeverity::Info => ("", theme::TEXT_SECONDARY),
                        StatusSeverity::Success => ("", theme::POSITIVE),
                        StatusSeverity::Error => ("Check: ", theme::NEGATIVE),
                    };
                    ui.vertical(|ui| {
                        ui.set_max_width((modal_width - 150.0).max(220.0));
                        ui.label(
                            egui::RichText::new(format!(
                                "Cofferly {APP_VERSION} · Changes save automatically on this device"
                            ))
                            .size(11.0)
                            .color(theme::TEXT_SECONDARY),
                        );
                        ui.label(
                            egui::RichText::new(format!("{prefix}{}", self.status.text))
                                .size(11.0)
                                .color(status_color),
                        );
                    });

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add_sized(
                                [104.0, 40.0],
                                egui::Button::new(
                                    egui::RichText::new("Done")
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(theme::ACCENT_DARK),
                            )
                            .clicked()
                        {
                            close_requested = true;
                        }
                    });
                });
            });

        if close_requested || response.should_close() {
            self.show_settings = false;
            self.confirm_delete_wallet = false;
        }
    }

    pub fn entry_form(&mut self, ui: &mut egui::Ui) {
        egui::Frame::new()
            .fill(theme::CARD_BG)
            .stroke(egui::Stroke::new(1.0, theme::BORDER))
            .corner_radius(egui::CornerRadius::same(12))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Add a transaction")
                        .strong()
                        .size(15.0)
                        .color(theme::TEXT_PRIMARY),
                );
                ui.label(
                    egui::RichText::new("Record money in or money out")
                        .size(12.0)
                        .color(theme::TEXT_SECONDARY),
                );
                ui.add_space(10.0);

                ui.label(
                    egui::RichText::new("Transaction type")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                ui.scope(|ui| {
                    ui.spacing_mut().button_padding.x = 4.0;
                    ui.columns(2, |columns| {
                        columns[0].selectable_value(
                            &mut self.draft.kind,
                            crate::data::EntryKind::Deposit,
                            egui::RichText::new("Money in").size(13.0),
                        );
                        columns[1].selectable_value(
                            &mut self.draft.kind,
                            crate::data::EntryKind::Deduction,
                            egui::RichText::new("Money out").size(13.0),
                        );
                    });
                });

                ui.label(
                    egui::RichText::new("What was it for?")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                let desc_response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.description)
                        .char_limit(100)
                        .hint_text("e.g. Weekly allowance"),
                );

                ui.label(
                    egui::RichText::new("Amount")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                let amount_response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.amount).hint_text("$0.00"),
                );

                ui.label(
                    egui::RichText::new("Date")
                        .size(11.0)
                        .strong()
                        .color(theme::TEXT_PRIMARY),
                );
                let date_response = ui.add_sized(
                    [ui.available_width(), 36.0],
                    egui::TextEdit::singleline(&mut self.draft.date_input).hint_text("MM/DD/YYYY"),
                );

                let enter_submit = ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && (desc_response.lost_focus()
                        || amount_response.lost_focus()
                        || date_response.lost_focus()
                        || desc_response.has_focus()
                        || amount_response.has_focus()
                        || date_response.has_focus());

                let action = match self.draft.kind {
                    crate::data::EntryKind::Deposit => "Add money",
                    crate::data::EntryKind::Deduction => "Record spending",
                };
                let clicked = ui
                    .add_sized(
                        [ui.available_width(), 40.0],
                        egui::Button::new(
                            egui::RichText::new(action)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(theme::ACCENT_DARK),
                    )
                    .clicked();

                if clicked || enter_submit {
                    self.add_entry();
                }
            });
    }

    pub fn ledger_table(&mut self, ui: &mut egui::Ui) {
        let ledger_sort = self.ledger_sort;
        // Rebuild cache if needed, then take an Arc clone (a pointer bump, not a
        // deep copy) so we do not hold a borrow across the TableBuilder, which
        // may need &mut self.
        let rows = self.cached_ledger_rows();
        let mut toggle_sort = false;
        const ROW_HEIGHT: f32 = 42.0;

        egui_extras::TableBuilder::new(ui)
            .striped(true)
            .resizable(true)
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(egui_extras::Column::initial(100.0).at_least(84.0))
            .column(egui_extras::Column::remainder().at_least(160.0))
            .column(egui_extras::Column::initial(100.0).at_least(82.0))
            .column(egui_extras::Column::initial(110.0).at_least(90.0))
            .header(34.0, |mut header| {
                header.col(|ui| {
                    let tooltip = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest first — click to sort oldest first",
                        LedgerSort::OldestFirst => "Oldest first — click to sort newest first",
                    };
                    let order_label = match ledger_sort {
                        LedgerSort::NewestFirst => "Newest",
                        LedgerSort::OldestFirst => "Oldest",
                    };
                    let response = ui
                        .horizontal(|ui| {
                            ui.set_min_width(ui.available_width());
                            ui.label(
                                egui::RichText::new("Date")
                                    .strong()
                                    .size(12.0)
                                    .color(theme::TEXT_PRIMARY),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(order_label)
                                            .size(10.0)
                                            .color(theme::TEXT_SECONDARY),
                                    );
                                },
                            );
                        })
                        .response;
                    let response = response
                        .interact(egui::Sense::click())
                        .on_hover_text(tooltip);
                    if response.clicked() {
                        toggle_sort = true;
                    }
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Description")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Amount")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
                header.col(|ui| {
                    ui.label(
                        egui::RichText::new("Balance")
                            .strong()
                            .size(12.0)
                            .color(theme::TEXT_PRIMARY),
                    );
                });
            })
            .body(|body| {
                // Virtualized rows: only visible rows are laid out each frame.
                body.rows(ROW_HEIGHT, rows.len(), |mut row| {
                    let index = row.index();
                    let ledger_row = &rows[index];
                    let is_start = matches!(ledger_row.date, LedgerRowDate::Start);

                    row.col(|ui| {
                        let date_text = egui::RichText::new(ledger_row.date.label())
                            .size(if is_start { 10.0 } else { 11.0 })
                            .color(theme::TEXT_SECONDARY);
                        ui.label(date_text);
                    });
                    row.col(|ui| {
                        let desc = if is_start {
                            egui::RichText::new(&ledger_row.description)
                                .size(11.0)
                                .italics()
                                .color(theme::TEXT_SECONDARY)
                        } else {
                            egui::RichText::new(&ledger_row.description)
                                .size(12.0)
                                .color(theme::TEXT_PRIMARY)
                        };
                        ui.label(desc);
                    });
                    row.col(|ui| {
                        let amount_prefix = if ledger_row.amount_cents > 0 && !is_start {
                            "+"
                        } else {
                            ""
                        };
                        let amt = egui::RichText::new(format!(
                            "{amount_prefix}{}",
                            format_money(ledger_row.amount_cents)
                        ))
                        .size(if is_start { 10.0 } else { 11.0 })
                        .color(amount_color(ledger_row.amount_cents));
                        ui.label(amt);
                    });
                    row.col(|ui| {
                        ui.label(
                            egui::RichText::new(format_money(ledger_row.balance_cents))
                                .size(11.0)
                                .strong()
                                .color(balance_color(ledger_row.balance_cents)),
                        );
                    });
                });
            });

        if toggle_sort {
            self.ledger_sort.toggle();
            self.invalidate_ledger_cache();
        }
    }
}

fn draw_story_icon(ui: &egui::Ui, texture: &egui::TextureHandle, rect: egui::Rect) {
    let icon_rect = egui::Rect::from_center_size(
        rect.center_top() + egui::vec2(0.0, 17.0),
        egui::vec2(26.0, 26.0),
    );
    ui.painter().image(
        texture.id(),
        icon_rect,
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        egui::Color32::WHITE,
    );
}

fn settings_modal_width(viewport_width: f32) -> f32 {
    (viewport_width - 48.0).clamp(360.0, 640.0)
}

fn settings_scroll_height(viewport_height: f32, capturing: bool) -> f32 {
    // Documentation captures need the full Settings content (Coffer Story + danger zone).
    let max_height = if capturing { 1000.0 } else { 520.0 };
    (viewport_height - 210.0).clamp(260.0, max_height)
}

fn settings_section<R>(
    ui: &mut egui::Ui,
    title: &str,
    subtitle: &str,
    fill: egui::Color32,
    stroke: egui::Stroke,
    title_color: egui::Color32,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::new()
        .fill(fill)
        .stroke(stroke)
        .corner_radius(egui::CornerRadius::same(12))
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.label(
                egui::RichText::new(title)
                    .size(16.0)
                    .strong()
                    .color(title_color),
            );
            ui.label(
                egui::RichText::new(subtitle)
                    .size(11.0)
                    .color(theme::TEXT_SECONDARY),
            );
            ui.add_space(12.0);
            add_contents(ui)
        })
        .inner
}

fn settings_field_label(ui: &mut egui::Ui, label: &str) {
    ui.label(
        egui::RichText::new(label)
            .size(12.0)
            .strong()
            .color(theme::TEXT_PRIMARY),
    );
    ui.add_space(4.0);
}

fn settings_input_action_row<R>(
    ui: &mut egui::Ui,
    action_width: f32,
    add_contents: impl FnOnce(&mut egui::Ui, f32) -> R,
) -> R {
    let input_width =
        (ui.available_width() - action_width - ui.spacing().item_spacing.x).max(160.0);
    ui.horizontal(|ui| add_contents(ui, input_width)).inner
}

fn draw_pin_coin(ui: &egui::Ui, index: usize, rect: egui::Rect, filled: bool, active: bool) {
    let painter = ui.ctx().layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new(("pin_coin", index)),
    ));
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.35;

    if active {
        painter.circle_filled(center, radius + 8.0, theme::GOLD_LIGHT);
    }

    painter.circle_filled(
        center,
        radius,
        if filled { theme::GOLD } else { theme::CARD_BG },
    );
    painter.circle_stroke(
        center,
        radius,
        egui::Stroke::new(if active { 2.5 } else { 1.5 }, theme::GOLD_DARK),
    );

    if filled {
        let check_color = theme::ACCENT_DARK;
        painter.line_segment(
            [
                center + egui::vec2(-9.0, 0.0),
                center + egui::vec2(-2.0, 7.0),
            ],
            egui::Stroke::new(2.5, check_color),
        );
        painter.line_segment(
            [
                center + egui::vec2(-2.0, 7.0),
                center + egui::vec2(11.0, -8.0),
            ],
            egui::Stroke::new(2.5, check_color),
        );
    } else if active {
        painter.circle_filled(center, 3.5, theme::GOLD_DARK);
    }
}

#[cfg(test)]
mod settings_layout_tests {
    use super::*;

    #[test]
    fn settings_modal_width_adapts_to_the_viewport() {
        assert_eq!(settings_modal_width(320.0), 360.0);
        assert_eq!(settings_modal_width(520.0), 472.0);
        assert_eq!(settings_modal_width(1080.0), 640.0);
    }

    #[test]
    fn settings_content_scroll_height_is_bounded() {
        assert_eq!(settings_scroll_height(420.0, false), 260.0);
        assert_eq!(settings_scroll_height(720.0, false), 510.0);
        assert_eq!(settings_scroll_height(1200.0, false), 520.0);
    }

    #[test]
    fn settings_content_scroll_height_expands_for_documentation_captures() {
        assert_eq!(settings_scroll_height(1200.0, true), 990.0);
    }
}
