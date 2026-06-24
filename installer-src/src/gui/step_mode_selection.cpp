#include "step_mode_selection.h"
#include "main_window.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>

namespace gui {

StepModeSelection::StepModeSelection(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state)
    : Fl_Group(x, y, w, h), main_window(parent_window) 
{
    Fl_Box* title = new Fl_Box(x, y + 20, w, 50, "Installation Mode");
    title->labelsize(28);
    title->labelfont(FL_HELVETICA_BOLD);
    
    Fl_Box* subtitle = new Fl_Box(x, y + 70, w, 30, "Where would you like to install Forge?");
    subtitle->labelsize(14);
    subtitle->labelfont(FL_HELVETICA);
    subtitle->labelcolor(fl_rgb_color(100, 100, 100));
    
    int content_x = x + 100;
    
    rb_global = new Fl_Round_Button(content_x, y + 130, w - 200, 30, " System-wide (Global)");
    rb_global->type(FL_RADIO_BUTTON);
    rb_global->labelsize(16);
    
    Fl_Box* desc_global = new Fl_Box(content_x + 30, y + 160, w - 200, 40, 
        "Installs Forge for all users.\n"
        "Binary: /opt/Forge/forge\nSymlink: /usr/local/bin/forge");
    desc_global->align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    desc_global->labelsize(12);
    desc_global->labelcolor(fl_rgb_color(120, 120, 120));
    
    rb_user = new Fl_Round_Button(content_x, y + 220, w - 200, 30, " Current User (Local)");
    rb_user->type(FL_RADIO_BUTTON);
    rb_user->labelsize(16);
    
    Fl_Box* desc_user = new Fl_Box(content_x + 30, y + 250, w - 200, 40, 
        "Installs Forge only for you.\n"
        "Binary: ~/.local/share/Forge/forge\nSymlink: ~/.local/bin/forge");
    desc_user->align(FL_ALIGN_LEFT | FL_ALIGN_INSIDE | FL_ALIGN_WRAP);
    desc_user->labelsize(12);
    desc_user->labelcolor(fl_rgb_color(120, 120, 120));
    
    // Default selection
    if (state.is_installed && state.scope == core::InstallScope::GLOBAL) {
        rb_global->value(1);
    } else {
        rb_user->value(1);
    }
    
    // Navigation Buttons
    int btn_y = y + 330;
    Fl_Button* btn_back = new Fl_Button(x + 20, btn_y, 100, 30, "Back");
    btn_back->callback(cb_back, this);
    btn_back->box(FL_FLAT_BOX);
    btn_back->color(fl_rgb_color(220, 220, 220));
    
    Fl_Button* btn_next = new Fl_Button(w - 120, btn_y, 100, 30, "Next");
    btn_next->callback(cb_next, this);
    btn_next->box(FL_FLAT_BOX);
    btn_next->color(fl_rgb_color(0, 120, 215));
    btn_next->labelcolor(FL_WHITE);
    
    end();
}

void StepModeSelection::cb_next(Fl_Widget*, void* data) {
    StepModeSelection* self = static_cast<StepModeSelection*>(data);
    
    if (self->rb_global->value()) {
        self->main_window->get_state().scope = core::InstallScope::GLOBAL;
    } else {
        self->main_window->get_state().scope = core::InstallScope::CURRENT_USER;
    }
    
    // Proceed to next step: Download/Install Progress
    self->main_window->show_progress_step();
}

void StepModeSelection::cb_back(Fl_Widget*, void* data) {
    StepModeSelection* self = static_cast<StepModeSelection*>(data);
    self->main_window->show_welcome_step();
}

} // namespace gui
