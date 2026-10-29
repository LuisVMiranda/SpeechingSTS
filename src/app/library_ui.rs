use super::library_view::SortOrder;
use super::*;

impl App {
    pub(super) fn conversation_sort_control(&mut self, ui: &mut egui::Ui, id: &str) {
        let lang = self.settings.ui_language;
        let previous = self.conversation_view.order;
        ui.horizontal(|ui| {
            ui.label(lang.text("Sort"));
            egui::ComboBox::from_id_salt(id)
                .width((ui.available_width() - 10.0).max(105.0))
                .selected_text(lang.text(self.conversation_view.order.label()))
                .show_ui(ui, |ui| {
                    for order in SortOrder::ALL {
                        ui.selectable_value(
                            &mut self.conversation_view.order,
                            order,
                            lang.text(order.label()),
                        );
                    }
                });
        });
        if self.conversation_view.order == previous {
            return;
        }
        if let Err(error) = self.conversation_view.save() {
            self.status = format!("Could not save conversation sorting: {error}");
            self.log_feedback = self.status.clone();
            self.error = true;
        }
    }
}
