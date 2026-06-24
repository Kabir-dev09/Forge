#include "step_uninstall_confirm.h"
#include "main_window.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>

namespace gui {

StepUninstallConfirm::StepUninstallConfirm(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state)
    : Fl_Group(x, y, w, h), main_window(parent_window), install_state(state)
{
    // Title
    Fl_Box* title = new Fl_Box(x, y + 20, w, 50, "Uninstall Forge");
    title->labelsize(28);
    title->labelfont(FL_HELVETICA_BOLD);

    // Warning icon area
    Fl_Box* warning = new Fl_Box(x, y + 90, w, 30, "Are you sure you want to uninstall Forge?");
    warning->labelsize(16);
    warning->labelfont(FL_HELVETICA_BOLD);

    // Details of what will be removed
    std::string details;
    if (state.scope == core::InstallScope::GLOBAL) {
        details = "The following will be permanently removed:\n"
                  "\n"
                  "  \xe2\x80\xa2  /opt/Forge/\n"
                  "  \xe2\x80\xa2  /usr/local/bin/forge\n"
                  "  \xe2\x80\xa2  /etc/forge/";
    } else {
        details = "The following will be permanently removed:\n"
                  "\n"
                  "  \xe2\x80\xa2  ~/.local/share/Forge/\n"
                  "  \xe2\x80\xa2  ~/.local/bin/forge\n"
                  "  \xe2\x80\xa2  ~/.config/forge/";
    }

    Fl_Box* detail_box = new Fl_Box(x + 80, y + 140, w - 160, 140, details.c_str());
    detail_box->align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_TOP | FL_ALIGN_WRAP);
    detail_box->labelsize(14);
    detail_box->labelcolor(fl_rgb_color(80, 80, 80));
    detail_box->box(FL_FLAT_BOX);
    detail_box->color(fl_rgb_color(245, 245, 245));

    // Navigation
    int btn_y = y + 310;

    Fl_Button* btn_back = new Fl_Button(x + 20, btn_y, 100, 35, "Back");
    btn_back->callback(cb_back, this);
    btn_back->box(FL_FLAT_BOX);
    btn_back->color(fl_rgb_color(220, 220, 220));
    btn_back->labelsize(14);

    Fl_Button* btn_confirm = new Fl_Button(w - 180, btn_y, 160, 35, "Yes, Uninstall");
    btn_confirm->callback(cb_confirm, this);
    btn_confirm->box(FL_FLAT_BOX);
    btn_confirm->color(fl_rgb_color(220, 53, 69)); // destructive red
    btn_confirm->labelcolor(FL_WHITE);
    btn_confirm->labelsize(14);

    end();
}

void StepUninstallConfirm::cb_confirm(Fl_Widget*, void* data) {
    StepUninstallConfirm* self = static_cast<StepUninstallConfirm*>(data);
    self->main_window->show_uninstall_progress_step();
}

void StepUninstallConfirm::cb_back(Fl_Widget*, void* data) {
    StepUninstallConfirm* self = static_cast<StepUninstallConfirm*>(data);
    self->main_window->show_welcome_step();
}

} // namespace gui
