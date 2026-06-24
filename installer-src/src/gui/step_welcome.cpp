#include "step_welcome.h"
#include "main_window.h"
#include "../utils/system_utils.h"
#include <FL/Fl_Box.H>
#include <FL/Fl_Button.H>
#include <FL/Fl_Secret_Input.H>
#include <FL/fl_ask.H>

namespace gui {

StepWelcome::StepWelcome(int x, int y, int w, int h, MainWindow* parent_window, const core::InstallerState& state)
    : Fl_Group(x, y, w, h), main_window(parent_window) 
{
    // Title
    Fl_Box* title = new Fl_Box(x, y + 20, w, 50, "Forge Terminal");
    title->labelsize(32);
    title->labelfont(FL_HELVETICA_BOLD);
    
    // Status Text
    Fl_Box* status = new Fl_Box(x, y + 70, w, 30);
    status->labelsize(16);
    status->labelfont(FL_HELVETICA);
    status->labelcolor(fl_rgb_color(100, 100, 100));
    
    int btn_width = 220;
    int btn_height = 45;
    int btn_x = x + (w - btn_width) / 2;
    
    input_sudo = new Fl_Secret_Input(btn_x + 60, y + 120, btn_width - 60, 30, "Sudo Pwd:");
    input_sudo->align(FL_ALIGN_LEFT);
    input_sudo->labelsize(14);
    input_sudo->labelfont(FL_HELVETICA);
    
    if (state.is_installed) {
        std::string status_txt = "Current Status: Installed (Version " + state.installed_version + ")";
        if (state.scope == core::InstallScope::GLOBAL) status_txt += " [Global]";
        else status_txt += " [Current User]";
        
        status->copy_label(status_txt.c_str());
        
        Fl_Button* btn_upgrade = new Fl_Button(btn_x, y + 180, btn_width, btn_height, "Upgrade Forge");
        btn_upgrade->callback(cb_upgrade, this);
        btn_upgrade->box(FL_FLAT_BOX);
        btn_upgrade->color(fl_rgb_color(0, 120, 215)); // Professional blue
        btn_upgrade->labelcolor(FL_WHITE);
        btn_upgrade->labelsize(16);
        
        Fl_Button* btn_uninstall = new Fl_Button(btn_x, y + 240, btn_width, btn_height, "Uninstall Forge");
        btn_uninstall->callback(cb_uninstall, this);
        btn_uninstall->box(FL_FLAT_BOX);
        btn_uninstall->color(fl_rgb_color(220, 53, 69)); // Destructive red
        btn_uninstall->labelcolor(FL_WHITE);
        btn_uninstall->labelsize(16);
    } else {
        status->copy_label("Current Status: Not Installed");
        
        Fl_Button* btn_install = new Fl_Button(btn_x, y + 180, btn_width, btn_height, "Install Forge");
        btn_install->callback(cb_install, this);
        btn_install->box(FL_FLAT_BOX);
        btn_install->color(fl_rgb_color(0, 120, 215)); // Professional blue
        btn_install->labelcolor(FL_WHITE);
        btn_install->labelsize(16);
    }
    
    end();
}

bool StepWelcome::validate_and_store_password() {
    std::string pwd = input_sudo->value();
    if (pwd.empty()) {
        fl_alert("Please enter your sudo password to proceed.");
        return false;
    }
    
    // Test the password
    int res = utils::SystemUtils::run_elevated("true", pwd);
    if (res != 0) {
        fl_alert("Incorrect sudo password. Please try again.");
        return false;
    }
    
    main_window->get_state().sudo_password = pwd;
    return true;
}

void StepWelcome::cb_install(Fl_Widget*, void* data) {
    StepWelcome* self = static_cast<StepWelcome*>(data);
    if (!self->validate_and_store_password()) return;
    self->main_window->show_mode_selection_step();
}

void StepWelcome::cb_upgrade(Fl_Widget*, void* data) {
    StepWelcome* self = static_cast<StepWelcome*>(data);
    if (!self->validate_and_store_password()) return;
    self->main_window->show_mode_selection_step();
}

void StepWelcome::cb_uninstall(Fl_Widget*, void* data) {
    StepWelcome* self = static_cast<StepWelcome*>(data);
    if (!self->validate_and_store_password()) return;
    self->main_window->show_uninstall_confirm_step();
}

} // namespace gui
